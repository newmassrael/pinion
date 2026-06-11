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
/// ```rust,no_run
/// # use pinion_core::test_fixtures::{assert_widget_view_carries_tag, ButtonFixture};
/// # use pinion_core::widgets::button::ButtonState;
/// # use pinion_core::Frame;
/// assert_widget_view_carries_tag::<ButtonFixture>(
///     ButtonState::Idle,
///     &Frame::default(),
/// );
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
/// ```rust,no_run
/// # use pinion_core::reactive::Owner;
/// # use pinion_core::test_fixtures::settle_owner_animations;
/// let owner = Owner::new();
/// owner.run(|| {
///     // Register / re-target Animations against this Owner (e.g.
///     // flip the active ThemeMode so theme_animated() retargets
///     // its ThemeLinear spring toward the new palette).
/// });
/// settle_owner_animations(&owner);
/// owner.run(|| {
///     // Springs at rest — palette-cascade assertions can compare
///     // against `Theme::dark().surface` etc. via exact equality.
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

/// R633 §5.7 §5.22 — substrate-level trait that routes a
/// `'static`-keyed cache binding through the appropriate per-widget
/// `use_*` hook. Lives in `pinion-core` (forward dep direction) so
/// `pinion-rpc` (and any other downstream crate that wires a per-tag
/// cache-binding fixture) consumes the abstraction instead of
/// defining it.
///
/// Pre-R633 the trait lived in `pinion-rpc::test_fixtures` and
/// impl'd for the `pinion-core` widget types via the orphan-rule
/// passport ("downstream trait + upstream type"). The arrangement
/// compiled but inverted the canonical dep direction:
/// `pinion-rpc` ↑ pinion-core via deps, yet `pinion-rpc`'s trait
/// reached *down* into core types. R633 flips the trait + impls
/// into `pinion-core::test_fixtures` (the `test-fixtures` feature
/// gate established in R51.127 §5.41) and `pinion-rpc` re-exports
/// or imports the trait by its substrate name.
///
/// Per the [[test-fixtures-feature-gate-pattern]] memory the
/// canonical Rust pattern for cross-crate test fixtures is:
///
/// 1. trait + impls live in the upstream crate behind a
///    `test-fixtures` (or `test-fixture`) feature
/// 2. downstream `dev-dependencies` activate the feature
/// 3. the trait + impls never reach a production binary
///
/// R633 lands all three: the trait is gated by `cfg(any(test,
/// feature = \"test-fixtures\"))` (via the `pub mod test_fixtures`
/// outer cfg in `lib.rs`); `pinion-rpc`'s `[dev-dependencies]`
/// activates the feature; consumers reach the trait via
/// `pinion_core::test_fixtures::BindableCacheSlot`.
pub trait BindableCacheSlot: Sized + 'static {
    /// Invoke the widget-specific `use_*` hook for `tag`. Must be
    /// called inside an active [`Owner::run`] scope — the helper
    /// fn [`bind_cache_slot`] wraps this contract so per-axis test
    /// sites collapse to a single line.
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self>;
}

impl BindableCacheSlot for crate::widgets::scroll::ScrollState {
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self> {
        crate::widgets::scroll::use_scroll_state(tag)
    }
}

impl BindableCacheSlot for crate::widgets::text_edit::TextEditState {
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self> {
        crate::widgets::text_edit::use_text_edit_state(tag)
    }
}

impl BindableCacheSlot for crate::theme::ThemeProvider {
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self> {
        crate::theme::use_theme(tag)
    }
}

impl BindableCacheSlot for crate::widgets::caret_blink::CaretBlink {
    fn use_in_scope(tag: &'static str) -> std::rc::Rc<Self> {
        crate::widgets::caret_blink::use_caret_blink(tag)
    }
}

