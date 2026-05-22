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
//!
//! R55.G.22 §5.49 — also hosts
//! [`assert_widget_view_carries_tag`], the framework-level regression
//! primitive for the [[composite-paint-root-tag-convention]] (R55.G.17
//! §5.49). Nine widget example bindings carry an identical inline
//! assertion shape since R55.G.17 / G.18 / G.20; the helper extracts
//! that body into a single canonical entry point so a future widget
//! author pins the convention with one trait-bound call site, and so
//! the framework owns one place to evolve the assertion's error
//! message / hook list as the AT bounds attach contract grows.

use std::borrow::Cow;

use crate::command::Command;
use crate::external::{External, IntrospectValue};
use crate::intent::Intent;
use crate::reactive::Owner;
use crate::scene::{ContainerNode, Rect, Scene, TextNode};
use crate::widget_core::WidgetCore;
use crate::widgets::aria;
use crate::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use crate::Frame;

/// R55.G.22 §5.49 — pin the composite paint-root tag convention.
///
/// Asserts that `V::view(state, frame)` returns a [`Scene`] which
/// contains a node tagged [`V::tag()`](WidgetCore::tag) somewhere
/// (depth-first walk via [`Scene::contains_tag`]). Pins the
/// [[composite-paint-root-tag-convention]] (R55.G.17 §5.49) per
/// widget binding — without the tag in the paint scene, AI-side
/// `scene/click` / `scene/key` / `scene/wheel` `{path: V::tag()}`
/// routing and `rect_for_tag` AT bounds attach both fail silently.
///
/// ## Why an `Owner::new()` wrap?
///
/// `WidgetCore::view` is sync and pure per §6.3 R51.27, but some
/// bindings observe [`Owner::current()`](crate::Owner::current)
/// inside the view fn — e.g. `examples/hello-button` registers a
/// hover-progress animation via [[oncecell-weak-self-pointer]] on
/// first paint. Calling the view without an active `Owner` would
/// panic outside the framework wrap; the helper installs a
/// throwaway `Owner::new().run(...)` scope so callers do not have to
/// remember which widget's view fn observes the current owner.
///
/// ## Usage
///
/// ```rust,ignore
/// use pinion_core::test_fixtures::assert_widget_view_carries_tag;
/// use pinion_core::Frame;
///
/// #[test]
/// fn r55_g20_view_contains_composite_paint_root_tag() {
///     assert_widget_view_carries_tag::<MyWidget>(
///         MyWidgetState::default(),
///         &Frame::new(),
///     );
/// }
/// ```
///
/// The `<V>` generic resolves both the view fn and the tag through
/// one trait-bound call site — adding a new widget pins the
/// convention with one line instead of replicating the 5-line
/// inline `assert!(scene.contains_tag(V::tag()), …)` block across
/// every example binding's test module.
///
/// # Panics
///
/// Panics if `V::view(state, frame)` returns a [`Scene`] that does
/// not contain a node tagged [`V::tag()`](WidgetCore::tag) anywhere
/// in its depth-first child / Scroll-content walk — that is exactly
/// the regression the helper exists to surface, so the panic is the
/// designed observable outcome.
pub fn assert_widget_view_carries_tag<V: WidgetCore>(state: V::State, frame: &Frame) {
    let owner = Owner::new();
    let scene = owner.run(|| V::view(state, frame));
    assert!(
        scene.contains_tag(V::tag()),
        "{} view must contain a node tagged {:?} (R55.G.17 §5.49 composite paint-root tag convention)",
        core::any::type_name::<V>(),
        V::tag(),
    );
}

/// R57.X.theme-fade §5.50 — advance `owner`'s registered animations by
/// one second of simulated wall-clock time (60 ticks of 1 / 60 s each)
/// so any in-flight spring settles to rest.
///
/// One second comfortably exceeds the `THEME_FADE_SPRING` Material 3
/// short4 (~200 ms) settle window the R57.X.theme-fade fade uses, and
/// also covers other paint-loop-driven [`Animation`]s
/// (`hello-button` hover progress, `caret_blink`, ...) at their
/// canonical settle horizons. After the call, the next
/// [`ThemeProvider::theme_animated`](crate::theme::ThemeProvider::theme_animated)
/// read returns the new target exactly via the at-rest snap path
/// (R585 §5.50), so widget cascade tests can assert exact equality
/// against palette field values without tolerance.
///
/// ## Usage pattern (R57.X.theme-fade widget cascade)
///
/// ```rust,ignore
/// use pinion_core::reactive::Owner;
/// use pinion_core::test_fixtures::settle_owner_animations;
/// use pinion_core::theme::{use_theme, Theme, ThemeMode};
///
/// let owner = Owner::new();
/// owner.run(|| {
///     use_theme(TAG).set_mode(ThemeMode::Light);
///     let scene = view(state, &Frame::default());
///     assert_eq!(panel_fill(&scene), Theme::light().surface);
///     // Flip mode + trigger the spring re-target by reading the
///     // animated accessor once.
///     use_theme(TAG).set_mode(ThemeMode::Dark);
///     let _ = view(state, &Frame::default());
/// });
/// settle_owner_animations(&owner);
/// owner.run(|| {
///     // Snap engaged — exact equality against the new palette.
///     let scene = view(state, &Frame::default());
///     assert_eq!(panel_fill(&scene), Theme::dark().surface);
/// });
/// ```
///
/// The helper exists at the substrate level rather than as a
/// per-example boilerplate because the same five-line settle pattern
/// appears verbatim in every R57.X widget-binding test that swaps the
/// theme mid-test ([[substrate-incompleteness-signal]] — 9 sites
/// across `pinion-core::theme` substrate tests + 5 example bindings).
pub fn settle_owner_animations(owner: &Owner) {
    for _ in 0..60 {
        owner.tick_animations(1.0 / 60.0);
    }
}

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

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: crate::input::Modifiers,
    ) -> bool {
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

    fn update(_state: Self::State, intent: &Intent) -> Vec<Command> {
        // R51.173 §5.23 R27 — by-value snapshot. The fixture
        // discards the snapshot (no state-dependent branching) and
        // emits one `echo.reply` per incoming Intent so the wiring
        // tests can count the queued commands deterministically.
        //
        // R51.177 §5.23 R27 — **test-only intentionally cascade-
        // unsafe**. Production reducers MUST match specific tags
        // (see `WidgetCore::update`'s "Cascade discipline" section)
        // because a wildcard-emit reducer paired with a handler
        // that echoes its intent through the SCXML send channel
        // forms an infinite loop. The substrate calls `update`
        // twice per cycle (R51.168 incoming + R51.169 drain), and
        // this fixture catches both — that asymmetry is what the
        // R51.168/169 wiring tests assert. Do NOT copy this body
        // into a widget binding.
        vec![Command::new_static(
            "echo.reply",
            IntrospectValue::Text(intent.tag_str().to_string()),
            42,
        )]
    }
}