/// R633 §5.7 §5.22 — bind a substrate-introspection state slot under
/// `tag` on `owner`. Wraps [`BindableCacheSlot::use_in_scope`] in
/// [`Owner::run`] so each per-axis test site collapses to a single
/// line.
///
/// Generic over `S: BindableCacheSlot`. Call as
/// `bind_cache_slot::<ScrollState>(&owner, "list")` (or
/// `bind_cache_slot::<_>(&owner, "list")` when the return type is
/// inferred from the binding's later use).
#[must_use]
pub fn bind_cache_slot<S: BindableCacheSlot>(
    owner: &Owner,
    tag: &'static str,
) -> std::rc::Rc<S> {
    owner.run(|| S::use_in_scope(tag))
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

/// (R55.D.5 §5.45, lifted R884) `Owner::cache` key for the shared
/// [`ScrollState`](crate::widgets::scroll::ScrollState) the
/// [`ScrollbarMultiFixture`]'s extra scrollbar External attaches.
/// Tests resolve the same `Rc` via
/// `root_owner.run(|| use_scroll_state(MULTI_FIXTURE_SCROLL_KEY))`
/// to observe offsets after a dispatch.
pub const MULTI_FIXTURE_SCROLL_KEY: &str = "sb_state";

/// (R55.D.5 §5.45, lifted R884) Multi-External composition fixture:
/// [`ButtonFixture`] semantics plus a sibling
/// [`ScrollBarExternal`](crate::widgets::scrollbar::ScrollBarExternal)
/// tagged `"sb"`, so [`WidgetCore::create_extra_externals`] is
/// non-empty and the substrate's state scene composes as
/// `Scene::Container([primary, scrollbar])` instead of the bare
/// `Scene::External(primary)`.
///
/// Lifted out of `pinion-runtime::core_shell::tests` at R884 so all
/// three dispatch producers pin the Container-root invariant against
/// the same fixture: `CoreShell::forward` / `send_to_primary`
/// (pinion-runtime), `ShellCore::dispatch_intent` (pinion-shell) and
/// `ShellCoreTui::dispatch_intent` (pinion-tui) — the R884 bug class
/// was exactly "framework send silently no-ops on a Container root",
/// and a bare-External fixture cannot catch it.
pub struct ScrollbarMultiFixture;

impl WidgetCore for ScrollbarMultiFixture {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        <ButtonFixture as WidgetCore>::create_external()
    }

    fn create_extra_externals() -> Vec<crate::widget_core::ExtraExternal> {
        let state = crate::widgets::scroll::use_scroll_state(MULTI_FIXTURE_SCROLL_KEY);
        state.set_max(0, 100);
        let bar = crate::widgets::scrollbar::ScrollBarExternal::new().attach_state(state);
        vec![crate::widget_core::ExtraExternal::new("sb", Box::new(bar))]
    }

    fn tag() -> &'static str {
        <ButtonFixture as WidgetCore>::tag()
    }

    fn read_state(scene: &Scene) -> Self::State {
        // Multi-External composition wraps the primary External in a
        // Container; walk to it by tag (R698.A §5.16 — state name
        // resolution through the WidgetStateName SSOT).
        scene
            .find_external_with_tag(<Self as WidgetCore>::tag())
            .and_then(|n| n.handle.introspect())
            .and_then(|i| i.query("state"))
            .map_or(ButtonState::Idle, |v| match v {
                IntrospectValue::Text(s) => {
                    <ButtonState as crate::WidgetStateName>::from_name_or_default(&s)
                }
                _ => ButtonState::Idle,
            })
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        <ButtonFixture as WidgetCore>::view(state, frame)
    }

    fn event_name(event: Self::Event) -> &'static str {
        <ButtonFixture as WidgetCore>::event_name(event)
    }

    fn title() -> &'static str {
        "MultiExternal"
    }
}

/// R887 §5.49 §5.53 — projected state for [`ContextMenuFixture`]:
/// the popup's open flag plus its anchor point, read straight off the
/// carried [`ContextMenuExternal`]'s `open` / `open_x` / `open_y`
/// query slots.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ContextMenuFixtureState {
    /// `query("open")` — whether the popup is showing.
    pub open: bool,
    /// `query("open_x")` / `query("open_y")` — the press anchor while
    /// open, `None` while closed.
    pub anchor: Option<(f32, f32)>,
}

/// R887 §5.49 §5.53 — secondary-click (right-click) producer fixture:
/// carries a real [`ContextMenuExternal`] and implements
/// [`WidgetCore::apply_secondary_click`] exactly the way the R772
/// `hello-contextmenu` binding does (`invoke("open_at", "<x>,<y>")`),
/// so every dispatch producer of the secondary-click arc pins the
/// same observable — the popup opens at the press point:
///
/// - `pinion-shell::substrate` — the `DeferredInput::SecondaryClick`
///   drain (`scene/click {button: "right"}`) and the winit
///   `MouseInput { button: Right }` path, both through
///   `secondary_click_for_window`.
/// - `pinion-tui::substrate` — the same drain plus the crossterm
///   `Down(Right)` arm, both through `ShellCoreTui::secondary_click`.
///
/// The walker is `find_external_with_tag` (root-shape-agnostic), not
/// a bare `Scene::External` root match — fixtures must stay valid if
/// a future variant composes extras around the primary (the R884
/// silent-drop class).
pub struct ContextMenuFixture;

impl WidgetCore for ContextMenuFixture {
    type State = ContextMenuFixtureState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(crate::widgets::context_menu::ContextMenuExternal::new(3))
    }

    fn tag() -> &'static str {
        "ctx_fixture"
    }

    fn read_state(scene: &Scene) -> Self::State {
        let open_query = |intro: &dyn crate::external::ExternalIntrospect, path: &str| {
            if let Some(IntrospectValue::Float(v)) = intro.query(path) {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "anchor coords are window-local f32 values widened for the wire"
                )]
                Some(v as f32)
            } else {
                None
            }
        };
        scene
            .find_external_with_tag(Self::tag())
            .and_then(|n| n.handle.introspect())
            .map_or_else(ContextMenuFixtureState::default, |intro| {
                let open = matches!(intro.query("open"), Some(IntrospectValue::Bool(true)));
                let anchor = match (open_query(intro, "open_x"), open_query(intro, "open_y")) {
                    (Some(x), Some(y)) => Some((x, y)),
                    _ => None,
                };
                ContextMenuFixtureState { open, anchor }
            })
    }

    fn view(_state: Self::State, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode {
            rect: Rect::new(0, 0, 200, 120),
            tag: Some(Cow::Borrowed("ctx_fixture")),
            children: vec![Scene::Text(TextNode::default())],
            ..Default::default()
        })
    }

    fn apply_secondary_click(scene: &mut Scene, x: f32, y: f32) -> bool {
        let Some(node) = scene.find_external_with_tag_mut(Self::tag()) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        matches!(
            intro.invoke(
                "open_at",
                crate::widgets::context_menu::ContextMenuExternal::open_at_args(x, y),
            ),
            Ok(IntrospectValue::Bool(true))
        )
    }

    fn event_name(event: Self::Event) -> &'static str {
        <ButtonFixture as WidgetCore>::event_name(event)
    }

    fn title() -> &'static str {
        "ContextMenuFixture"
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