#[cfg(test)]
mod r55_g22_tests {
    //! R55.G.22 §5.49 — `assert_widget_view_carries_tag` helper
    //! regression. Two arms:
    //!
    //! 1. Pass arm — [`ButtonFixture`] paints a Container tagged
    //!    `"test_btn"` matching [`ButtonFixture::tag()`], so the
    //!    helper must accept it without panicking.
    //! 2. Fail arm — `UntaggedFixture` paints a Container with **no**
    //!    tag (R55.G.19 §5.49 `contains_tag` returns `false`), so
    //!    the helper must panic with the convention-violation
    //!    message.
    //!
    //! The fail arm pins the helper's `assert!` arm against an
    //! accidental tautology refactor (e.g. swapping the assertion
    //! for an always-true predicate would let the fail-arm test
    //! catch it).
    use super::{ButtonFixture, assert_widget_view_carries_tag};
    use crate::external::External;
    use crate::scene::{ContainerNode, Rect, Scene, TextNode};
    use crate::widget_core::WidgetCore;
    use crate::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
    use crate::Frame;

    #[test]
    fn pass_arm_button_fixture_view_carries_tag() {
        // R55.G.22 §5.49 — pass arm. ButtonFixture::view paints a
        // Container with tag="test_btn" matching ButtonFixture::tag(),
        // so the helper accepts it. Doubles as a usage smoke test
        // showing the trait-bound call site.
        assert_widget_view_carries_tag::<ButtonFixture>(ButtonState::Idle, &Frame::default());
    }

    /// Negative fixture for the R55.G.22 fail arm — paints a
    /// Container with **no** tag, so [`Scene::contains_tag`] returns
    /// `false` and the helper must panic.
    struct UntaggedFixture;

    impl WidgetCore for UntaggedFixture {
        type State = ButtonState;
        type Event = ButtonEvent;

        fn create_external() -> Box<dyn External> {
            Box::new(ButtonExternal::new())
        }

        fn tag() -> &'static str {
            "untagged_fixture"
        }

        fn read_state(_: &Scene) -> Self::State {
            ButtonState::Idle
        }

        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            // Deliberately tagless — exercises the helper's panic
            // arm. Mirrors the R55.G.19 Scene::contains_tag "Effect
            // leaf / Container without tag" negative regression
            // arm.
            Scene::Container(ContainerNode {
                rect: Rect::new(0, 0, 32, 48),
                tag: None,
                children: vec![Scene::Text(TextNode::default())],
                ..Default::default()
            })
        }

        fn event_name(_: Self::Event) -> &'static str {
            "__internal__"
        }

        fn title() -> &'static str {
            "Untagged"
        }
    }

    #[test]
    #[should_panic(expected = "view must contain a node tagged \"untagged_fixture\"")]
    fn fail_arm_untagged_fixture_panics() {
        // R55.G.22 §5.49 — fail arm. UntaggedFixture's view paints
        // a Container with `tag: None`, so contains_tag returns
        // false and the helper panics with the convention-
        // violation message. The `#[should_panic(expected = …)]`
        // arm also pins the error message text so the convention
        // reference (R55.G.17 §5.49) stays user-visible.
        assert_widget_view_carries_tag::<UntaggedFixture>(ButtonState::Idle, &Frame::default());
    }

    #[test]
    fn pass_arm_returns_without_observable_side_effects() {
        // R55.G.22 §5.49 — the helper installs a throwaway
        // `Owner::new()` scope per call so widgets whose view fn
        // observes `Owner::current()` (hello-button hover
        // animation per R51.147 §5.28) can be exercised without
        // requiring callers to wrap manually. Repeated calls must
        // remain independent — verify by exercising the pass arm
        // twice in sequence.
        assert_widget_view_carries_tag::<ButtonFixture>(ButtonState::Idle, &Frame::default());
        assert_widget_view_carries_tag::<ButtonFixture>(ButtonState::Hover, &Frame::default());
    }
}
