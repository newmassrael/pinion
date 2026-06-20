//! R51.76 §5.40 — `ShellCore` dispatch-substrate regression tests.
//!
//! Closes the R51.75 verification gap. The §5.40 a11y dispatch path
//! (R51.67 `dispatch_access_action` / R51.70 composite child invoke /
//! R51.71 active-descendant focus / R51.72 incremental dirty / R51.75
//! no-change frame skip) all landed without unit coverage until this
//! round — the methods touched `accesskit_winit::Adapter`,
//! `winit::Window`, and an `EventLoopProxy`, which a `#[test]` cannot
//! synthesise on a headless CI runner.
//!
//! R51.76 extracted [`pinion_shell::ShellCore`] so the dispatch
//! surface is reachable without a winit `EventLoop` or wgpu device;
//! these tests exercise that surface and assert against
//! [`ShellCore::take_redraw_request`], [`ShellCore::focus`],
//! [`ShellCore::compute_access_emit`], and a [`TestView`] whose
//! `apply_key` / `access_child_invoke` impls record calls into
//! per-test static mocks.
//!
//! Test isolation: each `#[test]` acquires `TEST_LOCK` for its whole
//! body and clears the static mock state at entry. Cargo's default
//! multi-thread runner thus serialises the static logs the
//! [`WidgetView`] impl mutates, while running other crates' tests in
//! parallel.

// Mock-method bodies are dictated by the trait contract, not by what
// clippy can infer from the trivial body alone. Scoping the allow to
// this fixture keeps the workspace baseline strict.
#![allow(clippy::unused_self, clippy::unnecessary_wraps)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use pinion_a11y::{
    tag_to_node_id, AccessAction, AccessFocus, AccessNode, AccessValue, AriaRole,
    PinionAccessAction, WidgetA11y,
};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect};
use pinion_core::style::{BoxStyle, Color};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, WidgetView};

// R51.179 §5.41 — `TestRenderer` lives in
// `pinion_shell::test_fixtures` (R51.175 lift). The original inline
// definition here predated the lift and duplicated the same
// `VelloRenderer`-conforming stub byte-for-byte; this round drops
// the duplicate and pulls the shared symbol through the local
// `test-fixtures` feature path (self dev-dep in Cargo.toml).

// ---------- Test-fixture External --------------------------------------
//
// Minimal [`External`] opting in to the introspect channel so the
// `WidgetView::read_state` path resolves a value. The dispatch tests
// drive `apply_key` and `access_child_invoke` directly via the
// statics below — the External itself is just a state slot the shell
// requires to construct its `Scene::External` root.

#[derive(Debug, Default)]
struct TestExternal {
    value: i32,
}

impl External for TestExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }
    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
    fn is_dirty(&self) -> bool {
        // R51.169 §5.23 R27 — test drain mock. `walk_scene_and_drain`
        // skips `drain_intents` unless this returns true; the wiring
        // tests below toggle the static queue to exercise the
        // SCXML-side drain → V::update path.
        EXTERNAL_DRAIN_INTENT.lock().unwrap().is_some()
    }
    fn drain_intents(&mut self, sink: &mut dyn FnMut(pinion_core::Intent)) {
        // R51.169 — pops the queued intent (one-shot) and pushes it
        // into the drain sink so the substrate's tail() collects it.
        if let Some(intent) = EXTERNAL_DRAIN_INTENT.lock().unwrap().take() {
            sink(intent);
        }
    }
    fn on_focus_change(&mut self, focused: bool) {
        // R56.1.h §5.38 §5.39 — record the focus-change wire
        // observation so the focus-lifecycle tests assert the
        // shell substrate's `notify_focus_change` dispatch order
        // (blur on the outgoing widget then focus on the incoming
        // widget) and tag-match gating.
        FOCUS_CHANGE_LOG.lock().unwrap().push(focused);
    }
}

impl ExternalIntrospect for TestExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[("value", "i32")])
    }
    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "value" => Some(IntrospectValue::Int(i64::from(self.value))),
            _ => None,
        }
    }
    fn intervene(
        &mut self,
        _path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        Err(InterveneError::UnknownPath)
    }
    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        Ok(IntrospectValue::Null)
    }
}

// ---------- Mock state for TestView ------------------------------------
//
// Each test acquires `TEST_LOCK` for its whole body and clears the
// statics at entry via `reset_mocks()`. The TestView trait impls then
// log each call into the statics; the test asserts on the logs after
// running the dispatch path. AtomicBool drives the deterministic
// return value (default `false` = "unhandled"; set to `true` to model
// a widget that consumes the key / child invoke).

static TEST_LOCK: Mutex<()> = Mutex::new(());
static APPLY_KEY_LOG: Mutex<Vec<(Option<String>, String)>> = Mutex::new(Vec::new());
static APPLY_KEY_RETURNS: AtomicBool = AtomicBool::new(false);
static CHILD_INVOKE_LOG: Mutex<Vec<(String, AccessAction)>> = Mutex::new(Vec::new());
static CHILD_INVOKE_RETURNS: AtomicBool = AtomicBool::new(false);
// R51.78 §5.37 — keybinding lookup mock. `true` makes
// `TestView::keybinding` return `Some(())`, routing the character
// through the typed-event path (`ShellCore::forward`); `false` returns
// `None`, falling through to `apply_key`.
static KEYBINDING_RETURNS_SOME: AtomicBool = AtomicBool::new(false);
static EVENT_NAME_LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

// R51.146 §5.22 — captures the `Owner::current()` observation each
// time `TestView::view` runs so the R51.146 paint-cycle-owner-wrap
// test can assert the substrate wraps `V::view` in
// `root_owner().run(...)`. Reset by `reset_mocks`.
static OBSERVED_OWNER_ID: Mutex<Option<u64>> = Mutex::new(None);

// R51.152 §5.22 — captures the `Owner::current()` observation each
// time `TestView::apply_key` runs so the R51.152 apply_key-owner-wrap
// test can assert the substrate wraps `V::apply_key` in
// `root_owner().run(...)`.
static OBSERVED_OWNER_ID_APPLY_KEY: Mutex<Option<u64>> = Mutex::new(None);

// R51.168 §5.23 R27 — gate for the `TestView::update` reducer mock.
// Default `false` keeps the reducer a pure no-op so existing dispatch
// tests (which assert specific `pending_commands` counts) are
// unaffected. R51.168 wiring tests flip this to `true` so the reducer
// emits one `test.echo` command per intent and the test can verify
// the command landed on the root owner's queue.
static UPDATE_EMITS_ECHO_COMMAND: AtomicBool = AtomicBool::new(false);
static UPDATE_INTENT_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

// R51.169 §5.23 R27 — one-shot drain queue for `TestExternal`. A
// `forward(()) → tail()` cycle pops the queued intent (if any) and
// surfaces it through `walk_scene_and_drain` so the wiring test
// exercises the SCXML-side drain → V::update path identically to
// real widgets like `ButtonExternal` whose statecharts emit click
// intents on transition.
static EXTERNAL_DRAIN_INTENT: Mutex<Option<pinion_core::Intent>> = Mutex::new(None);

// R56.1.h §5.38 §5.39 — focus-change observation log. Each
// `TestExternal::on_focus_change(focused)` call pushes the boolean
// into this vec so the shell-substrate focus-wire tests assert the
// `notify_focus_change` dispatch order (`blur` then `focus`) and
// the exact tag-matching gates. The single tag in TestView's scene
// ("test") means we only need the focused bool sequence — multi-tag
// scenarios live in pinion-core unit tests where multiple
// TextFieldExternals can coexist.
static FOCUS_CHANGE_LOG: Mutex<Vec<bool>> = Mutex::new(Vec::new());

// R56.2.a §5.13 §5.38 — composition observation log. Each
// `TestView::apply_composition` call pushes a tuple
// `(focused, label)` where `label` is the variant tag (`"start"` /
// `"update(<text>)"` / `"commit(<text>)"` / `"cancel"`) so the
// shell-substrate Ime-wire tests assert the
// `ShellCore::apply_composition` dispatch path: focused-tag carried
// through, event borrow shape, post-handle redraw + revision bump
// on the handled arm.
static APPLY_COMPOSITION_LOG: Mutex<Vec<(Option<String>, String)>> = Mutex::new(Vec::new());

// R56.2.a §5.13 §5.38 — controls whether `TestView::apply_composition`
// reports the event as handled. `true` returns `Some(DispatchTail)`
// from `CoreShell::apply_composition` so `ShellCore` bumps the
// revision + drains the tail; `false` reports unhandled so the
// shell substrate skips the post-handle path. Default `false`
// (mirrors the trait default).
static APPLY_COMPOSITION_RETURNS: AtomicBool = AtomicBool::new(false);

// R56.2.a §5.13 §5.38 — captures the `Owner::current()` observation
// each time `TestView::apply_composition` runs so the R56.2.a wrap
// test can assert the substrate wraps `V::apply_composition` in
// `root_owner().run(...)`. Mirrors `OBSERVED_OWNER_ID_APPLY_KEY`
// (R51.152) symmetrically.
static OBSERVED_OWNER_ID_APPLY_COMPOSITION: Mutex<Option<u64>> = Mutex::new(None);

// R56.2.e §5.13 §5.22 — same shape as APPLY_KEY_LOG /
// APPLY_COMPOSITION_LOG: each `TestView::apply_middle_click` call
// pushes the focused tag so the shell-substrate middle-click-wire
// tests assert the `ShellCore::middle_click` dispatch path forwards
// the focused tag from `FocusManager::focused()`.
static APPLY_MIDDLE_CLICK_LOG: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());

// R56.2.e §5.13 §5.22 — controls whether `TestView::apply_middle_click`
// reports the click as handled. Mirrors APPLY_KEY_RETURNS /
// APPLY_COMPOSITION_RETURNS — `true` yields `Some(DispatchTail)` so
// `ShellCore::middle_click` bumps the revision + drains the tail.
static APPLY_MIDDLE_CLICK_RETURNS: AtomicBool = AtomicBool::new(false);

// R56.2.e §5.13 §5.22 — `Owner::current()` snapshot inside
// `TestView::apply_middle_click` so the R51.152-symmetric wrap test
// can assert `CoreShell::apply_middle_click` runs under
// `root_owner().run(...)`.
static OBSERVED_OWNER_ID_APPLY_MIDDLE_CLICK: Mutex<Option<u64>> = Mutex::new(None);

fn reset_mocks() {
    APPLY_KEY_LOG.lock().unwrap().clear();
    APPLY_KEY_RETURNS.store(false, Ordering::SeqCst);
    CHILD_INVOKE_LOG.lock().unwrap().clear();
    CHILD_INVOKE_RETURNS.store(false, Ordering::SeqCst);
    KEYBINDING_RETURNS_SOME.store(false, Ordering::SeqCst);
    EVENT_NAME_LOG.lock().unwrap().clear();
    *OBSERVED_OWNER_ID.lock().unwrap() = None;
    *OBSERVED_OWNER_ID_APPLY_KEY.lock().unwrap() = None;
    UPDATE_EMITS_ECHO_COMMAND.store(false, Ordering::SeqCst);
    UPDATE_INTENT_LOG.lock().unwrap().clear();
    *EXTERNAL_DRAIN_INTENT.lock().unwrap() = None;
    FOCUS_CHANGE_LOG.lock().unwrap().clear();
    APPLY_COMPOSITION_LOG.lock().unwrap().clear();
    APPLY_COMPOSITION_RETURNS.store(false, Ordering::SeqCst);
    *OBSERVED_OWNER_ID_APPLY_COMPOSITION.lock().unwrap() = None;
    APPLY_MIDDLE_CLICK_LOG.lock().unwrap().clear();
    APPLY_MIDDLE_CLICK_RETURNS.store(false, Ordering::SeqCst);
    *OBSERVED_OWNER_ID_APPLY_MIDDLE_CLICK.lock().unwrap() = None;
}

struct TestView;

impl WidgetCore for TestView {
    type State = i32;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(TestExternal::default())
    }

    fn tag() -> &'static str {
        "test"
    }

    fn read_state(scene: &Scene) -> Self::State {
        match scene {
            Scene::External(node) => {
                match node.handle.introspect().and_then(|i| i.query("value")) {
                    Some(IntrospectValue::Int(v)) => i32::try_from(v).unwrap_or(0),
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    fn view(_state: Self::State, _frame: &Frame) -> Scene {
        // R51.146 §5.22 — record the `Owner::current()` observation
        // so the paint-cycle-owner-wrap test asserts that the
        // substrate runs `V::view` inside `root_owner().run(...)`.
        // Other tests reset the static via `reset_mocks` and leave
        // the value alone; the cost is one atomic op per `view` call.
        *OBSERVED_OWNER_ID.lock().unwrap() =
            pinion_core::Owner::current().map(|o| o.id());
        Scene::Container(
            ContainerNode::new(vec![Scene::Box(BoxNode::filled(
                Rect::default(),
                Color::rgb(0x00, 0x00, 0x00),
            ))])
            .with_tag("test")
            .with_style(BoxStyle::filled(Color::rgb(0x00, 0x00, 0x00)))
            // (R1020 §5.39) Mark the single focus stop so the scene-derived
            // enumeration collects "test" (the pre-R1020 `focusable_tags()`
            // default `vec![tag()]` is retired).
            .with_layout(pinion_core::style::LayoutStyle::new().with_focusable(true)),
        )
    }

    fn event_name(_event: Self::Event) -> &'static str {
        // R51.78 §5.37 — record the event_name lookup so tests that
        // exercise the typed-event path (`handle_character_key` →
        // `forward`) can assert the channel actually fired.
        EVENT_NAME_LOG.lock().unwrap().push("__test__");
        "__test__"
    }

    fn keybinding(_key: &str) -> Option<Self::Event> {
        if KEYBINDING_RETURNS_SOME.load(Ordering::SeqCst) {
            Some(())
        } else {
            None
        }
    }

    fn title() -> &'static str {
        "test"
    }

    fn apply_key(
        _scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        APPLY_KEY_LOG
            .lock()
            .unwrap()
            .push((focused.map(ToOwned::to_owned), key.to_owned()));
        // R51.152 §5.22 — record Owner::current() observation so the
        // apply_key-owner-wrap test asserts CoreShell wraps the call
        // in `root_owner().run(...)`. Other tests reset the static
        // via reset_mocks and ignore it.
        *OBSERVED_OWNER_ID_APPLY_KEY.lock().unwrap() =
            pinion_core::Owner::current().map(|o| o.id());
        APPLY_KEY_RETURNS.load(Ordering::SeqCst)
    }

    fn apply_composition(
        _scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        // `CompositionEvent` is `#[non_exhaustive]` at the crate
        // boundary; the wildcard arm is forward-compat for future
        // variants (e.g. delete_surrounding). Mock records all four
        // current variants by tag.
        let label = match event {
            pinion_core::CompositionEvent::Start => "start".to_owned(),
            pinion_core::CompositionEvent::Update(t) => format!("update({t})"),
            pinion_core::CompositionEvent::Commit(t) => format!("commit({t})"),
            pinion_core::CompositionEvent::Cancel => "cancel".to_owned(),
            _ => "future".to_owned(),
        };
        APPLY_COMPOSITION_LOG
            .lock()
            .unwrap()
            .push((focused.map(ToOwned::to_owned), label));
        // R56.2.a §5.13 §5.38 — record Owner::current() observation
        // so the apply_composition-owner-wrap test asserts
        // CoreShell wraps the call in `root_owner().run(...)`
        // symmetric with R51.152 apply_key.
        *OBSERVED_OWNER_ID_APPLY_COMPOSITION.lock().unwrap() =
            pinion_core::Owner::current().map(|o| o.id());
        APPLY_COMPOSITION_RETURNS.load(Ordering::SeqCst)
    }

    fn apply_middle_click(
        _scene: &mut Scene,
        focused: Option<&str>,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        APPLY_MIDDLE_CLICK_LOG
            .lock()
            .unwrap()
            .push(focused.map(ToOwned::to_owned));
        // R56.2.e §5.13 §5.22 — record Owner::current() observation
        // so the apply_middle_click-owner-wrap test asserts
        // CoreShell wraps the call in `root_owner().run(...)`
        // (R51.152 symmetric arc).
        *OBSERVED_OWNER_ID_APPLY_MIDDLE_CLICK.lock().unwrap() =
            pinion_core::Owner::current().map(|o| o.id());
        APPLY_MIDDLE_CLICK_RETURNS.load(Ordering::SeqCst)
    }

    fn update(
        _state: Self::State,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::Command> {
        // R51.168 §5.23 R27 — log the intent and conditionally emit
        // a `test.echo` command so the wiring test can assert that
        // `ShellCore::dispatch_intent` actually routed the intent
        // through `WidgetCore::update` before the SCXML send.
        UPDATE_INTENT_LOG
            .lock()
            .unwrap()
            .push(intent.tag_str().to_owned());
        if UPDATE_EMITS_ECHO_COMMAND.load(Ordering::SeqCst) {
            vec![pinion_core::Command::new_static(
                "test.echo",
                IntrospectValue::Text(intent.tag_str().to_owned()),
                0,
            )]
        } else {
            Vec::new()
        }
    }
}

impl WidgetA11y for TestView {
    fn access_child_invoke(
        _scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        CHILD_INVOKE_LOG
            .lock()
            .unwrap()
            .push((sub_tag.to_owned(), action));
        CHILD_INVOKE_RETURNS.load(Ordering::SeqCst)
    }
}

impl WidgetView for TestView {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: 8, height: 8 }
    }
}

/// Build an atomic `AccessNode` snapshot. `value` is a `bool` so two
/// snapshots with different values differ via
/// `AccessValue::Bool` — the simplest type that exercises the
/// `PartialEq` diff `compute_access_emit` runs.
fn atomic_node(tag: &str, value: bool) -> AccessNode {
    AccessNode::new(tag, AriaRole::CheckBox)
        .with_name("snapshot")
        .with_value(AccessValue::Bool(value))
}

// ---------- Tests ------------------------------------------------------

#[test]
fn shell_core_new_starts_in_clean_state() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let core = ShellCore::<TestView>::new();
    assert!(
        core.focus().focused().is_none(),
        "focus starts cleared (no widget focused on app boot)",
    );
    assert_eq!(core.revision(), 0, "OCC revision starts at 0");
    assert!(
        !core.redraw_requested(),
        "first frame has no pending redraw before any dispatch runs",
    );
}

#[test]
fn r51_82_composite_focus_routes_to_child_invoke() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    CHILD_INVOKE_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    let rev_before = core.revision();
    // Composite Focus action targets the same `parent#child` shape
    // the Click path consumes. R51.82 routes Focus through
    // `access_child_invoke` so composites can mirror the active
    // descendant without inheriting the Click activation chain.
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test#child_3".to_owned(),
        kind: AccessAction::Focus,
    });

    assert_eq!(
        core.focus().focused(),
        Some("test"),
        "composite Focus still pins focus on the parent tag",
    );
    let child_log = CHILD_INVOKE_LOG.lock().unwrap();
    assert_eq!(
        child_log.len(),
        1,
        "Focus arm now routes through access_child_invoke (R51.82)",
    );
    assert_eq!(child_log[0].0, "child_3");
    assert_eq!(child_log[0].1, AccessAction::Focus);
    assert!(
        APPLY_KEY_LOG.lock().unwrap().is_empty(),
        "Focus never falls back to apply_key — no Enter activation",
    );
    assert!(
        core.revision() > rev_before,
        "composite Focus bumps the revision (active descendant moved)",
    );
}

#[test]
fn r51_82_atomic_focus_unchanged_no_child_invoke() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    CHILD_INVOKE_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    // Atomic tag (no `#`) keeps the pre-R51.82 fast path: just
    // focus_set + request_redraw, no access_child_invoke call.
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test".to_owned(),
        kind: AccessAction::Focus,
    });

    assert_eq!(core.focus().focused(), Some("test"));
    assert!(
        CHILD_INVOKE_LOG.lock().unwrap().is_empty(),
        "atomic Focus must not call access_child_invoke",
    );
}

#[test]
fn r51_67_focus_action_sets_focus_and_requests_redraw() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test".to_owned(),
        kind: AccessAction::Focus,
    });

    assert_eq!(
        core.focus().focused(),
        Some("test"),
        "AccessAction::Focus → FocusManager::focus_set",
    );
    assert!(
        core.take_redraw_request(),
        "AccessAction::Focus requests a redraw (focus ring refresh)",
    );
    assert!(
        APPLY_KEY_LOG.lock().unwrap().is_empty(),
        "Focus arm must NOT call apply_key — pure focus shift, no key event",
    );
}

#[test]
fn r51_67_click_action_routes_through_apply_key_enter() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test".to_owned(),
        kind: AccessAction::Click,
    });

    assert_eq!(core.focus().focused(), Some("test"));
    let log = APPLY_KEY_LOG.lock().unwrap();
    assert_eq!(log.len(), 1, "Click → exactly one apply_key call");
    assert_eq!(
        log[0].0.as_deref(),
        Some("test"),
        "apply_key receives the focused tag as its 'focused' argument",
    );
    assert_eq!(
        log[0].1, "Enter",
        "ARIA Action::Click maps to the W3C 'Enter' key string",
    );
    assert!(core.take_redraw_request());
}

#[test]
fn r51_67_default_action_aliases_click() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test".to_owned(),
        kind: AccessAction::Default,
    });

    let log = APPLY_KEY_LOG.lock().unwrap();
    assert_eq!(log.len(), 1, "Default mirrors Click — one apply_key call");
    assert_eq!(log[0].1, "Enter");
}

#[test]
fn r51_67_increment_routes_to_arrow_right() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test".to_owned(),
        kind: AccessAction::Increment,
    });

    let log = APPLY_KEY_LOG.lock().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0].1, "ArrowRight",
        "WAI-ARIA Slider Increment lowers to the 'ArrowRight' key string",
    );
}

#[test]
fn r51_67_decrement_routes_to_arrow_left() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test".to_owned(),
        kind: AccessAction::Decrement,
    });

    let log = APPLY_KEY_LOG.lock().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0].1, "ArrowLeft",
        "WAI-ARIA Slider Decrement lowers to the 'ArrowLeft' key string",
    );
}

#[test]
fn r51_67_other_action_silent_drop() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test".to_owned(),
        kind: AccessAction::Other,
    });

    assert!(
        APPLY_KEY_LOG.lock().unwrap().is_empty(),
        "Other (unmapped AccessKit action) must not reach apply_key",
    );
    assert!(CHILD_INVOKE_LOG.lock().unwrap().is_empty());
    assert!(
        core.focus().focused().is_none(),
        "Other must not change focus (AT over-request safety)",
    );
    assert!(
        !core.take_redraw_request(),
        "Other must not request a redraw",
    );
}

#[test]
fn r51_70_composite_click_routes_to_child_invoke_first() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    CHILD_INVOKE_RETURNS.store(true, Ordering::SeqCst);
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test#child_2".to_owned(),
        kind: AccessAction::Click,
    });

    assert_eq!(
        core.focus().focused(),
        Some("test"),
        "composite Click focuses the parent tag (R51.70 contract)",
    );
    let child_log = CHILD_INVOKE_LOG.lock().unwrap();
    assert_eq!(child_log.len(), 1, "access_child_invoke called exactly once");
    assert_eq!(child_log[0].0, "child_2", "sub-tag is everything after '#'");
    assert_eq!(child_log[0].1, AccessAction::Click);
    assert!(
        APPLY_KEY_LOG.lock().unwrap().is_empty(),
        "child_invoke returned true → atomic apply_key chain is skipped",
    );
    assert!(
        core.take_redraw_request(),
        "composite-child invoke commits a redraw on success",
    );
}

#[test]
fn r51_70_composite_child_invoke_false_falls_back_to_atomic_chain() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    CHILD_INVOKE_RETURNS.store(false, Ordering::SeqCst);
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test#unknown".to_owned(),
        kind: AccessAction::Click,
    });

    let child_log = CHILD_INVOKE_LOG.lock().unwrap();
    assert_eq!(
        child_log.len(),
        1,
        "child_invoke is tried before the atomic chain falls back",
    );
    let key_log = APPLY_KEY_LOG.lock().unwrap();
    assert_eq!(
        key_log.len(),
        1,
        "child_invoke returned false → atomic apply_key('Enter') fires",
    );
    assert_eq!(key_log[0].1, "Enter");
}

#[test]
fn r51_67_handle_action_request_unknown_target_silent() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    // No prior compute_access_emit → tag map is empty → translate
    // returns None → dispatch is skipped silently.
    let req = accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(0x0DEA_DBEE_F000_0001),
        data: None,
    };
    core.handle_action_request(&req);

    assert!(APPLY_KEY_LOG.lock().unwrap().is_empty());
    assert!(core.focus().focused().is_none());
}

#[test]
fn r51_67_handle_action_request_resolves_via_plan_commit() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    // R51.77 §5.40 — populate the tag map via the plan + commit
    // pair (same shape `AppShell::render` runs on every frame).
    let nodes = vec![atomic_node("test", false)];
    let focus = Some(AccessFocus::atomic("test"));
    let decision = core.plan_access_emit(&nodes, focus.as_ref());
    assert!(decision.should_emit, "initial emit always fires");
    core.commit_access_emit(nodes.clone(), focus.as_ref());

    let req = accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_tree: accesskit::TreeId::ROOT,
        target_node: tag_to_node_id("test"),
        data: None,
    };
    core.handle_action_request(&req);

    let log = APPLY_KEY_LOG.lock().unwrap();
    assert_eq!(log.len(), 1, "valid tag resolves through to apply_key");
    assert_eq!(log[0].1, "Enter");
}

#[test]
fn r51_72_plan_access_emit_initial_marks_all_dirty() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let core = ShellCore::<TestView>::new();
    let nodes = vec![atomic_node("a", false), atomic_node("b", false)];
    let focus = Some(AccessFocus::atomic("a"));
    let decision = core.plan_access_emit(&nodes, focus.as_ref());

    assert!(decision.should_emit, "first frame always emits");
    assert!(decision.initial, "first call sees initial = true");
    assert_eq!(decision.dirty.len(), 2, "initial dirty contains every tag");
    assert!(decision.dirty.contains("a"));
    assert!(decision.dirty.contains("b"));
}

#[test]
fn r51_77_plan_alone_is_pure_no_state_advance() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    // R51.77 §5.40 — two back-to-back plans without a commit in
    // between must return identical decisions. The planner is pure
    // by contract; the textbook regression for the pre-R51.77
    // silent-surprise (mutating `compute_access_emit`) would yield
    // different `dirty` sets and `initial` flags on the two calls.
    let core = ShellCore::<TestView>::new();
    let nodes = vec![atomic_node("a", false)];
    let focus = Some(AccessFocus::atomic("a"));

    let d1 = core.plan_access_emit(&nodes, focus.as_ref());
    let d2 = core.plan_access_emit(&nodes, focus.as_ref());

    assert_eq!(d1.should_emit, d2.should_emit);
    assert_eq!(d1.initial, d2.initial);
    assert_eq!(d1.dirty, d2.dirty);
    assert!(d1.initial, "still initial — no commit ran in between");
}

#[test]
fn r51_75_no_change_frame_skips_emit() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    let nodes = vec![atomic_node("a", false)];
    let focus = Some(AccessFocus::atomic("a"));
    let _initial = core.plan_access_emit(&nodes, focus.as_ref());
    core.commit_access_emit(nodes.clone(), focus.as_ref());

    let second = core.plan_access_emit(&nodes, focus.as_ref());

    assert!(
        !second.should_emit,
        "identical nodes + focus → skip Adapter::update_if_active",
    );
    assert!(!second.initial, "post-commit frames are non-initial");
    assert!(
        second.dirty.is_empty(),
        "no dirty tags when nothing changed",
    );
}

#[test]
fn r51_72_changed_node_only_in_dirty() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    let initial_nodes =
        vec![atomic_node("a", false), atomic_node("b", false)];
    let focus = Some(AccessFocus::atomic("a"));
    let _initial = core.plan_access_emit(&initial_nodes, focus.as_ref());
    core.commit_access_emit(initial_nodes.clone(), focus.as_ref());

    let next_nodes = vec![atomic_node("a", false), atomic_node("b", true)];
    let second = core.plan_access_emit(&next_nodes, focus.as_ref());

    assert!(second.should_emit, "node body changed → emit");
    assert_eq!(second.dirty.len(), 1, "only b's body changed");
    assert!(second.dirty.contains("b"));
    assert!(!second.dirty.contains("a"), "unchanged a is not dirty");
}

#[test]
fn r51_75_focus_change_emits_with_empty_dirty() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    let nodes = vec![atomic_node("a", false), atomic_node("b", false)];
    let focus_a = Some(AccessFocus::atomic("a"));
    let focus_b = Some(AccessFocus::atomic("b"));
    let _initial = core.plan_access_emit(&nodes, focus_a.as_ref());
    core.commit_access_emit(nodes.clone(), focus_a.as_ref());

    let second = core.plan_access_emit(&nodes, focus_b.as_ref());

    assert!(
        second.should_emit,
        "focus shifted to b → emit even though every node body is unchanged",
    );
    assert!(
        second.dirty.is_empty(),
        "focus-only change leaves the dirty set empty",
    );
}

#[test]
fn r51_71_active_descendant_does_not_leak_into_dirty() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    let nodes = vec![atomic_node("group", false)];
    let focus_1 =
        Some(AccessFocus::composite("group", "group#child_1"));
    let focus_2 =
        Some(AccessFocus::composite("group", "group#child_2"));
    let _initial = core.plan_access_emit(&nodes, focus_1.as_ref());
    core.commit_access_emit(nodes.clone(), focus_1.as_ref());

    // Active-descendant shift is a focus change, not a node body
    // change — dirty stays empty but should_emit fires (R51.71
    // roving-tabindex semantics).
    let second = core.plan_access_emit(&nodes, focus_2.as_ref());

    assert!(second.should_emit, "active descendant shifted");
    assert!(
        second.dirty.is_empty(),
        "active descendant lives in AccessFocus, not in AccessNode body",
    );
}

#[test]
fn r51_75_focus_unset_after_set_emits() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    let nodes = vec![atomic_node("a", false)];
    let focus = Some(AccessFocus::atomic("a"));
    let _initial = core.plan_access_emit(&nodes, focus.as_ref());
    core.commit_access_emit(nodes.clone(), focus.as_ref());

    let second = core.plan_access_emit(&nodes, None);

    assert!(
        second.should_emit,
        "focus cleared → emit (AT must observe the focus drop)",
    );
}

// ---------- R51.78 §5.37 winit-free key dispatch ------------------------

#[test]
fn r51_78_focus_traverse_tab_advances_then_wraps() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    // Default focusable_tags = vec![Self::tag()] = ["test"]. Tab on
    // a one-tag list should land on "test" first, then cycle back
    // to the same tag (no change on the second Tab — same tag).
    assert!(
        core.handle_focus_traverse(false),
        "first Tab advances from None → 'test'",
    );
    assert_eq!(core.focus().focused(), Some("test"));
    assert!(
        core.take_redraw_request(),
        "focus change requests redraw",
    );

    // FocusManager::focus_next on a one-tag list keeps focus on the
    // single tag — no change, no redraw.
    let changed_again = core.handle_focus_traverse(false);
    assert_eq!(core.focus().focused(), Some("test"));
    assert!(
        !core.take_redraw_request() && !changed_again,
        "Tab on a one-tag list after first land is a no-op",
    );
}

#[test]
fn r51_78_focus_traverse_shift_tab_calls_focus_prev() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    let advanced = core.handle_focus_traverse(true);

    assert!(
        advanced,
        "Shift+Tab on empty focus advances FocusManager::focus_prev to the last tag",
    );
    assert_eq!(
        core.focus().focused(),
        Some("test"),
        "single-tag list: focus_prev lands on 'test' (the only tag)",
    );
}

#[test]
fn r51_78_character_key_routes_to_apply_key_when_no_binding() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);
    KEYBINDING_RETURNS_SOME.store(false, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    core.handle_character_key("z");

    let log = APPLY_KEY_LOG.lock().unwrap();
    assert_eq!(
        log.len(),
        1,
        "no keybinding → fall through to V::apply_key",
    );
    assert_eq!(log[0].1, "z");
    assert!(
        EVENT_NAME_LOG.lock().unwrap().is_empty(),
        "no event_name lookup when keybinding returned None",
    );
}

#[test]
fn r51_78_character_key_routes_through_forward_when_bound() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    KEYBINDING_RETURNS_SOME.store(true, Ordering::SeqCst);
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    let rev_before = core.revision();
    core.handle_character_key("c");

    assert!(
        APPLY_KEY_LOG.lock().unwrap().is_empty(),
        "keybinding returned Some → typed-event path, NOT apply_key",
    );
    assert_eq!(
        EVENT_NAME_LOG.lock().unwrap().len(),
        1,
        "forward path looks up V::event_name exactly once",
    );
    assert!(
        core.revision() > rev_before,
        "forward bumps the §5.34 OCC revision",
    );
}

// ---------- R51.80 §5.40 deeper extraction ----------------------------

#[test]
fn r51_80_compute_paint_scene_returns_view_output() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    let scene = core.compute_paint_scene(64, 32);

    // TestView::view always returns a tagged Container("test"). The
    // compute step layered compute_layout on top, but the root tag
    // identity is preserved.
    match scene {
        Scene::Container(c) => assert_eq!(c.tag.as_deref(), Some("test")),
        _ => panic!("expected Container root, got {scene:?}"),
    }
}

#[test]
fn r51_80_finalize_frame_snapshots_layout() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    // R890 §5.12 — the stored paint scene IS the layout source; the
    // substrate projects a LayoutNode from it on demand. Before any
    // finalize the projection is honestly absent...
    assert!(
        core.last_paint_layout_for_window(pinion_runtime::DEFAULT_WINDOW).is_none(),
        "no paint yet -> no layout projection",
    );
    let scene = core.compute_paint_scene(64, 32);
    core.finalize_frame(scene);
    // ...and after finalize the projection answers with the painted
    // frame's geometry (drives `scene/layout {viewport: null}`).
    let layout = core
        .last_paint_layout_for_window(pinion_runtime::DEFAULT_WINDOW)
        .expect("finalize stores the scene the projection reads");
    assert_eq!((layout.rect.w, layout.rect.h), (64, 32));
    // Re-running finalize must remain safe and idempotent.
    let scene2 = core.compute_paint_scene(64, 32);
    core.finalize_frame(scene2);
}

#[test]
fn r51_80_window_blurred_then_focused_restores_focus() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    // Seed focus on the only focusable tag.
    core.dispatch_access_action(&PinionAccessAction {
        tag: "test".to_owned(),
        kind: AccessAction::Focus,
    });
    assert_eq!(core.focus().focused(), Some("test"));
    let _ = core.take_redraw_request();

    core.window_blurred(); // FocusManager::save remembers the tag.
    core.window_focused(); // FocusManager::restore reinstates.
    assert_eq!(
        core.focus().focused(),
        Some("test"),
        "ARIA Focus Order: blur + refocus reinstates the saved tag",
    );
}

#[test]
fn r51_80_collect_access_emit_inputs_runs_pipeline() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();

    let mut core = ShellCore::<TestView>::new();
    let scene = core.compute_paint_scene(64, 32);
    // R813 §5.40 — per-window node contribution; the default window id
    // routes through `access_node_for_window`'s default forward to the
    // global `access_node` (TestView opts out → empty either way).
    let (nodes, focus) =
        core.collect_access_emit_inputs(pinion_runtime::DEFAULT_WINDOW, &scene);

    // TestView::access_node defaults to empty (Vec::new) — opt-out
    // path. The substrate must still run name enrichment + bounds
    // assignment without panicking on zero nodes; focus_target
    // defaults to `AccessFocus::atomic(focused)` which is None when
    // no widget is focused.
    assert!(nodes.is_empty(), "TestView opts out of access_node");
    assert!(
        focus.is_none(),
        "no focused tag → access_focus_target returns None",
    );
}

#[test]
fn r51_78_named_key_routes_to_apply_key() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_mocks();
    APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

    let mut core = ShellCore::<TestView>::new();
    core.handle_named_key("ArrowLeft");

    let log = APPLY_KEY_LOG.lock().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0].1, "ArrowLeft",
        "named-key dispatch passes the W3C string through unchanged",
    );
}

// ─────────────────────────────────────────────────────────────────────
// R51.143 §5.28 — paint cycle dt measurement + tick_animations wiring.
// ─────────────────────────────────────────────────────────────────────

mod r51_143_paint_cycle_dt {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::thread::sleep;
    use std::time::Duration;

    use pinion_core::animation::Tickable;

    use super::{reset_mocks, ShellCore, TestView, TEST_LOCK};

    /// Records every `tick(dt)` the substrate dispatches so the test
    /// asserts both the count of dispatches and the magnitude of
    /// the measured delta.
    struct TickRecorder {
        ticks: Cell<u32>,
        last_dt: Cell<f32>,
    }

    impl TickRecorder {
        fn new() -> Self {
            Self {
                ticks: Cell::new(0),
                last_dt: Cell::new(f32::NAN),
            }
        }
    }

    impl Tickable for TickRecorder {
        fn tick(&self, dt: f32) {
            self.ticks.set(self.ticks.get() + 1);
            self.last_dt.set(dt);
        }
        fn is_at_rest(&self, _epsilon: f32) -> bool {
            false
        }
    }

    #[test]
    fn first_compute_paint_scene_ticks_with_zero_dt() {
        // R51.143 — the very first `compute_paint_scene` measures
        // against a missing previous timestamp and passes `dt = 0.0`
        // to the spring solver. At-rest animations stay at rest;
        // construction-time baseline holds.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let recorder = Rc::new(TickRecorder::new());
        let mut core = ShellCore::<TestView>::new();
        core.root_owner().register_animation(recorder.clone());

        let _scene = core.compute_paint_scene(64, 48);

        assert_eq!(
            recorder.ticks.get(),
            1,
            "compute_paint_scene drives one tick per call",
        );
        assert_eq!(
            recorder.last_dt.get().to_bits(),
            0.0_f32.to_bits(),
            "first paint sees dt=0 (no prior timestamp)",
        );
    }

    #[test]
    fn second_compute_paint_scene_measures_real_dt() {
        // R51.143 — the second call sees `dt = now - prev` from the
        // real wall clock. We sleep ~5ms between calls so the
        // recorded dt is bounded above zero and well under one
        // second; the assertion uses an inclusive lower bound and
        // a generous upper bound so the test stays robust under
        // any scheduler jitter.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let recorder = Rc::new(TickRecorder::new());
        let mut core = ShellCore::<TestView>::new();
        core.root_owner().register_animation(recorder.clone());

        let _scene1 = core.compute_paint_scene(64, 48);
        sleep(Duration::from_millis(5));
        let _scene2 = core.compute_paint_scene(64, 48);

        assert_eq!(
            recorder.ticks.get(),
            2,
            "two paints → two ticks",
        );
        let dt = recorder.last_dt.get();
        assert!(
            dt > 0.001,
            "5ms sleep → measured dt must exceed 1ms (saw {dt})",
        );
        assert!(
            dt < 1.0,
            "wall-clock dt should never exceed 1s in a test (saw {dt})",
        );
    }

    #[test]
    fn repeated_compute_paint_scene_drives_animation_repeatedly() {
        // R51.143 — five consecutive paints dispatch five ticks; the
        // substrate never skips, never short-circuits past
        // `Tickable::is_at_rest=false` recorders.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let recorder = Rc::new(TickRecorder::new());
        let mut core = ShellCore::<TestView>::new();
        core.root_owner().register_animation(recorder.clone());

        for _ in 0..5 {
            let _scene = core.compute_paint_scene(64, 48);
        }

        assert_eq!(
            recorder.ticks.get(),
            5,
            "five paints → five ticks (substrate is per-call, never throttled)",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// R51.152 §5.22 — V::apply_key runs under `root_owner().run(...)` so
// `Owner::current()` resolves from inside the keyboard handler.
// Application code (typeahead cursors etc.) can call
// `Owner::current().cache(...)` (R51.150) without thread-local
// workarounds.
// ─────────────────────────────────────────────────────────────────────

mod r51_152_apply_key_owner_wrap {
    use std::sync::atomic::Ordering;

    use super::{
        reset_mocks, ShellCore, TestView, APPLY_KEY_RETURNS, OBSERVED_OWNER_ID_APPLY_KEY,
        TEST_LOCK,
    };

    #[test]
    fn apply_key_runs_under_root_owner() {
        // R51.152 — substrate wraps V::apply_key in
        // root_owner().run(...). Inside TestView::apply_key the
        // OBSERVED_OWNER_ID_APPLY_KEY captures Owner::current().id()
        // and we expect it to match the binding's root_owner().id().
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

        let mut core = ShellCore::<TestView>::new();
        let expected = core.root_owner().id();

        core.apply_key("Enter");

        let observed = *OBSERVED_OWNER_ID_APPLY_KEY.lock().unwrap();
        assert_eq!(
            observed,
            Some(expected),
            "V::apply_key must observe the substrate's root_owner via Owner::current()",
        );
    }

    #[test]
    fn current_returns_to_none_after_apply_key_exits() {
        // R51.152 — RAII pop: the wrap is symmetric.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        APPLY_KEY_RETURNS.store(true, Ordering::SeqCst);

        let mut core = ShellCore::<TestView>::new();
        core.apply_key("Enter");

        assert!(
            pinion_core::Owner::current().is_none(),
            "OwnerHandleGuard must pop on apply_key exit",
        );
    }

    #[test]
    fn apply_key_unhandled_still_wraps_owner() {
        // R51.152 — even when apply_key returns false (unhandled),
        // the substrate still wraps in root_owner.run; Owner::current()
        // inside the handler must resolve.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        APPLY_KEY_RETURNS.store(false, Ordering::SeqCst);

        let mut core = ShellCore::<TestView>::new();
        let expected = core.root_owner().id();
        core.apply_key("AnyKey");

        let observed = *OBSERVED_OWNER_ID_APPLY_KEY.lock().unwrap();
        assert_eq!(observed, Some(expected));
    }
}

// ─────────────────────────────────────────────────────────────────────
// R51.146 §5.22 — view fn runs under `root_owner().run(...)` so
// `Owner::current()` resolves from inside `V::view` without threading
// the [`Owner`] argument through every callee.
// ─────────────────────────────────────────────────────────────────────

mod r51_146_view_fn_owner_wrap {
    use super::{reset_mocks, ShellCore, TestView, OBSERVED_OWNER_ID, TEST_LOCK};

    #[test]
    fn compute_paint_scene_runs_view_under_root_owner() {
        // R51.146 — `ShellCore::compute_paint_scene` wraps the
        // `V::view` call in `root_owner().run(|| ...)`. Inside
        // `TestView::view` we record `Owner::current().id()` and
        // expect to see the binding's `root_owner().id()`.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let expected = core.root_owner().id();

        let _scene = core.compute_paint_scene(64, 48);

        let observed = *OBSERVED_OWNER_ID.lock().unwrap();
        assert_eq!(
            observed,
            Some(expected),
            "view fn must run under the binding's root reactive scope",
        );
    }

    #[test]
    fn dispatch_rpc_synthetic_paint_runs_view_under_root_owner() {
        // R51.146 — the `dispatch_rpc` producer closure (used by
        // `scene/layout {viewport: w,h}` RPC requests for synthetic
        // paint snapshots) also wraps `V::view` in
        // `root_owner.run(...)`. Trigger a synthetic paint through
        // an RPC `scene/layout` request and assert the same owner
        // id observation.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let expected = core.root_owner().id();

        // Synthetic-paint RPC request. The exact response body is
        // irrelevant for this test — we only need the producer
        // closure to fire (which it does whenever the RPC method
        // requests a non-null viewport snapshot).
        let mut resize_noop = |_w: u32, _h: u32| {};
        let _resp = core.dispatch_rpc(
            r#"{"jsonrpc":"2.0","id":1,"method":"scene/layout","params":{"viewport":[80,60]}}"#,
            &mut resize_noop,
        );

        let observed = *OBSERVED_OWNER_ID.lock().unwrap();
        assert_eq!(
            observed,
            Some(expected),
            "dispatch_rpc producer closure must wrap V::view in root_owner.run",
        );
    }

    #[test]
    fn current_returns_to_none_after_compute_paint_scene_exits() {
        // R51.146 — the framework wrap is RAII: once
        // `compute_paint_scene` returns, the handle stack is empty
        // again, so a stray `Owner::current()` from caller code
        // (e.g. test diagnostics) sees `None` and not a dangling
        // reference to the just-painted owner.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let _scene = core.compute_paint_scene(64, 48);

        assert!(
            pinion_core::Owner::current().is_none(),
            "OwnerHandleGuard must pop on compute_paint_scene exit",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// R51.159 §5.23 — pinion-shell tokio binding integration tests.
//
// Verifies the ShellCore-level wiring: set_command_executor injects an
// executor, handle_tail drains pending commands on every dispatch arm,
// dispatch_intent re-feeds an Intent into the SCXML send channel.
//
// The executor under test is BlockOnExecutor (synchronous, deterministic);
// the tokio multi-thread runtime that pinion-shell's run_with_handlers
// installs is unit-tested in pinion-shell/src/executor.rs directly.
// ─────────────────────────────────────────────────────────────────────────

mod r51_159_command_executor_wiring {
    use super::*;
    use std::sync::Arc;

    use pinion_core::{Command, Intent};
    use pinion_runtime::{
        BlockOnExecutor, CommandExecutor, Executor, HandlerFuture, HandlerRegistry, IntentSink,
        VecSink,
    };

    fn echo_handler() -> Arc<dyn pinion_runtime::Handler> {
        Arc::new(|cmd: Command| -> HandlerFuture {
            Box::pin(async move {
                Intent::new_owned(
                    format!("echo.{}", cmd.kind_str()),
                    cmd.payload,
                )
            })
        })
    }

    fn build_executor(
        kinds: &[&'static str],
    ) -> (Arc<CommandExecutor>, Arc<VecSink>) {
        let mut reg = HandlerRegistry::new();
        for k in kinds {
            reg.register(*k, echo_handler());
        }
        let sink = Arc::new(VecSink::new());
        let exec: Arc<dyn Executor> = Arc::new(BlockOnExecutor);
        let sink_dyn: Arc<dyn IntentSink> = sink.clone();
        let cmd_exec = Arc::new(CommandExecutor::new(reg, exec, sink_dyn));
        (cmd_exec, sink)
    }

    #[test]
    fn set_command_executor_installs_and_returns_prior() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        assert!(core.command_executor().is_none(), "fresh ShellCore has no executor");

        let (first, _sink) = build_executor(&[]);
        let first_ptr = Arc::as_ptr(&first).cast::<()>() as usize;
        let prior = core.set_command_executor(first);
        assert!(prior.is_none(), "first install returns no prior");
        let installed = core
            .command_executor()
            .expect("set_command_executor installs Some");
        assert_eq!(
            Arc::as_ptr(installed).cast::<()>() as usize,
            first_ptr,
            "accessor returns the just-installed Arc",
        );

        let (second, _sink_b) = build_executor(&[]);
        let prior = core
            .set_command_executor(second)
            .expect("replace returns prior");
        assert_eq!(
            Arc::as_ptr(&prior).cast::<()>() as usize,
            first_ptr,
            "swap returns the first executor",
        );
    }

    #[test]
    fn forward_drain_pumps_handled_command_through_to_sink() {
        // R51.159 — installing an executor + queuing a Command on the
        // root owner, then triggering any dispatch arm (forward),
        // routes the queued command through the registry and the
        // resolved Intent arrives at the sink.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let (executor, sink) = build_executor(&["http.get"]);
        let mut core = ShellCore::<TestView>::new();
        let _ = core.set_command_executor(executor);

        // Queue a command on the root owner — this would normally
        // come from a reducer or SCXML; for the integration test we
        // inject it directly.
        let scope_id = core.root_owner().id();
        core.root_owner().dispatch_command(Command::new_static(
            "http.get",
            IntrospectValue::Text("/api".into()),
            scope_id,
        ));

        assert_eq!(
            core.root_owner().pending_commands().len(),
            1,
            "command queued before dispatch arm runs",
        );

        // Trigger any dispatch arm — `forward` runs `handle_tail`
        // which calls `core.dispatch_pending_commands`.
        core.forward(());

        assert!(
            core.root_owner().pending_commands().is_empty(),
            "handle_tail drained the queue",
        );
        let drained = sink.drain();
        assert_eq!(drained.len(), 1, "sink received the resolved intent");
        assert_eq!(drained[0].tag_str(), "echo.http.get");
    }

    #[test]
    fn forward_drain_pumps_unhandled_command_logged_only() {
        // R51.159 — when no handler is registered for the command's
        // kind, the drain pump returns the command as unhandled. The
        // shell side currently logs to stderr; the sink stays empty.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let (executor, sink) = build_executor(&["other.kind"]);
        let mut core = ShellCore::<TestView>::new();
        let _ = core.set_command_executor(executor);

        let scope_id = core.root_owner().id();
        core.root_owner().dispatch_command(Command::new_static(
            "missing.kind",
            IntrospectValue::Null,
            scope_id,
        ));
        core.forward(());

        assert!(
            core.root_owner().pending_commands().is_empty(),
            "even unhandled commands consume the queue on drain",
        );
        assert!(sink.is_empty(), "unregistered handler → sink stays empty");
    }

    #[test]
    fn dispatch_intent_bumps_revision() {
        // R51.159 — dispatch_intent re-feeds an Intent through the
        // SCXML send channel. The OCC revision bump observable
        // before/after confirms the path ran. The TestExternal's
        // invoke is a no-op, so no state change is expected; we just
        // verify the surface fires.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let rev_before = core.revision();
        let intent = Intent::new_static("test.evt", IntrospectValue::Null);
        core.dispatch_intent(&intent);
        assert!(
            core.revision() > rev_before,
            "dispatch_intent must bump OCC revision (winit input bypass policy)",
        );
    }

    #[test]
    fn forward_without_executor_keeps_queue_intact() {
        // R51.159 — without an installed executor, handle_tail's
        // drain step is a no-op (Vec::new returned by
        // dispatch_pending_commands); pending commands stay parked.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        assert!(core.command_executor().is_none());

        let scope_id = core.root_owner().id();
        core.root_owner().dispatch_command(Command::new_static(
            "foo",
            IntrospectValue::Null,
            scope_id,
        ));
        core.forward(());
        assert_eq!(
            core.root_owner().pending_commands().len(),
            1,
            "no executor → drain pump is no-op; queue preserved",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// R51.168 §5.23 R27 — `ShellCore::dispatch_intent` wires the
// `CoreShell::route_intent_through_update` substrate API before the
// SCXML `invoke("send", tag)` call. Verifies that the reducer is
// called (intent observed in `UPDATE_INTENT_LOG`) and the produced
// `Vec<Command>` lands on the root owner's queue.
//
// Default-reducer behaviour is asserted indirectly: every existing
// R51.159 test runs through the now-wired dispatch path with
// `UPDATE_EMITS_ECHO_COMMAND` left at `false`, so a regression in
// the wiring would surface as a test failure elsewhere.
// ─────────────────────────────────────────────────────────────────────────

mod r51_168_dispatch_intent_reducer_routing {
    use super::*;
    use pinion_core::Intent;

    #[test]
    fn dispatch_intent_calls_update_reducer() {
        // R51.168 — the wiring observation: dispatch_intent must
        // invoke V::update before the SCXML send, and the intent
        // payload must reach the reducer unchanged.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let intent = Intent::new_static("test.click", IntrospectValue::Null);
        core.dispatch_intent(&intent);

        let log = UPDATE_INTENT_LOG.lock().unwrap();
        assert_eq!(
            *log,
            vec!["test.click".to_string()],
            "dispatch_intent must call WidgetCore::update with the incoming intent",
        );
    }

    #[test]
    fn dispatch_intent_queues_reducer_commands_on_root_owner() {
        // R51.168 — reducer-produced Vec<Command> must land on the
        // substrate's root owner queue so the next handle_tail pump
        // routes them through the registered handlers.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        UPDATE_EMITS_ECHO_COMMAND.store(true, Ordering::SeqCst);

        let mut core = ShellCore::<TestView>::new();
        let intent = Intent::new_static("test.echo_in", IntrospectValue::Null);
        core.dispatch_intent(&intent);

        let pending = core.root_owner().pending_commands();
        assert_eq!(pending.len(), 1, "reducer command must be queued");
        assert_eq!(pending[0].kind_str(), "test.echo");
        assert_eq!(
            pending[0].payload,
            IntrospectValue::Text("test.echo_in".to_string()),
        );
    }

    #[test]
    fn default_reducer_keeps_queue_empty_under_dispatch_intent() {
        // R51.168 — the default `Vec::new()` reducer (mock flag off)
        // leaves the owner queue empty, proving the wiring is
        // semantically transparent when no override is in play.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let intent = Intent::new_static("test.no_op", IntrospectValue::Null);
        core.dispatch_intent(&intent);

        assert!(
            core.root_owner().pending_commands().is_empty(),
            "default reducer must not queue any commands",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// R51.169 §5.23 R27 — `handle_tail` routes every drained
// §5.20 [`Intent`] through `V::update` so widget-side state
// transitions (button.click, toggle.changed, …) emit `Vec<Command>`
// into the same owner queue the async-re-feed path uses. Closes the
// R27 dispatch loop's input → drain → reducer arc on the Vello side.
// ─────────────────────────────────────────────────────────────────────────

mod r51_169_handle_tail_drain_routing {
    use super::*;
    use pinion_core::Intent;

    #[test]
    fn drained_intent_runs_through_update_reducer() {
        // R51.169 — push a synthetic drain intent into the
        // TestExternal queue, set the reducer mock to emit, then
        // call `forward(())` which advances the dispatch path:
        // SCXML `invoke("send", ...)` → no-op → `tail()` →
        // `walk_scene_and_drain` pops the intent → `handle_tail`
        // routes it through `V::update` (R51.169 wiring) → echo
        // command queued on the owner.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        UPDATE_EMITS_ECHO_COMMAND.store(true, Ordering::SeqCst);
        *EXTERNAL_DRAIN_INTENT.lock().unwrap() = Some(Intent::new_static(
            "drain_event",
            IntrospectValue::Null,
        ));

        let mut core = ShellCore::<TestView>::new();
        core.forward(());

        let pending = core.root_owner().pending_commands();
        assert_eq!(
            pending.len(),
            1,
            "drained intent must produce exactly one reducer-emitted command",
        );
        assert_eq!(pending[0].kind_str(), "test.echo");
        // Drain prefix wraps `drain_event` with TestView's tag
        // ("test") per the §5.20 R22 widget-tag convention.
        assert_eq!(
            pending[0].payload,
            IntrospectValue::Text("test.drain_event".to_string()),
        );

        let log = UPDATE_INTENT_LOG.lock().unwrap();
        assert!(
            log.iter().any(|t| t == "test.drain_event"),
            "reducer must observe the drained intent (log={log:?})",
        );
    }

    #[test]
    fn default_reducer_keeps_queue_empty_on_drain() {
        // R51.169 — default reducer leaves the queue empty even when
        // a drain occurs, proving the new wiring is semantically
        // transparent on the no-op path.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        *EXTERNAL_DRAIN_INTENT.lock().unwrap() = Some(Intent::new_static(
            "drain_event",
            IntrospectValue::Null,
        ));

        let mut core = ShellCore::<TestView>::new();
        core.forward(());

        assert!(
            core.root_owner().pending_commands().is_empty(),
            "default reducer must not queue commands on drain",
        );
        let log = UPDATE_INTENT_LOG.lock().unwrap();
        assert!(
            log.iter().any(|t| t == "test.drain_event"),
            "default reducer is still called (logs the intent) but emits no commands",
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// R51.175 §5.41 §5.23 R27 — shared reducer fixture wiring tests.
//
// Closes the process-maturity carry: the R51.168/169 wiring on the
// shell side used a custom `TestView` mock (with `UPDATE_INTENT_LOG`
// + `UPDATE_EMITS_ECHO_COMMAND` statics) while the TUI side already
// drove the same wiring through `pinion_core::test_fixtures::
// EchoButtonFixture`. The asymmetry made the two backends'
// dispatch-loop guarantees harder to compare — a reader had to map
// the mock's static flags onto the fixture's `update` body manually.
//
// R51.175 lifts the Vello-side `WidgetView` impl for
// `EchoButtonFixture` into `pinion_shell::test_fixtures` (gated
// behind the local `test-fixtures` feature, forwarded via the
// `pinion-shell` self-`dev-dependencies` entry) so this sub-module
// can drive the same fixture through `ShellCore::dispatch_intent`
// and the substrate's drain pump. The mock TestView path stays in
// place for the apply_key / event_name / access_child_invoke
// observability tests that still need a per-test static log.
// ─────────────────────────────────────────────────────────────────────────

mod r51_175_shared_fixture_wiring {
    use pinion_core::external::IntrospectValue;
    use pinion_core::test_fixtures::{ContextMenuFixture, EchoButtonFixture, ScrollbarMultiFixture};
    use pinion_core::widgets::button::ButtonState;
    use pinion_core::Intent;
    use pinion_shell::ShellCore;

    #[test]
    fn r887_rpc_right_click_opens_context_menu_at_press_point() {
        // R887 — `scene/click {button: "right"}` end-to-end through
        // the Vello-side producer: dispatch enqueues
        // `DeferredInput::SecondaryClick`, the post-dispatch drain
        // seeds the cursor cache then routes the press-edge one-shot
        // through `secondary_click_for_window` →
        // `CoreShell::apply_secondary_click`, and the popup opens at
        // the press point. Pre-R887 no RPC arc reached
        // `apply_secondary_click` at all — a right-click was
        // human-only input (§2 invariant #2 gap, R881.1 carry).
        let mut core = ShellCore::<ContextMenuFixture>::new();
        assert!(!core.cached_state().open, "popup starts closed");

        let req = r#"{"jsonrpc":"2.0","method":"scene/click","params":{"at":{"x":40.0,"y":25.0},"button":"right"},"id":1}"#;
        let mut no_resize = |_: u32, _: u32| {};
        let resp = core.dispatch_rpc(req, &mut no_resize).expect("response");
        assert!(
            resp.contains(r#""result":null"#),
            "right-click injection succeeds: {resp}"
        );
        let state = *core.cached_state();
        assert!(state.open, "popup must open on the drained right-click");
        assert_eq!(
            state.anchor,
            Some((40.0, 25.0)),
            "popup anchors at the injected press point (cursor-seed leg)",
        );
    }

    #[test]
    fn r884_dispatch_intent_reaches_primary_through_container_root() {
        // R884 — the intent-feedback SCXML send must advance the
        // primary statechart when extras wrap the state scene in a
        // Container (`CoreShell::compose_root`). Pre-R884 this
        // producer matched the bare-External root inline, so every
        // multi-External binding silently dropped the send; the
        // shape-agnostic home is `CoreShell::send_to_primary`.
        let mut core = ShellCore::<ScrollbarMultiFixture>::new();
        assert_eq!(*core.cached_state(), ButtonState::Idle);

        let intent = Intent::new_static("Disable", IntrospectValue::Null);
        core.dispatch_intent(&intent);
        assert_eq!(
            *core.cached_state(),
            ButtonState::Disabled,
            "dispatch_intent must reach the primary through the Container root",
        );
    }

    #[test]
    fn dispatch_intent_queues_reducer_command_on_root_owner() {
        // R51.175 — mirror of pinion-tui's
        // `r51_168_dispatch_intent_reducer_routing::
        // dispatch_intent_queues_reducer_commands_on_root_owner`.
        // The Vello-side substrate must call the shared reducer
        // BEFORE forwarding to the SCXML invoke send and queue the
        // produced command on the root owner — identical observable
        // behaviour to the TUI side.
        let mut core = ShellCore::<EchoButtonFixture>::new();
        let intent = Intent::new_static("echo_btn.tick", IntrospectValue::Null);
        core.dispatch_intent(&intent);
        let pending = core.root_owner().pending_commands();
        assert_eq!(pending.len(), 1, "reducer command must be queued");
        assert_eq!(pending[0].kind_str(), "echo.reply");
        assert_eq!(
            pending[0].payload,
            IntrospectValue::Text("echo_btn.tick".to_string()),
        );
    }

    #[test]
    fn dispatch_intent_accumulates_reducer_commands_across_calls() {
        // R51.175 — mirror of pinion-tui's
        // `dispatch_intent_accumulates_reducer_commands_across_calls`.
        // FIFO accumulation across calls is part of the §5.23 R27
        // contract: a downstream `dispatch_pending_commands` pump
        // must reach every reducer-emitted command in submission
        // order.
        let mut core = ShellCore::<EchoButtonFixture>::new();
        let i1 = Intent::new_static("echo_btn.a", IntrospectValue::Null);
        let i2 = Intent::new_static("echo_btn.b", IntrospectValue::Null);
        core.dispatch_intent(&i1);
        core.dispatch_intent(&i2);
        let pending = core.root_owner().pending_commands();
        assert_eq!(pending.len(), 2);
        assert_eq!(
            pending[0].payload,
            IntrospectValue::Text("echo_btn.a".to_string()),
        );
        assert_eq!(
            pending[1].payload,
            IntrospectValue::Text("echo_btn.b".to_string()),
        );
    }

    #[test]
    fn drained_intent_runs_through_update_reducer() {
        // R51.175 — mirror of pinion-tui's
        // `r51_169_handle_tail_drain_routing::
        // drained_intent_runs_through_update_reducer`. The
        // incoming-intent (carrier) + drained-intent (statechart
        // emission) pair both flow through `V::update`, so a single
        // `KeyboardActivate` dispatch lands two commands: one from
        // the R51.168 incoming pass, one from the R51.169 drain
        // pass.
        let mut core = ShellCore::<EchoButtonFixture>::new();
        let intent = Intent::new_static("KeyboardActivate", IntrospectValue::Null);
        core.dispatch_intent(&intent);

        let pending = core.root_owner().pending_commands();
        assert_eq!(
            pending.len(),
            2,
            "incoming + drained reducer must each queue one command",
        );
        // Carrier payload (incoming reducer pass).
        assert!(
            pending
                .iter()
                .any(|c| c.payload == IntrospectValue::Text("KeyboardActivate".to_string())),
            "incoming reducer must observe `KeyboardActivate`",
        );
        // Drained payload (`<tag>.<kind>` per R51.122 convention).
        assert!(
            pending
                .iter()
                .any(|c| c.payload == IntrospectValue::Text("echo_btn.click".to_string())),
            "drain reducer must observe `echo_btn.click`",
        );
    }
}

// =====================================================================
// R56.1.h §5.38 §5.39 focus lifecycle wire — end-to-end tests asserting
// that the shell substrate's `notify_focus_change` walks the scene tree
// and fires `External::on_focus_change` on the outgoing + incoming
// widgets across every focus-mutating dispatch path (Tab traversal,
// click-to-focus, AT actions, RPC focus/set).
// =====================================================================

mod r56_1_h_focus_lifecycle_wire {
    use super::{
        reset_mocks, FOCUS_CHANGE_LOG, TEST_LOCK, TestView,
    };
    use pinion_a11y::{AccessAction, PinionAccessAction};
    use pinion_shell::ShellCore;

    /// Snapshot the focus-change log without holding the guard
    /// across an assert. Mutex poisoning from a failing assert would
    /// otherwise cascade into every subsequent test that locks the
    /// log (poisoned `Mutex::lock()` returns `Err(PoisonError)`).
    fn snapshot_focus_log() -> Vec<bool> {
        FOCUS_CHANGE_LOG
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Reset the focus log mid-test (e.g. between two dispatch
    /// calls when only the second is being asserted). Recovers a
    /// poisoned mutex via `into_inner` so a prior test failure does
    /// not cascade.
    fn reset_focus_log() {
        FOCUS_CHANGE_LOG
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    #[test]
    fn r56_1_h_focus_traverse_first_tab_fires_focus_true() {
        // Tab from None → "test" must fire on_focus_change(true) on
        // the "test" external (single-tag focusable list).
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        assert!(core.handle_focus_traverse(false));
        assert_eq!(core.focus().focused(), Some("test"));
        assert_eq!(
            snapshot_focus_log(),
            vec![true],
            "first Tab fires on_focus_change(true) on incoming widget",
        );
    }

    #[test]
    fn r56_1_h_focus_traverse_no_change_does_not_fire() {
        // Tab on a one-tag list after the first Tab is a no-op.
        // notify_focus_change's early-return on `before == after`
        // must keep the log empty for the second Tab.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        // First Tab: None → "test" (records true).
        let _ = core.handle_focus_traverse(false);
        reset_focus_log();
        // Second Tab: "test" → "test" (no change, no fire).
        let changed = core.handle_focus_traverse(false);
        assert!(!changed, "single-tag list Tab is a no-op after first land");
        assert!(
            snapshot_focus_log().is_empty(),
            "no on_focus_change fire when focus tag did not change",
        );
    }

    #[test]
    fn r56_1_h_dispatch_access_focus_action_fires_focus_true() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.dispatch_access_action(&PinionAccessAction {
            tag: "test".to_string(),
            kind: AccessAction::Focus,
        });
        assert_eq!(core.focus().focused(), Some("test"));
        assert_eq!(
            snapshot_focus_log(),
            vec![true],
            "AT Focus action fires on_focus_change(true)",
        );
    }

    #[test]
    fn r56_1_h_dispatch_access_click_action_fires_focus_true() {
        // AccessAction::Click does focus_set + apply_a11y_key. The
        // first focus_set path notifies (true); apply_a11y_key's
        // redundant focus_set is observed as no-change.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.dispatch_access_action(&PinionAccessAction {
            tag: "test".to_string(),
            kind: AccessAction::Click,
        });
        assert_eq!(
            snapshot_focus_log(),
            vec![true],
            "AT Click action fires on_focus_change(true) exactly once",
        );
    }

    #[test]
    fn r56_1_h_rpc_focus_set_path_fires_focus_true() {
        // RPC `focus/set` lands through dispatch_rpc, which samples
        // focus_before, dispatches the JSON request (which mutates
        // focus inside the dispatcher), and notifies on diff.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let req = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"test"},"id":1}"#;
        let mut no_resize = |_: u32, _: u32| {};
        let _resp = core.dispatch_rpc(req, &mut no_resize);
        assert_eq!(core.focus().focused(), Some("test"));
        assert_eq!(
            snapshot_focus_log(),
            vec![true],
            "RPC focus/set fires on_focus_change(true) on incoming",
        );
    }

    #[test]
    fn r56_1_h_rpc_focus_set_unchanged_does_not_fire() {
        // RPC `focus/set` against the already-focused tag must not
        // fire (focus_before == focus_after).
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let req = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"test"},"id":1}"#;
        let mut no_resize = |_: u32, _: u32| {};
        let _ = core.dispatch_rpc(req, &mut no_resize);
        reset_focus_log();
        let _ = core.dispatch_rpc(req, &mut no_resize);
        assert!(
            snapshot_focus_log().is_empty(),
            "no-op focus/set must not fire on_focus_change",
        );
    }

    #[test]
    fn r56_1_h_rpc_focus_set_null_after_set_fires_focus_false() {
        // Focus "test" then focus/set with tag=null clears focus.
        // The cleared path fires on_focus_change(false) on the
        // outgoing widget.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let set = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"test"},"id":1}"#;
        let mut no_resize = |_: u32, _: u32| {};
        let _ = core.dispatch_rpc(set, &mut no_resize);
        reset_focus_log();
        let clear = r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":null},"id":2}"#;
        let _ = core.dispatch_rpc(clear, &mut no_resize);
        assert_eq!(core.focus().focused(), None);
        assert_eq!(
            snapshot_focus_log(),
            vec![false],
            "focus/set null fires on_focus_change(false) on outgoing",
        );
    }

    #[test]
    fn r56_1_h_shift_tab_first_press_fires_focus_true() {
        // Shift+Tab on empty focus uses focus_prev which wraps to
        // the last tag — single-tag list lands on "test" too.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let _ = core.handle_focus_traverse(true);
        assert_eq!(core.focus().focused(), Some("test"));
        assert_eq!(
            snapshot_focus_log(),
            vec![true],
            "Shift+Tab from None also fires on_focus_change(true)",
        );
    }
}

#[cfg(test)]
mod r56_2_a_apply_composition_wire {
    //! R56.2.a §5.13 §5.38 — `ShellCore::apply_composition` wiring
    //! regression. Closes the `WindowEvent::Ime` substrate path:
    //!
    //! - Forwards the focused tag (via `FocusManager::focused()`) and
    //!   borrowed `CompositionEvent` to `CoreShell::apply_composition`.
    //! - On handled (`true` from `V::apply_composition`): bumps the
    //!   §5.34 revision and runs `handle_tail` (paint-state re-read +
    //!   redraw + intent drain).
    //! - On unhandled (`false`): swallows quietly — no redraw, no
    //!   revision bump (composition events have no fallback arc
    //!   unlike `apply_key`'s scroll fallback).
    //! - Wraps the trait call in `root_owner.run` (the R51.152
    //!   `apply_key` wrap symmetric arc) so application-side
    //!   `Owner::cache` calls inside the composition handler resolve
    //!   to the binding's root scope.
    //!
    //! The full winit↔CompositionEvent mapping (Enabled / Preedit /
    //! Commit / Disabled with the `was_composing` state machine)
    //! lives in `pinion-shell::app::AppShell::window_event` — those
    //! arms have no observable substrate without a real winit
    //! `EventLoop`, so the per-mapping coverage lives in the unit
    //! tests on `pinion-shell::app::winit_ime_to_composition` (added
    //! alongside the arm in this same round).
    use super::{
        reset_mocks, APPLY_COMPOSITION_LOG, APPLY_COMPOSITION_RETURNS,
        OBSERVED_OWNER_ID_APPLY_COMPOSITION, TEST_LOCK, TestView,
    };
    use pinion_a11y::{AccessAction, PinionAccessAction};
    use pinion_shell::ShellCore;
    use std::sync::atomic::Ordering;

    fn snapshot_log() -> Vec<(Option<String>, String)> {
        APPLY_COMPOSITION_LOG.lock().unwrap().clone()
    }

    // Focus "test" via the AccessAction::Focus side door (the same
    // path the other R56.1.h tests use). Returns the ShellCore ready
    // for composition dispatch with `focus().focused() == Some("test")`.
    fn shell_with_focused_test() -> ShellCore<TestView> {
        let mut core = ShellCore::<TestView>::new();
        core.dispatch_access_action(&PinionAccessAction {
            tag: "test".to_owned(),
            kind: AccessAction::Focus,
        });
        // Drain side-effects of the focus arc (redraw request,
        // notify_focus_change(true) recorded on FOCUS_CHANGE_LOG —
        // reset_mocks clears it on the next test).
        let _ = core.take_redraw_request();
        core
    }

    #[test]
    fn r56_2_a_apply_composition_forwards_focused_tag_and_event_label() {
        // Focused == Some("test") after dispatch_access_action(Focus);
        // ShellCore reads the focus manager and forwards through
        // CoreShell which wraps and calls TestView::apply_composition.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        APPLY_COMPOSITION_RETURNS.store(false, Ordering::SeqCst);

        let mut core = shell_with_focused_test();
        assert_eq!(core.focus().focused(), Some("test"));

        core.apply_composition(&pinion_core::CompositionEvent::Start);
        core.apply_composition(&pinion_core::CompositionEvent::Update("ha".to_owned()));
        core.apply_composition(&pinion_core::CompositionEvent::Commit("\u{D55C}".to_owned()));
        core.apply_composition(&pinion_core::CompositionEvent::Cancel);

        let log = snapshot_log();
        assert_eq!(log.len(), 4, "four dispatches → four V::apply_composition calls");
        assert_eq!(log[0], (Some("test".to_owned()), "start".to_owned()));
        assert_eq!(log[1], (Some("test".to_owned()), "update(ha)".to_owned()));
        assert_eq!(log[2], (Some("test".to_owned()), "commit(\u{D55C})".to_owned()));
        assert_eq!(log[3], (Some("test".to_owned()), "cancel".to_owned()));
    }

    #[test]
    fn r56_2_a_apply_composition_carries_none_focus_when_blurred() {
        // Without focus_set, FocusManager::focused() returns None.
        // ShellCore forwards None as the focused-tag argument, and
        // widget impls (mirroring `apply_key`) short-circuit on the
        // roving-tabindex predicate. The mock records the None.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.apply_composition(&pinion_core::CompositionEvent::Start);

        let log = snapshot_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].0, None, "blurred shell carries None as focused tag");
    }

    #[test]
    fn r56_2_a_handled_arm_bumps_revision() {
        // APPLY_COMPOSITION_RETURNS=true forces V::apply_composition
        // to report handled, so CoreShell yields Some(DispatchTail).
        // ShellCore bumps the §5.34 revision (mirrors apply_key's
        // behaviour). handle_tail's redraw + read-state arc only
        // fires on visible state changes; TestView's state stays at
        // 0 so this test only pins the revision bump.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        APPLY_COMPOSITION_RETURNS.store(true, Ordering::SeqCst);

        let mut core = ShellCore::<TestView>::new();
        let before = core.revision();
        core.apply_composition(&pinion_core::CompositionEvent::Start);
        assert_eq!(
            core.revision(),
            before + 1,
            "handled apply_composition bumps the §5.34 revision",
        );
    }

    #[test]
    fn r56_2_a_unhandled_arm_skips_revision_bump() {
        // APPLY_COMPOSITION_RETURNS=false (default) reports unhandled
        // so CoreShell yields None. ShellCore must NOT bump the
        // revision or request a redraw — composition events have no
        // fallback arc, the substrate stays silent.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let before = core.revision();
        core.apply_composition(&pinion_core::CompositionEvent::Cancel);
        assert_eq!(
            core.revision(),
            before,
            "unhandled apply_composition leaves the revision untouched",
        );
        assert!(
            !core.take_redraw_request(),
            "unhandled apply_composition does not request a redraw",
        );
    }

    #[test]
    fn r56_2_a_apply_composition_runs_under_root_owner_scope() {
        // Symmetric with R51.152 apply_key wrap: the trait call must
        // run under `root_owner().run(...)`. Verify by capturing
        // Owner::current() inside V::apply_composition and asserting
        // it matches the binding's root owner id (via the existing
        // OBSERVED_OWNER_ID_APPLY_KEY arc — same wrap shape, same id).
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        // Pre-dispatch: no active wrap.
        assert!(pinion_core::Owner::current().is_none());
        core.apply_composition(&pinion_core::CompositionEvent::Start);
        // The mock recorded Owner::current().map(|o| o.id()); the
        // wrap is in place iff this is Some(_).
        assert!(
            OBSERVED_OWNER_ID_APPLY_COMPOSITION.lock().unwrap().is_some(),
            "apply_composition must run inside root_owner.run(...)",
        );
        // Post-dispatch: wrap popped.
        assert!(
            pinion_core::Owner::current().is_none(),
            "wrap pops on exit",
        );
    }
}

mod r56_2_e_apply_middle_click_wire {
    //! R56.2.e §5.13 §5.22 — `ShellCore::middle_click` (the paste
    //! funnel) wiring regression:
    //!
    //! - Forwards the focused tag (via `FocusManager::focused()`) and
    //!   current modifier state to `CoreShell::apply_middle_click`.
    //! - On handled (`true` from `V::apply_middle_click`): bumps the
    //!   §5.34 revision and runs `handle_tail` (paint-state re-read
    //!   + redraw + intent drain).
    //! - On unhandled (`false`): swallows quietly — no redraw, no
    //!   revision bump (middle-click has no fallback arc).
    //! - Wraps the trait call in `root_owner.run` (R51.152 symmetric)
    //!   so application-side `Owner::cache` calls inside the
    //!   `apply_middle_click` handler resolve to the binding's root
    //!   scope.
    //!
    //! R881 §5.35 re-sequenced WHO calls the funnel: the winit
    //! `{ Middle, Pressed }` arm no longer pastes — `middle_click`
    //! fires from `ShellCore::middle_released_for_window` on the
    //! router's release-in-place verdict (`PanRelease::Click`); a
    //! drag-to-pan never reaches it. That choreography is pinned by
    //! the `r881_middle_gesture_paste_on_release` mod below; the
    //! funnel mechanics pinned here are unchanged. The winit
    //! `MouseButton::Middle` arms live in
    //! `pinion-shell::app::AppShell::window_event` and have no
    //! observable substrate without a real winit `EventLoop` — they
    //! are verified by inspection alongside the R56.2.a
    //! `WindowEvent::Ime` arm.

    use super::{
        reset_mocks, APPLY_MIDDLE_CLICK_LOG, APPLY_MIDDLE_CLICK_RETURNS,
        OBSERVED_OWNER_ID_APPLY_MIDDLE_CLICK, TEST_LOCK, TestView,
    };
    use pinion_a11y::{AccessAction, PinionAccessAction};
    use pinion_shell::ShellCore;
    use std::sync::atomic::Ordering;

    fn snapshot_log() -> Vec<Option<String>> {
        APPLY_MIDDLE_CLICK_LOG.lock().unwrap().clone()
    }

    fn shell_with_focused_test() -> ShellCore<TestView> {
        let mut core = ShellCore::<TestView>::new();
        core.dispatch_access_action(&PinionAccessAction {
            tag: "test".to_owned(),
            kind: AccessAction::Focus,
        });
        let _ = core.take_redraw_request();
        core
    }

    #[test]
    fn r56_2_e_middle_click_forwards_focused_tag() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        APPLY_MIDDLE_CLICK_RETURNS.store(false, Ordering::SeqCst);

        let mut core = shell_with_focused_test();
        assert_eq!(core.focus().focused(), Some("test"));

        core.middle_click();

        let log = snapshot_log();
        assert_eq!(log.len(), 1, "one middle_click dispatches one V::apply_middle_click");
        assert_eq!(log[0], Some("test".to_owned()));
    }

    #[test]
    fn r56_2_e_middle_click_carries_none_focus_when_blurred() {
        // Without focus_set, FocusManager::focused() returns None.
        // ShellCore forwards None as the focused-tag argument, and
        // widget impls (mirroring `apply_key`) short-circuit on the
        // roving-tabindex predicate. The mock records the None.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.middle_click();

        let log = snapshot_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], None, "blurred shell carries None as focused tag");
    }

    #[test]
    fn r56_2_e_handled_arm_bumps_revision() {
        // APPLY_MIDDLE_CLICK_RETURNS=true forces V::apply_middle_click
        // to report handled, so CoreShell yields Some(DispatchTail).
        // ShellCore bumps the §5.34 revision (mirrors apply_key /
        // apply_composition behaviour).
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();
        APPLY_MIDDLE_CLICK_RETURNS.store(true, Ordering::SeqCst);

        let mut core = ShellCore::<TestView>::new();
        let before = core.revision();
        core.middle_click();
        assert_eq!(
            core.revision(),
            before + 1,
            "handled middle_click bumps the §5.34 revision",
        );
    }

    #[test]
    fn r56_2_e_unhandled_arm_skips_revision_bump() {
        // APPLY_MIDDLE_CLICK_RETURNS=false (default) reports unhandled
        // so CoreShell yields None. ShellCore must NOT bump the
        // revision or request a redraw — middle-click has no
        // fallback arc, the substrate stays silent.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let before = core.revision();
        core.middle_click();
        assert_eq!(
            core.revision(),
            before,
            "unhandled middle_click leaves the revision untouched",
        );
        assert!(
            !core.take_redraw_request(),
            "unhandled middle_click does not request a redraw",
        );
    }

    #[test]
    fn r56_2_e_middle_click_runs_under_root_owner_scope() {
        // Symmetric with R51.152 apply_key wrap + R56.2.a
        // apply_composition wrap: the trait call must run under
        // `root_owner().run(...)`. The mock captures Owner::current()
        // inside V::apply_middle_click; the wrap is in place iff the
        // observation is `Some(_)`.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        assert!(pinion_core::Owner::current().is_none());
        core.middle_click();
        assert!(
            OBSERVED_OWNER_ID_APPLY_MIDDLE_CLICK.lock().unwrap().is_some(),
            "apply_middle_click must run inside root_owner.run(...)",
        );
        assert!(
            pinion_core::Owner::current().is_none(),
            "wrap pops on exit",
        );
    }

    #[test]
    fn r56_2_e_default_apply_middle_click_returns_false() {
        // The trait default returns false. Verify by calling the trait
        // method directly with a scratch scene + None focus + empty
        // modifiers — no mock interception. Default false is the
        // baseline for non-text widgets.
        use pinion_core::scene::{ContainerNode, Scene};
        use pinion_core::WidgetCore;
        struct PlainView;
        impl pinion_a11y::WidgetA11y for PlainView {}
        impl WidgetCore for PlainView {
            type State = ();
            type Event = ();
            fn create_external() -> Box<dyn pinion_core::External> {
                unreachable!("default trait test never instantiates an external")
            }
            fn tag() -> &'static str {
                "plain"
            }
            fn read_state(_scene: &pinion_core::scene::Scene) -> Self::State {}
            fn view(_state: Self::State, _frame: &pinion_core::Frame) -> pinion_core::scene::Scene {
                pinion_core::scene::Scene::Container(ContainerNode::new(vec![]).with_tag("plain"))
            }
            fn event_name(_event: Self::Event) -> &'static str {
                ""
            }
            fn title() -> &'static str {
                "PlainView"
            }
        }
        let mut scene = Scene::Container(ContainerNode::new(vec![]).with_tag("plain"));
        let handled = PlainView::apply_middle_click(
            &mut scene,
            Some("plain"),
            pinion_core::Modifiers::empty(),
        );
        assert!(!handled, "default trait impl must return false");
    }
}

#[cfg(test)]
mod r881_middle_gesture_paste_on_release {
    //! R881 §5.35 §5.49 — the middle-button press/release pair
    //! choreography at the `ShellCore` tier. Pre-R881 the winit
    //! `{ Middle, Pressed }` arm pasted immediately; R881 defers the
    //! paste to a release-in-place (`PanRelease::Click` from the
    //! router's `DragLatch`) so a drag-to-pan never pastes — and the
    //! paste funnel itself (`ShellCore::middle_click`, covered by the
    //! `r56_2_e` mod above) is unchanged, just re-sequenced.

    use super::{reset_mocks, APPLY_MIDDLE_CLICK_LOG, TEST_LOCK, TestView};
    use pinion_runtime::PointerId;
    use pinion_shell::ShellCore;

    fn paste_count() -> usize {
        APPLY_MIDDLE_CLICK_LOG.lock().unwrap().len()
    }

    #[test]
    fn r881_press_does_not_paste_release_in_place_pastes_once() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.middle_pressed(PointerId::MOUSE);
        assert_eq!(paste_count(), 0, "paste is deferred past the press");
        core.middle_released(PointerId::MOUSE);
        assert_eq!(paste_count(), 1, "release-in-place runs the paste funnel once");
        // A trailing spurious release must not double-paste.
        core.middle_released(PointerId::MOUSE);
        assert_eq!(paste_count(), 1, "NoPress release is silent");
    }

    #[test]
    fn r881_middle_drag_never_pastes() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        // Seed a cursor so the press opens a pan-capable gesture, then
        // stray far past the DragLatch dead zone before releasing.
        core.cursor_moved(PointerId::MOUSE, 10.0, 10.0);
        core.middle_pressed(PointerId::MOUSE);
        core.cursor_moved(PointerId::MOUSE, 60.0, 60.0);
        core.middle_released(PointerId::MOUSE);
        assert_eq!(paste_count(), 0, "a moved middle drag is a pan, never a paste");
    }
}

mod r882_space_chord_pan {
    //! R882 §5.35 §5.39 — the Space-hold pan chord at the `ShellCore`
    //! tier (the Figma / Photoshop hand tool): while Space is held,
    //! `mouse_pressed_for_window` routes the left press into the
    //! router's pan channel instead of the widget press arc, and
    //! `mouse_released_for_window` resolves by the *gesture in flight*
    //! (gesture-capture), not the chord's current state.
    //!
    //! Observability trick: the middle-button paste funnel is the
    //! discriminator. While ANY pan-class gesture owns the pointer, a
    //! middle press is refused (R881.1 exclusivity → release =
    //! `NoPress` → no paste); with no gesture in flight, a middle
    //! press-release-in-place pastes once. So "did the left press open
    //! a pan gesture?" is observable as "does a middle click paste?" —
    //! no paint scene required.

    use super::{reset_mocks, APPLY_MIDDLE_CLICK_LOG, TEST_LOCK, TestView};
    use pinion_runtime::PointerId;
    use pinion_shell::ShellCore;

    fn paste_count() -> usize {
        APPLY_MIDDLE_CLICK_LOG.lock().unwrap().len()
    }

    /// A middle press-release-in-place against `core`, reporting
    /// whether it pasted — `false` means a live gesture owns the
    /// pointer (the middle press was refused).
    fn middle_click_pastes(core: &mut ShellCore<TestView>) -> bool {
        let before = paste_count();
        core.middle_pressed(PointerId::MOUSE);
        core.middle_released(PointerId::MOUSE);
        paste_count() > before
    }

    #[test]
    fn r882_space_chord_routes_left_press_into_pan_channel() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.cursor_moved(PointerId::MOUSE, 50.0, 50.0);
        core.note_key_state("Space", true);
        assert!(core.space_held());
        core.mouse_pressed(PointerId::MOUSE);
        assert!(
            !middle_click_pastes(&mut core),
            "the chorded left press opened a pan gesture, so the pointer is owned",
        );
        core.mouse_released(PointerId::MOUSE);
        assert!(
            middle_click_pastes(&mut core),
            "the left release resolved the pan gesture and freed the pointer",
        );
    }

    #[test]
    fn r882_without_chord_a_hoverless_left_press_leaves_pointer_free() {
        // Control for the discriminator above: the same press without
        // the chord opens nothing (no hover target, no pan channel),
        // so the middle click pastes — proving the refusal in the
        // chord test comes from the pan gesture, not the left press
        // itself.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.cursor_moved(PointerId::MOUSE, 50.0, 50.0);
        core.mouse_pressed(PointerId::MOUSE);
        assert!(
            middle_click_pastes(&mut core),
            "an un-chorded hoverless press owns nothing — the middle click pastes",
        );
        core.mouse_released(PointerId::MOUSE);
    }

    #[test]
    fn r882_chord_lift_mid_gesture_release_still_resolves_in_pan_channel() {
        // Gesture-capture: releasing Space mid-pan must not re-route
        // the left release into the widget arc — the gesture in
        // flight, not the chord state, owns the routing.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.cursor_moved(PointerId::MOUSE, 50.0, 50.0);
        core.note_key_state("Space", true);
        core.mouse_pressed(PointerId::MOUSE);
        core.note_key_state("Space", false);
        assert!(
            !middle_click_pastes(&mut core),
            "the pan gesture survives the chord lift",
        );
        core.mouse_released(PointerId::MOUSE);
        assert!(
            middle_click_pastes(&mut core),
            "the release resolved in the pan channel even with the chord lifted",
        );
    }

    #[test]
    fn r882_window_blur_clears_the_chord() {
        // The browser missed-keyup convention: the keyup after a focus
        // loss goes to another window; a stranded chord would turn
        // every post-refocus left drag into a pan.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.note_key_state("Space", true);
        assert!(core.space_held());
        core.window_blurred();
        assert!(!core.space_held(), "blur clears the held chord");
        // And behaviourally: a post-blur left press opens no pan.
        core.cursor_moved(PointerId::MOUSE, 50.0, 50.0);
        core.mouse_pressed(PointerId::MOUSE);
        assert!(
            middle_click_pastes(&mut core),
            "after blur the left press is un-chorded again",
        );
        core.mouse_released(PointerId::MOUSE);
    }

    #[test]
    fn r882_note_key_state_tracks_only_the_chord_vocabulary() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.note_key_state("a", true);
        core.note_key_state("Enter", true);
        assert!(!core.space_held(), "non-chord keys never arm the pan chord");
        core.note_key_state("Space", true);
        assert!(core.space_held());
        // Auto-repeat re-sends the pressed edge — idempotent.
        core.note_key_state("Space", true);
        assert!(core.space_held());
        core.note_key_state("Space", false);
        assert!(!core.space_held());
    }

    #[test]
    fn r882_1_injected_click_mid_pan_is_inert_and_native_release_resolves() {
        // Session-audit regression (the same-button release-theft
        // hole): a full injected click cycle (what the `scene/click`
        // drain produces) landing mid-chord-pan must neither end the
        // native gesture nor dispatch anything; only the native
        // release frees the pointer. Observability: the middle-paste
        // discriminator from the tests above.
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        core.cursor_moved(PointerId::MOUSE, 50.0, 50.0);
        core.note_key_state("Space", true);
        core.mouse_pressed(PointerId::MOUSE);
        // Injected click while the chord pan owns the pointer (chord
        // still held — the drain's press takes the chord arc).
        core.mouse_pressed(PointerId::MOUSE);
        core.mouse_released(PointerId::MOUSE);
        assert!(
            !middle_click_pastes(&mut core),
            "the injected pair must not consume the native pan gesture",
        );
        // And the chordless flavour (chord lifted mid-pan).
        core.note_key_state("Space", false);
        core.mouse_pressed(PointerId::MOUSE);
        core.mouse_released(PointerId::MOUSE);
        assert!(
            !middle_click_pastes(&mut core),
            "the chordless injected pair must not consume it either",
        );
        // The native release (the press that opened the gesture)
        // resolves it and frees the pointer.
        core.mouse_released(PointerId::MOUSE);
        assert!(
            middle_click_pastes(&mut core),
            "the owning native release resolved the pan",
        );
    }

    #[test]
    fn r882_rpc_scene_key_edges_drive_the_chord() {
        // The wire peer: `scene/key state:"down"` arms the chord
        // exactly as a physical Space press would; `state:"up"`
        // releases it; the legacy edgeless form never touches it (an
        // atomic press cannot strand the chord).
        let _g = TEST_LOCK.lock().unwrap();
        reset_mocks();

        let mut core = ShellCore::<TestView>::new();
        let mut no_resize = |_: u32, _: u32| {};
        let down = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":5.0,"y":5.0},"key":"Space","state":"down"},"id":1}"#;
        let _ = core.dispatch_rpc(down, &mut no_resize);
        assert!(core.space_held(), "scene/key state:down arms the chord");
        let up = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":5.0,"y":5.0},"key":"Space","state":"up"},"id":2}"#;
        let _ = core.dispatch_rpc(up, &mut no_resize);
        assert!(!core.space_held(), "scene/key state:up releases the chord");
        let legacy = r#"{"jsonrpc":"2.0","method":"scene/key","params":{"at":{"x":5.0,"y":5.0},"key":"Space"},"id":3}"#;
        let _ = core.dispatch_rpc(legacy, &mut no_resize);
        assert!(
            !core.space_held(),
            "the legacy atomic press never touches the held-key cache",
        );
    }
}

#[cfg(test)]
mod r56_2_c_ime_caret_rect_default {
    //! R56.2.c §5.13 §5.38 — `WidgetView::ime_caret_rect` default-impl
    //! contract regression.
    //!
    //! `TestView` does not override `ime_caret_rect`; the trait
    //! default returns `None`, which the shell's `render()` path
    //! interprets as "no IME-relevant caret right now, skip
    //! `Window::set_ime_cursor_area`". The shell-wire arc itself
    //! (cursor-area dedup, `winit::Window::set_ime_cursor_area`
    //! call) requires a live `winit::EventLoop` + `Window` and is
    //! exercised on real platform IME input rather than under
    //! `cargo test` — the trait default and the application
    //! override path are pinned here so a future trait shape change
    //! lands a compile-time + test-time regression.

    use super::TestView;
    use pinion_core::scene::{ContainerNode, Scene};
    use pinion_shell::WidgetView;

    #[test]
    fn r56_2_c_default_ime_caret_rect_returns_none_for_any_state() {
        // TestView::State == i32 (carrying no caret semantics).
        // Default `WidgetView::ime_caret_rect` defers to the trait
        // signature's `None`; no platform IME positioning happens.
        let scene = Scene::Container(ContainerNode::new(vec![]).with_tag("test"));
        for state in [0_i32, 1_i32, -7_i32] {
            assert!(
                TestView::ime_caret_rect(&state, &scene, Some("test")).is_none(),
                "default ime_caret_rect must yield None on TestView state {state}",
            );
            assert!(
                TestView::ime_caret_rect(&state, &scene, None).is_none(),
                "default ime_caret_rect None whether or not anything is focused",
            );
            assert!(
                TestView::ime_caret_rect(&state, &scene, Some("foreign_tag")).is_none(),
                "default ime_caret_rect None on a wrong-focus dispatch",
            );
        }
    }

}

mod r680_per_window_redraw_wakeup {
    //! R680 atomic 2 §5.16 §5.41 — per-window redraw wake-up API.
    //!
    //! Tests pin the load-bearing invariants:
    //!
    //! - `request_redraw_for_window(id)` sets ONLY that slot's flag;
    //!   sibling slots stay false.
    //! - `take_redraw_request_for_window(id)` drains the addressed
    //!   flag (returns true once, then false).
    //! - `redraw_requested_for_window(id)` is a peek-only probe;
    //!   does not drain.
    //! - `request_redraw()` and the binding-wide
    //!   `take_redraw_request()` keep their pre-R680 semantics
    //!   (fan out to every window); per-window flags coexist
    //!   independently.
    //! - Unknown `window_id` never-touched returns false on probe +
    //!   drain without allocating.

    use super::TestView;
    use super::TEST_LOCK;
    use pinion_shell::ShellCore;

    #[test]
    fn r680_request_redraw_for_window_targets_only_addressed_slot() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.request_redraw_for_window("inspector");
        assert!(core.redraw_requested_for_window("inspector"));
        assert!(
            !core.redraw_requested_for_window("main"),
            "sibling slot must stay false on a targeted wake-up",
        );
        assert!(
            !core.redraw_requested_for_window("palette"),
            "never-touched slot reads false without allocating",
        );
    }

    #[test]
    fn r680_take_redraw_request_for_window_drains_once() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.request_redraw_for_window("inspector");
        assert!(core.take_redraw_request_for_window("inspector"));
        assert!(
            !core.take_redraw_request_for_window("inspector"),
            "drain yields false on the second call (flag reset)",
        );
        assert!(!core.redraw_requested_for_window("inspector"));
    }

    #[test]
    fn r680_take_redraw_request_for_unknown_window_returns_false() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        assert!(!core.take_redraw_request_for_window("never-touched"));
        assert!(!core.redraw_requested_for_window("never-touched"));
    }

    #[test]
    fn r680_request_redraw_for_window_idempotent_between_drains() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        // Three calls in one event-loop iteration → one drain yields
        // true; the next drain yields false.
        core.request_redraw_for_window("inspector");
        core.request_redraw_for_window("inspector");
        core.request_redraw_for_window("inspector");
        assert!(core.take_redraw_request_for_window("inspector"));
        assert!(!core.take_redraw_request_for_window("inspector"));
    }

    #[test]
    fn r680_binding_wide_and_per_window_coexist_independently() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        // Binding-wide flag fans out (the pre-R680 contract).
        core.request_redraw();
        // Per-window flag targets one slot.
        core.request_redraw_for_window("inspector");

        assert!(core.redraw_requested(), "binding-wide flag observable");
        assert!(core.redraw_requested_for_window("inspector"));
        assert!(!core.redraw_requested_for_window("palette"));

        // Drains are independent.
        assert!(core.take_redraw_request());
        assert!(
            !core.redraw_requested(),
            "binding-wide drain resets the binding-wide flag only",
        );
        assert!(
            core.redraw_requested_for_window("inspector"),
            "per-window flag survives an unrelated binding-wide drain",
        );
        assert!(core.take_redraw_request_for_window("inspector"));
    }

    #[test]
    fn r680_two_distinct_window_ids_drain_independently() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.request_redraw_for_window("inspector");
        core.request_redraw_for_window("palette");
        // Each drain consumes only its own slot.
        assert!(core.take_redraw_request_for_window("inspector"));
        assert!(
            core.redraw_requested_for_window("palette"),
            "inspector drain must leave palette untouched",
        );
        assert!(core.take_redraw_request_for_window("palette"));
        assert!(!core.redraw_requested_for_window("inspector"));
        assert!(!core.redraw_requested_for_window("palette"));
    }
}

#[cfg(test)]
mod r681_immediate_mode_paint_cycle {
    //! R681 §2 #4 atomic 1 — per-window immediate-mode tick + paint
    //! dispatch through [`ShellCore::compute_paint_scene_for_window`].
    //!
    //! Tests pin the load-bearing wiring invariants:
    //!
    //! - The substrate walks the paint scene returned by the view fn
    //!   for [`Scene::ImmediateModeNode`]s and invokes
    //!   `handle.borrow_mut().tick(dt)` exactly once per node per
    //!   per-window paint cycle, with `dt` matched to the per-window
    //!   delta the substrate already used for the animation tick.
    //! - The same `dt` lands in
    //!   [`pinion_core::scene::ImmediateModeNode::last_dt`] for AI
    //!   introspection.
    //! - A scene with at least one immediate-mode node arms BOTH the
    //!   binding-wide `redraw_requested` flag AND the per-window
    //!   `redraw_requested_for_window(window_id)` flag, so the next
    //!   frame fires from the per-window paint clock without input.
    //! - A scene with zero immediate-mode nodes does not arm those
    //!   redraw flags via the immediate-mode wire (other paths may
    //!   still set them — animation tick / scroll-dirty / explicit
    //!   `request_redraw`).
    //! - Each per-window paint cycle ticks the drivers for that
    //!   window independently; calling
    //!   `compute_paint_scene_for_window("inspector", …)` does not
    //!   tick a driver that only appears in the primary window's
    //!   view (and vice-versa).
    //!
    //! Pixel correctness of `paint_immediate_mode_node` (the Vello-side
    //! encode) is the paint adapter's contract, exercised by its own
    //! integration tests; this module covers the dispatch wiring.
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use pinion_core::scene::{
        ContainerNode, ImmediateMode, ImmediateModeNode, Rect, Scene, StubImmediateMode,
    };
    use pinion_core::{Frame, WidgetCore};
    use pinion_shell::{ShellCore, WidgetView};

    use super::{TestExternal, TestRenderer};

    // R681 atomic 1 — the shared driver is stored in a `thread_local!`
    // because `Rc<RefCell<...>>` is not `Sync` (so it cannot live in
    // a `static OnceLock`). The shell-side dispatch runs on the test
    // thread inside `TEST_LOCK`, so the thread-local matches the
    // dispatch site without crossing thread boundaries. Each test
    // resets the slot to `None` on entry + exit.
    thread_local! {
        static SHARED_DRIVER: RefCell<Option<Rc<RefCell<StubImmediateMode>>>> =
            const { RefCell::new(None) };
    }

    fn install_driver() -> Rc<RefCell<StubImmediateMode>> {
        let driver = Rc::new(RefCell::new(StubImmediateMode::new()));
        SHARED_DRIVER.with(|slot| *slot.borrow_mut() = Some(driver.clone()));
        driver
    }

    fn clear_driver() {
        SHARED_DRIVER.with(|slot| *slot.borrow_mut() = None);
    }

    fn driver_clone() -> Option<Rc<RefCell<StubImmediateMode>>> {
        SHARED_DRIVER.with(|slot| slot.borrow().clone())
    }

    /// Static toggle: when `true`, `R681View::view` emits a scene
    /// containing one [`Scene::ImmediateModeNode`] wrapping the
    /// shared driver; when `false`, returns a baseline Container.
    /// Reset between tests via `set_emit_immediate(false)`.
    static EMIT_IMMEDIATE: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    fn set_emit_immediate(emit: bool) {
        EMIT_IMMEDIATE.store(emit, std::sync::atomic::Ordering::SeqCst);
    }

    struct R681View;

    impl pinion_a11y::WidgetA11y for R681View {}

    impl WidgetCore for R681View {
        type State = i32;
        type Event = ();
        fn create_external() -> Box<dyn pinion_core::External> {
            Box::new(TestExternal::default())
        }
        fn tag() -> &'static str {
            "r681_test"
        }
        fn read_state(_scene: &Scene) -> Self::State {
            0
        }
        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            if EMIT_IMMEDIATE.load(std::sync::atomic::Ordering::SeqCst) {
                let handle: Rc<RefCell<dyn ImmediateMode>> =
                    driver_clone().expect("install_driver before view");
                Scene::Container(
                    ContainerNode::new(vec![Scene::ImmediateModeNode(
                        ImmediateModeNode::new(handle, Rect::new(0, 0, 100, 100))
                            .with_tag("canvas"),
                    )])
                    .with_tag("r681_test"),
                )
            } else {
                Scene::Container(ContainerNode::new(vec![]).with_tag("r681_test"))
            }
        }
        fn event_name(_event: Self::Event) -> &'static str {
            ""
        }
        fn title() -> &'static str {
            "R681View"
        }
    }

    impl WidgetView for R681View {
        type Renderer = TestRenderer;

        fn initial_size_strategy() -> pinion_shell::SizeStrategy {
            pinion_shell::SizeStrategy::Fixed { width: 200, height: 200 }
        }
    }

    fn reset_state() {
        set_emit_immediate(false);
        clear_driver();
    }

    #[test]
    fn r681_view_with_immediate_node_ticks_driver_and_arms_per_window_redraw() {
        let _guard = super::TEST_LOCK.lock().unwrap();
        reset_state();
        let driver = install_driver();
        set_emit_immediate(true);
        let mut core: ShellCore<R681View> = ShellCore::new();
        // First paint — the per-window clock has no prior instant, so
        // dt=0.0. R831: the fixed-timestep accumulator releases zero
        // whole steps for a zero delta, so the driver is NOT ticked yet
        // (the pre-R831 variable-step model ticked once per paint).
        let scene = core.compute_paint_scene_for_window("main", 200, 200);
        // The paint scene contains the ImmediateModeNode the view
        // fn emitted.
        assert!(scene.has_immediate_mode_subtree());
        assert_eq!(
            driver.borrow().tick_count, 0,
            "dt=0 first paint releases no whole fixed step (R831 accumulator)",
        );
        assert_eq!(
            driver.borrow().last_observed_dt,
            Duration::ZERO,
            "driver untouched → last_observed_dt is still the ZERO sentinel",
        );
        // The redraw flags are armed by PRESENCE of the immediate node
        // (`has_immediate_mode_subtree`), NOT by whether a step ticked —
        // the game loop must keep painting so the accumulator fills.
        assert!(
            core.redraw_requested(),
            "immediate-mode arms binding-wide redraw_requested",
        );
        assert!(
            core.redraw_requested_for_window("main"),
            "immediate-mode arms per-window redraw flag for the painted window",
        );
        reset_state();
    }

    #[test]
    fn r831_injected_time_drives_exact_fixed_steps() {
        // R831 §2 #4 §5.28 — drive the immediate-mode game loop
        // deterministically by INJECTING elapsed time via `scene/tick`
        // (the §2 #2 RPC peer of a wall-clock advance), then painting to
        // consume it through the fixed-timestep accumulator. 0.03 s is
        // 3.6 fixed steps (1/120 s each), so the accumulator releases
        // exactly 3 whole steps (floor), carrying the 0.6-step remainder
        // — a deterministic count, unlike wall-clock timing.
        let _guard = super::TEST_LOCK.lock().unwrap();
        reset_state();
        let driver = install_driver();
        set_emit_immediate(true);
        let mut core: ShellCore<R681View> = ShellCore::new();
        // Prime the per-window paint clock (first paint, dt=0, 0 steps).
        let _ = core.compute_paint_scene_for_window("main", 200, 200);
        assert_eq!(driver.borrow().tick_count, 0, "dt=0 prime paint: no step");
        // Inject 0.03 s of simulation time at the addressed (default)
        // window, then paint to consume it.
        let mut no_resize = |_w: u32, _h: u32| {};
        let _ = core.dispatch_rpc(
            r#"{"jsonrpc":"2.0","method":"scene/tick","params":{"dt":0.03},"id":1}"#,
            &mut no_resize,
        );
        let _ = core.compute_paint_scene_for_window("main", 200, 200);
        assert_eq!(
            driver.borrow().tick_count, 3,
            "0.03 s / (1/120 s) = 3.6 → exactly 3 whole fixed steps",
        );
        // Each step advanced by the FIXED timestep, not the injected
        // total — last_observed_dt is one fixed step (~8.33 ms).
        let last = driver.borrow().last_observed_dt;
        assert!(
            (Duration::from_micros(8000)..=Duration::from_micros(8700)).contains(&last),
            "each tick is the fixed 1/120 s step; saw {last:?}",
        );
        reset_state();
    }

    #[test]
    fn r681_view_without_immediate_node_does_not_arm_per_window_redraw() {
        let _guard = super::TEST_LOCK.lock().unwrap();
        reset_state();
        // No driver installed; `EMIT_IMMEDIATE` stays false → view
        // returns a baseline Container with no ImmediateModeNode.
        let mut core: ShellCore<R681View> = ShellCore::new();
        let scene = core.compute_paint_scene_for_window("main", 200, 200);
        assert!(!scene.has_immediate_mode_subtree());
        // The R681 immediate-mode wire did NOT arm the per-window
        // flag. Other paths (e.g. an active animation, scroll-dirty,
        // explicit request_redraw) may still arm it — we assert the
        // immediate-mode wire stays inert by checking neither flag
        // is set on this fresh ShellCore that has no other reason
        // to request a redraw.
        assert!(
            !core.redraw_requested_for_window("main"),
            "no immediate node → no per-window flag from R681 wire",
        );
        reset_state();
    }

    #[test]
    fn r681_per_window_immediate_targets_only_painted_window_slot() {
        let _guard = super::TEST_LOCK.lock().unwrap();
        reset_state();
        let _driver = install_driver();
        set_emit_immediate(true);
        let mut core: ShellCore<R681View> = ShellCore::new();
        // Paint only the inspector window — the per-window flag
        // should target "inspector", not "main".
        let _ = core.compute_paint_scene_for_window("inspector", 200, 200);
        assert!(core.redraw_requested_for_window("inspector"));
        assert!(
            !core.redraw_requested_for_window("main"),
            "paint of inspector window must not arm main's per-window flag",
        );
        reset_state();
    }

    #[test]
    fn r681_last_paint_instant_for_window_populated_after_paint() {
        // R681 atomic 2 — `about_to_wait` reads this to compute the
        // per-window next-paint deadline for `ControlFlow::WaitUntil`.
        let _guard = super::TEST_LOCK.lock().unwrap();
        reset_state();
        let mut core: ShellCore<R681View> = ShellCore::new();
        assert!(
            core.last_paint_instant_for_window("main").is_none(),
            "pre-paint: no Instant recorded",
        );
        let _ = core.compute_paint_scene_for_window("main", 200, 200);
        let first = core.last_paint_instant_for_window("main");
        assert!(first.is_some(), "post-paint: Instant recorded for window");
        // Second paint moves the Instant forward.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = core.compute_paint_scene_for_window("main", 200, 200);
        let second = core
            .last_paint_instant_for_window("main")
            .expect("second paint recorded");
        assert!(second >= first.unwrap(), "Instant monotonically forward");
        // Unrelated window still None.
        assert!(
            core.last_paint_instant_for_window("inspector").is_none(),
            "other windows untouched",
        );
        reset_state();
    }

    #[test]
    fn r831_last_dt_sidecar_publishes_the_fixed_step() {
        // R831 §2 #4 §5.28 — with the fixed-timestep accumulator,
        // `last_dt` publishes the FIXED simulation step (1/120 s ≈
        // 8.33 ms), NOT the variable wall-clock frame delta. It stays at
        // the `Duration::ZERO` sentinel until the first WHOLE fixed step
        // fires.
        let _guard = super::TEST_LOCK.lock().unwrap();
        reset_state();
        let _driver = install_driver();
        set_emit_immediate(true);
        let mut core: ShellCore<R681View> = ShellCore::new();
        // First paint sees dt=0 → zero whole steps → ZERO sentinel.
        let scene_1 = core.compute_paint_scene_for_window("main", 200, 200);
        if let Some(node) = walk_first_immediate(&scene_1) {
            assert_eq!(node.last_dt(), Duration::ZERO);
        }
        // Sleep past one fixed step (≈8.33 ms) so the accumulated time
        // crosses a whole-step boundary and the driver ticks once.
        std::thread::sleep(std::time::Duration::from_millis(12));
        let scene_2 = core.compute_paint_scene_for_window("main", 200, 200);
        if let Some(node) = walk_first_immediate(&scene_2) {
            let last = node.last_dt();
            assert!(last > Duration::ZERO, "a whole fixed step fired");
            // It is the FIXED step, not the ≥12 ms wall-clock frame — the
            // defining property of the fixed-timestep loop.
            assert!(
                (Duration::from_micros(8000)..=Duration::from_micros(8700))
                    .contains(&last),
                "last_dt is the fixed 1/120 s step (~8333 us), not the \
                 wall-clock frame delta; saw {last:?}",
            );
        }
        reset_state();
    }

    fn walk_first_immediate(scene: &Scene) -> Option<&ImmediateModeNode> {
        match scene {
            Scene::ImmediateModeNode(n) => Some(n),
            Scene::Container(c) => c.children.iter().find_map(walk_first_immediate),
            Scene::Scroll(s) => walk_first_immediate(&s.content),
            _ => None,
        }
    }

    #[test]
    fn r681_set_target_fps_for_window_round_trips_through_substrate() {
        let _guard = super::TEST_LOCK.lock().unwrap();
        reset_state();
        let mut core: ShellCore<R681View> = ShellCore::new();
        // Default (no override): None means "use derived default
        // policy — 60fps when immediate, idle otherwise".
        assert!(core.target_fps_for_window("main").is_none());
        // Setter persists through reader.
        core.set_target_fps_for_window("main", 120);
        assert_eq!(core.target_fps_for_window("main"), Some(120));
        // Setting another window does not perturb the first.
        core.set_target_fps_for_window("inspector", 30);
        assert_eq!(core.target_fps_for_window("inspector"), Some(30));
        assert_eq!(core.target_fps_for_window("main"), Some(120));
        // Re-set overwrites (latest wins).
        core.set_target_fps_for_window("main", 144);
        assert_eq!(core.target_fps_for_window("main"), Some(144));
        // Sentinel: fps = 0 round-trips (caller's "paused polled" signal).
        core.set_target_fps_for_window("main", 0);
        assert_eq!(core.target_fps_for_window("main"), Some(0));
        reset_state();
    }
}

// ─────────────────────────────────────────────────────────────
// R682 §5.16 atomic 3 — FragmentCacheStats publish/getter substrate
// ─────────────────────────────────────────────────────────────

mod r682_fragment_cache_stats_substrate {
    use pinion_core::scene::Rect;
    use pinion_shell::{FragmentCacheStats, ShellCore};

    use super::TestView;

    #[test]
    fn r682_stats_for_unknown_window_returns_none() {
        let _g = super::TEST_LOCK.lock().unwrap();
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
        let core: ShellCore<TestView> = ShellCore::new();
        assert!(core.fragment_cache_stats_for_window("main").is_none());
        assert!(core.fragment_cache_stats_for_window("inspector").is_none());
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
    }

    #[test]
    fn r682_publish_round_trips_typed_snapshot() {
        let _g = super::TEST_LOCK.lock().unwrap();
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
        let mut core: ShellCore<TestView> = ShellCore::new();
        let stats = FragmentCacheStats {
            hits: 42,
            misses: 7,
            paint_count: 49,
            entries: 12,
            last_damage_region: Some(Rect::new(10, 20, 100, 50)),
        };
        core.publish_fragment_cache_stats("main", stats);
        let got = core
            .fragment_cache_stats_for_window("main")
            .expect("published");
        assert_eq!(got, stats);
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
    }

    #[test]
    fn r682_publish_per_window_independent() {
        let _g = super::TEST_LOCK.lock().unwrap();
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
        let mut core: ShellCore<TestView> = ShellCore::new();
        let main_stats = FragmentCacheStats {
            hits: 5,
            misses: 1,
            paint_count: 6,
            entries: 2,
            last_damage_region: None,
        };
        let inspector_stats = FragmentCacheStats {
            hits: 0,
            misses: 3,
            paint_count: 3,
            entries: 3,
            last_damage_region: Some(Rect::new(0, 0, 480, 320)),
        };
        core.publish_fragment_cache_stats("main", main_stats);
        core.publish_fragment_cache_stats("inspector", inspector_stats);
        assert_eq!(
            core.fragment_cache_stats_for_window("main"),
            Some(main_stats)
        );
        assert_eq!(
            core.fragment_cache_stats_for_window("inspector"),
            Some(inspector_stats)
        );
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
    }

    #[test]
    fn r682_publish_overwrites_latest_wins() {
        let _g = super::TEST_LOCK.lock().unwrap();
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.publish_fragment_cache_stats(
            "main",
            FragmentCacheStats { hits: 1, ..Default::default() },
        );
        core.publish_fragment_cache_stats(
            "main",
            FragmentCacheStats { hits: 99, ..Default::default() },
        );
        let got = core.fragment_cache_stats_for_window("main").unwrap();
        assert_eq!(got.hits, 99);
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
    }

    #[test]
    fn r682_stat_windows_iterates_published_keys() {
        let _g = super::TEST_LOCK.lock().unwrap();
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
        let mut core: ShellCore<TestView> = ShellCore::new();
        let stats = FragmentCacheStats::default();
        core.publish_fragment_cache_stats("main", stats);
        core.publish_fragment_cache_stats("inspector", stats);
        let mut keys: Vec<_> = core.fragment_cache_stat_windows().collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["inspector", "main"]);
        // (No reset_state — R682 stats tests do not touch the
        // R681 immediate-mode SHARED_DRIVER / EMIT_IMMEDIATE globals;
        // they only exercise typed substrate accessors.)
    }

    #[test]
    fn r682_stats_hit_rate_zero_when_no_lookups() {
        let stats = FragmentCacheStats::default();
        assert!((stats.hit_rate() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r682_stats_hit_rate_matches_hits_over_total() {
        let stats = FragmentCacheStats {
            hits: 7,
            misses: 3,
            paint_count: 10,
            entries: 5,
            last_damage_region: None,
        };
        assert!((stats.hit_rate() - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn r682_stats_default_is_empty() {
        let stats = FragmentCacheStats::default();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.paint_count, 0);
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.last_damage_region, None);
    }
}

// ─────────────────────────────────────────────────────────────
// R907 §5.16 §5.7 — frame-timing profiler substrate. Mirrors the
// R682 cache-stats topology: the surface records per-frame phase
// samples, the substrate projects a rolling-window snapshot at the
// AI-paced `scene/frame_timings` read.
// ─────────────────────────────────────────────────────────────
mod r907_frame_timing_substrate {
    use pinion_runtime::FrameTiming;
    use pinion_shell::ShellCore;

    use super::TestView;

    #[test]
    fn r907_timings_for_unknown_window_returns_none() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let core: ShellCore<TestView> = ShellCore::new();
        // No paint recorded → bootstrap state, projected as None
        // (mapped to FrameTimingsUnavailable at the RPC layer).
        assert!(core.frame_timings_for_window("main").is_none());
        assert!(core.frame_timings_for_window("inspector").is_none());
    }

    #[test]
    fn r907_record_round_trips_projected_snapshot() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.record_frame_timing("main", FrameTiming::new(300, 100, 80, 540));
        let snap = core
            .frame_timings_for_window("main")
            .expect("recorded → projectable");
        assert_eq!(snap.frame_count, 1);
        assert_eq!(snap.window_len, 1);
        assert_eq!(snap.last, FrameTiming::new(300, 100, 80, 540));
        assert_eq!(snap.mean_total_us, 540);
        // total >= build + encode + render holds on the projected last.
        assert!(snap.last.total_us >= snap.last.phase_sum_us());
    }

    #[test]
    fn r907_record_per_window_independent() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.record_frame_timing("main", FrameTiming::new(100, 50, 30, 200));
        core.record_frame_timing("inspector", FrameTiming::new(900, 80, 60, 1100));
        let main = core.frame_timings_for_window("main").unwrap();
        let inspector = core.frame_timings_for_window("inspector").unwrap();
        assert_eq!(main.last.total_us, 200);
        assert_eq!(inspector.last.total_us, 1100);
        // Counters are per-window, not shared.
        assert_eq!(main.frame_count, 1);
        assert_eq!(inspector.frame_count, 1);
    }

    #[test]
    fn r907_cumulative_count_survives_window_aggregate() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        // Three frames of growing cost: count = 3, window folds all 3.
        core.record_frame_timing("main", FrameTiming::new(100, 50, 30, 200));
        core.record_frame_timing("main", FrameTiming::new(200, 60, 40, 400));
        core.record_frame_timing("main", FrameTiming::new(300, 70, 50, 600));
        let snap = core.frame_timings_for_window("main").unwrap();
        assert_eq!(snap.frame_count, 3);
        assert_eq!(snap.window_len, 3);
        assert_eq!(snap.min_total_us, 200);
        assert_eq!(snap.max_total_us, 600);
        assert_eq!(snap.mean_total_us, (200 + 400 + 600) / 3);
        // The freshest frame is `last`, not the max.
        assert_eq!(snap.last.total_us, 600);
    }

    #[test]
    fn r907_remove_window_drops_timing_entry() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.register_window("inspector");
        core.record_frame_timing("inspector", FrameTiming::new(100, 50, 30, 200));
        assert!(core.frame_timings_for_window("inspector").is_some());
        // Cleanup removes the per-window accumulator (the remove_window
        // OR-of-maps reports at least one map carried an entry).
        assert!(core.remove_window("inspector"));
        assert!(core.frame_timings_for_window("inspector").is_none());
    }

    // ── R925 §5.16 §5.7 — embedder derives the jank budget from the
    // window's target_fps and feeds it into the projection. ───────────

    #[test]
    fn r925_no_target_fps_yields_no_budget() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.record_frame_timing("main", FrameTiming::new(300, 100, 80, 540));
        let snap = core.frame_timings_for_window("main").unwrap();
        // No declared frame target → unpaced window → no jank concept.
        assert_eq!(snap.budget_us, None);
        assert_eq!(snap.over_budget_frames, 0);
        assert!(snap.jank_ratio.abs() < f32::EPSILON);
    }

    #[test]
    fn r925_target_fps_budget_classifies_recorded_frames() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        // 60fps budget = ⌊1e6 / 60⌋ = 16_666µs. One frame under it, one
        // well over it: the over-budget frame is the dropped frame.
        core.set_target_fps_for_window("main", 60);
        core.record_frame_timing("main", FrameTiming::new(8_000, 4_000, 2_000, 14_000));
        core.record_frame_timing("main", FrameTiming::new(12_000, 5_000, 3_000, 20_000));
        let snap = core.frame_timings_for_window("main").unwrap();
        assert_eq!(snap.budget_us, Some(16_666), "⌊1e6/60⌋ µs budget");
        assert_eq!(snap.over_budget_frames, 1, "only the 20_000µs frame is over");
        assert_eq!(snap.worst_overrun_us, 20_000 - 16_666);
        assert!((snap.jank_ratio - 0.5).abs() < 1e-6, "1 of 2 frames janked");
    }

    #[test]
    fn r925_budget_is_the_pacing_budget_higher_fps_is_tighter() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.record_frame_timing("main", FrameTiming::new(5_000, 3_000, 1_000, 12_000));
        // The reported budget IS 1/fps — the very deadline the render
        // loop paces to. A higher target gives a tighter budget, so a
        // frame that met the 60fps budget can miss the 120fps one.
        core.set_target_fps_for_window("main", 60);
        let at_60 = core.frame_timings_for_window("main").unwrap();
        assert_eq!(at_60.budget_us, Some(16_666));
        assert_eq!(at_60.over_budget_frames, 0, "12_000µs is within 16_666µs");
        core.set_target_fps_for_window("main", 120);
        let at_120 = core.frame_timings_for_window("main").unwrap();
        assert_eq!(at_120.budget_us, Some(8_333), "⌊1e6/120⌋");
        assert_eq!(at_120.over_budget_frames, 1, "12_000µs misses the 8_333µs budget");
    }

    #[test]
    fn r925_paused_window_fps_zero_has_no_budget() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.record_frame_timing("main", FrameTiming::new(300, 100, 80, 9_999));
        // fps = 0 is the paused sentinel: no deadline, hence no budget —
        // jank is undefined while the window is frame-stepped.
        core.set_target_fps_for_window("main", 0);
        let snap = core.frame_timings_for_window("main").unwrap();
        assert_eq!(snap.budget_us, None);
        assert_eq!(snap.over_budget_frames, 0);
    }

    // ── R926.1 — the jank budget equals the render-loop pacing budget
    // for an IMMEDIATE-MODE window too (R925 regression: the budget was
    // hardcoded has_immediate=false, so immediate windows that pace at
    // the default 60fps reported budget_us=null / zero jank forever). ──

    #[test]
    fn r926_1_immediate_mode_window_gets_default_60fps_jank_budget() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.record_frame_timing("game", FrameTiming::new(8_000, 4_000, 2_000, 20_000));
        // No target_fps override, but the painted scene carried an
        // immediate-mode subtree -> the render loop paces it at the
        // default 60fps, and the jank profiler must judge against that
        // same 16_666µs budget (not report it as unpaced).
        core.set_immediate_subtree_for_window("game", true);
        let snap = core.frame_timings_for_window("game").unwrap();
        assert_eq!(
            snap.budget_us,
            Some(16_666),
            "an immediate-mode window paces at the default 60fps",
        );
        assert_eq!(
            snap.over_budget_frames, 1,
            "the 20_000µs frame missed the 16_666µs budget",
        );
        assert_eq!(snap.worst_overrun_us, 20_000 - 16_666);
    }

    #[test]
    fn r926_1_immediate_flag_defaults_false_and_override_wins() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        // Default (no flag published): retained-tree -> no budget.
        core.record_frame_timing("w", FrameTiming::new(100, 50, 30, 9_999));
        assert!(!core.immediate_subtree_for_window("w"));
        assert_eq!(core.frame_timings_for_window("w").unwrap().budget_us, None);
        // An explicit target_fps override wins over the immediate-mode
        // 60fps default (frame_budget_for_window: override takes
        // precedence) — exactly as the pacing loop resolves it.
        core.set_immediate_subtree_for_window("w", true);
        core.set_target_fps_for_window("w", 30);
        assert_eq!(
            core.frame_timings_for_window("w").unwrap().budget_us,
            Some(33_333),
            "30fps override wins over the 60fps immediate default",
        );
    }
}

// ─────────────────────────────────────────────────────────────
// R885 §5.49 — scene/input_state RPC dispatch wire (READ peer of the
// out-of-band input writes; each read mirrors its write shape)
// ─────────────────────────────────────────────────────────────

mod r888_pacing_state_rpc {
    use pinion_shell::ShellCore;

    use super::TestView;

    fn fps_of(core: &mut ShellCore<TestView>) -> serde_json::Value {
        let mut no_resize = |_: u32, _: u32| {};
        let read = r#"{"jsonrpc":"2.0","method":"scene/pacing_state","id":1}"#;
        let resp = core.dispatch_rpc(read, &mut no_resize).expect("response");
        let body: serde_json::Value = serde_json::from_str(&resp).expect("JSON");
        body.get("result").expect("dispatch ok").get("fps").expect("fps field").clone()
    }

    fn set_fps(core: &mut ShellCore<TestView>, fps: &str) {
        let mut no_resize = |_: u32, _: u32| {};
        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"scene/set_fps","params":{{"fps":{fps}}},"id":2}}"#
        );
        let resp = core.dispatch_rpc(&req, &mut no_resize).expect("response");
        assert!(resp.contains(r#""result":null"#), "set_fps {fps} ok: {resp}");
    }

    #[test]
    fn r888_pacing_state_round_trips_every_set_fps_write() {
        // R888 — read = inverse of write across the whole axis: boot
        // (default policy) -> null; override N -> N; paused 0 -> 0;
        // clear (null write) -> null again. Pre-R888 the axis was
        // write-only AND the boot state was unreachable once any set
        // landed (insert-only map).
        let _g = super::TEST_LOCK.lock().unwrap();
        super::reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();

        assert_eq!(fps_of(&mut core), serde_json::Value::Null, "boot: default policy");
        set_fps(&mut core, "30");
        assert_eq!(fps_of(&mut core), serde_json::json!(30), "override installs");
        set_fps(&mut core, "0");
        assert_eq!(fps_of(&mut core), serde_json::json!(0), "paused (frame-step) reads 0");
        set_fps(&mut core, "null");
        assert_eq!(
            fps_of(&mut core),
            serde_json::Value::Null,
            "null write clears the override (boot state wire-reachable again)",
        );
    }
}

// ─────────────────────────────────────────────────────────────
// R889 §5.49 §5.16 — window-known predicate SSOT: registered-but-
// unpainted windows answer per-window READ axes honestly; unknown
// window scopes are rejected wholesale (READ + WRITE share the gate).
// ─────────────────────────────────────────────────────────────

mod r889_window_known_gate {
    use pinion_rpc::parse_request;
    use pinion_shell::ShellCore;

    use super::TestView;

    fn no_resize(_w: u32, _h: u32) {}

    fn frame(id: u64, method: &str, window: &str, extra: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"window":"{window}"{extra}}}}}"#,
        )
    }

    /// Dispatch one window-scoped frame the way `AppShell::dispatch_rpc`
    /// does — the scope derives from the frame's own in-band
    /// `{window}` param (R890.1: one source, no out-of-band argument).
    fn dispatch_for_window(
        core: &mut ShellCore<TestView>,
        id: u64,
        method: &str,
        window: &str,
        extra: &str,
    ) -> serde_json::Value {
        let mut nr = no_resize;
        let req = parse_request(&frame(id, method, window, extra)).expect("frame parses");
        let resp = core
            .dispatch_rpc_scoped(req, &mut nr)
            .expect("call requests always answer");
        serde_json::from_str(&resp).expect("response is JSON")
    }

    fn assert_unknown_window(body: &serde_json::Value, supplied: &str) {
        let err = body.get("error").expect("error frame");
        assert_eq!(err.get("code"), Some(&serde_json::json!(-32602)));
        assert_eq!(err.get("message"), Some(&serde_json::json!("unknown_window")));
        assert_eq!(err.get("data"), Some(&serde_json::json!(supplied)));
    }

    #[test]
    fn r889_known_unpainted_window_pacing_axis_round_trips() {
        // THE R889 headline: a registered-but-never-painted window
        // (R683 tear-off pre-first-paint) honors `scene/set_fps` AND
        // reads the same state back through `scene/pacing_state` —
        // pre-R889 the write was honored while the read answered
        // `PacingStateUnavailable` (availability piggybacked on the
        // router registry = "has painted", a category error).
        let _g = super::TEST_LOCK.lock().unwrap();
        super::reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.register_window("tear");
        assert!(
            !core.has_last_paint_scene_for_window("tear"),
            "pre-condition: registered window has never painted",
        );

        let body = dispatch_for_window(&mut core, 1, "scene/pacing_state", "tear", "");
        assert_eq!(
            body.get("result").expect("read ok").get("fps"),
            Some(&serde_json::Value::Null),
            "boot: default policy, not Unavailable",
        );
        let body = dispatch_for_window(&mut core, 2, "scene/set_fps", "tear", r#","fps":30"#);
        assert!(body.get("error").is_none(), "set_fps honored: {body}");
        let body = dispatch_for_window(&mut core, 3, "scene/pacing_state", "tear", "");
        assert_eq!(
            body.get("result").expect("read ok").get("fps"),
            Some(&serde_json::json!(30)),
            "read mirrors the write on the never-painted window",
        );
        let body = dispatch_for_window(&mut core, 4, "scene/set_fps", "tear", r#","fps":null"#);
        assert!(body.get("error").is_none(), "null clear honored: {body}");
        let body = dispatch_for_window(&mut core, 5, "scene/pacing_state", "tear", "");
        assert_eq!(
            body.get("result").expect("read ok").get("fps"),
            Some(&serde_json::Value::Null),
            "clear restores default policy",
        );
        assert!(
            !core.has_last_paint_scene_for_window("tear"),
            "the whole axis round-tripped without a single paint",
        );
    }

    #[test]
    fn r889_known_unpainted_window_input_state_available_with_null_cursor() {
        // Held keys + modifiers are binding-global facts; the cursor
        // is a router (per-paint) fact. A known-unpainted window
        // answers the axis with `cursor: null` ("no cursor event
        // yet") instead of `InputStateUnavailable`.
        let _g = super::TEST_LOCK.lock().unwrap();
        super::reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.register_window("tear");
        let body = dispatch_for_window(&mut core, 1, "scene/input_state", "tear", "");
        let result = body.get("result").expect("axis available for known window");
        assert_eq!(result.get("cursor"), Some(&serde_json::Value::Null));
        assert_eq!(result.get("held_keys"), Some(&serde_json::json!([])));
    }

    #[test]
    fn r889_unknown_window_write_rejected_and_primary_untouched() {
        // The pre-R889 production bug: `resolve_spec_id` aliased
        // unknown ids onto the primary, so `scene/set_fps {window:
        // "bogus", fps: 0}` FROZE THE PRIMARY's game loop. Post-R889
        // the write is rejected and the primary's pacing is untouched.
        let _g = super::TEST_LOCK.lock().unwrap();
        super::reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        let body = dispatch_for_window(&mut core, 1, "scene/set_fps", "bogus", r#","fps":0"#);
        assert_unknown_window(&body, "bogus");
        assert_eq!(
            core.target_fps_for_window(pinion_runtime::DEFAULT_WINDOW),
            None,
            "primary pacing untouched by the rejected bogus-window write",
        );
        let body = dispatch_for_window(&mut core, 2, "scene/pacing_state", "bogus", "");
        assert_unknown_window(&body, "bogus");
        let body = dispatch_for_window(&mut core, 3, "scene/input_state", "bogus", "");
        assert_unknown_window(&body, "bogus");
    }

    #[test]
    fn r889_single_window_entry_gates_in_band_window_param() {
        // The legacy single-window `dispatch_rpc` entry ignores the
        // window param for SCOPING (documented: multi-window callers
        // use `dispatch_rpc_scoped`), but the unknown-window gate
        // reads the IN-BAND param so a bogus scope errors here too
        // instead of silently acting on the primary.
        let _g = super::TEST_LOCK.lock().unwrap();
        super::reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        let mut nr = no_resize;
        let resp = core
            .dispatch_rpc(&frame(1, "scene/pacing_state", "bogus", ""), &mut nr)
            .expect("response");
        let body: serde_json::Value = serde_json::from_str(&resp).expect("JSON");
        assert_unknown_window(&body, "bogus");
        // A known id (the seeded primary) passes the gate unchanged.
        let resp = core
            .dispatch_rpc(
                &frame(2, "scene/pacing_state", pinion_runtime::DEFAULT_WINDOW, ""),
                &mut nr,
            )
            .expect("response");
        let body: serde_json::Value = serde_json::from_str(&resp).expect("JSON");
        assert!(body.get("result").is_some(), "known primary passes: {body}");
    }

    #[test]
    fn r889_remove_window_revokes_registration() {
        // Registry lifecycle: the reconcile drop pass's
        // `remove_window` is the removal edge — after it, the same
        // scope that round-tripped above is rejected again.
        let _g = super::TEST_LOCK.lock().unwrap();
        super::reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.register_window("tear");
        let body = dispatch_for_window(&mut core, 1, "scene/pacing_state", "tear", "");
        assert!(body.get("result").is_some(), "registered: axis answers");
        assert!(core.remove_window("tear"), "removal edge drains the entry");
        let body = dispatch_for_window(&mut core, 2, "scene/pacing_state", "tear", "");
        assert_unknown_window(&body, "tear");
    }
}

mod r885_input_state_rpc {
    use pinion_shell::ShellCore;

    use super::TestView;

    fn req(method: &str, params: &str, id: u64) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","method":"{method}","params":{params},"id":{id}}}"#,
        )
    }

    fn result(core: &mut ShellCore<TestView>, frame: &str) -> serde_json::Value {
        let mut no_resize = |_: u32, _: u32| {};
        let resp = core.dispatch_rpc(frame, &mut no_resize).expect("response");
        let body: serde_json::Value = serde_json::from_str(&resp).expect("JSON");
        body.get("result").expect("dispatch ok").clone()
    }

    #[test]
    fn r885_read_mirrors_modifier_and_held_key_writes() {
        let _g = super::TEST_LOCK.lock().unwrap();
        super::reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();

        // Boot: no modifier held, no chord key, no cursor event yet.
        let r0 = result(&mut core, &req("scene/input_state", "{}", 1));
        let mods0 = r0.get("modifiers").expect("GUI always tracks modifiers");
        assert_eq!(mods0.get("ctrl"), Some(&serde_json::Value::Bool(false)));
        assert_eq!(mods0.get("shift"), Some(&serde_json::Value::Bool(false)));
        assert_eq!(r0.get("held_keys").unwrap().as_array().unwrap().len(), 0);
        assert!(r0.get("cursor").unwrap().is_null(), "no cursor event yet");

        // scene/modifiers write → read returns the same object shape.
        let _ = result(
            &mut core,
            &req(
                "scene/modifiers",
                r#"{"shift":false,"ctrl":true,"alt":false,"meta":false}"#,
                2,
            ),
        );
        let r1 = result(&mut core, &req("scene/input_state", "{}", 3));
        assert_eq!(
            r1.get("modifiers").unwrap().get("ctrl"),
            Some(&serde_json::Value::Bool(true)),
            "read = inverse of the scene/modifiers write",
        );

        // scene/key state:"down" arms the chord cache; the read
        // enumerates the canonical named spelling. The injection also
        // lands a cursor move at the key's position.
        let _ = result(
            &mut core,
            &req(
                "scene/key",
                r#"{"key":"Space","state":"down","at":{"x":4.0,"y":5.0}}"#,
                4,
            ),
        );
        let r2 = result(&mut core, &req("scene/input_state", "{}", 5));
        assert_eq!(
            r2.get("held_keys").unwrap().as_array().unwrap(),
            &vec![serde_json::Value::String("Space".into())],
        );
        let cursor = r2.get("cursor").expect("cursor follows the key injection");
        assert_eq!(cursor.get("x").and_then(serde_json::Value::as_f64), Some(4.0));
        assert_eq!(cursor.get("y").and_then(serde_json::Value::as_f64), Some(5.0));

        // Release clears the chord; the modifier cache is untouched.
        let _ = result(
            &mut core,
            &req("scene/key", r#"{"key":"Space","state":"up"}"#, 6),
        );
        let r3 = result(&mut core, &req("scene/input_state", "{}", 7));
        assert_eq!(r3.get("held_keys").unwrap().as_array().unwrap().len(), 0);
        assert_eq!(
            r3.get("modifiers").unwrap().get("ctrl"),
            Some(&serde_json::Value::Bool(true)),
            "held-key release must not disturb the modifier cache",
        );
    }
}

// R682.B §5.16 — scene/cache_stats RPC dispatch wire
// ─────────────────────────────────────────────────────────────

mod r682b_cache_stats_rpc {
    use pinion_core::scene::Rect;
    use pinion_shell::{FragmentCacheStats, ShellCore};

    use super::TestView;

    /// Helper: build the JSON-RPC `scene/cache_stats` request frame.
    fn cache_stats_request(id: u64) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","method":"scene/cache_stats","params":{{}},"id":{id}}}"#,
        )
    }

    fn parse_response(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("response is JSON")
    }

    #[test]
    fn r682b_rpc_returns_unavailable_when_no_publish() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        let mut no_resize = |_: u32, _: u32| {};
        let resp = core
            .dispatch_rpc(&cache_stats_request(1), &mut no_resize)
            .expect("response carries id");
        let body = parse_response(&resp);
        let err = body.get("error").expect("dispatch surfaces error");
        let data = err.get("data").expect("error.data tags the variant");
        assert_eq!(data, "CacheStatsUnavailable");
    }

    #[test]
    fn r682b_rpc_returns_published_counters() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.publish_fragment_cache_stats(
            "main",
            FragmentCacheStats {
                hits: 240,
                misses: 12,
                paint_count: 5,
                entries: 12,
                last_damage_region: None,
            },
        );
        let mut no_resize = |_: u32, _: u32| {};
        let resp = core
            .dispatch_rpc(&cache_stats_request(2), &mut no_resize)
            .expect("response");
        let body = parse_response(&resp);
        let result = body.get("result").expect("dispatch ok");
        assert_eq!(result.get("hits").and_then(serde_json::Value::as_u64), Some(240));
        assert_eq!(result.get("misses").and_then(serde_json::Value::as_u64), Some(12));
        assert_eq!(result.get("paint_count").and_then(serde_json::Value::as_u64), Some(5));
        assert_eq!(result.get("entries").and_then(serde_json::Value::as_u64), Some(12));
        // hit_rate ≈ 240 / 252.
        let hit_rate = result.get("hit_rate").and_then(serde_json::Value::as_f64).unwrap();
        let expected = 240.0 / 252.0;
        assert!((hit_rate - expected).abs() < 1e-5);
        // last_damage_region absent (skip_serializing_if).
        assert!(result.get("last_damage_region").is_none());
    }

    #[test]
    fn r682b_rpc_emits_damage_region_when_present() {
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.publish_fragment_cache_stats(
            "main",
            FragmentCacheStats {
                hits: 0,
                misses: 1,
                paint_count: 1,
                entries: 1,
                last_damage_region: Some(Rect::new(8, 16, 320, 200)),
            },
        );
        let mut no_resize = |_: u32, _: u32| {};
        let resp = core
            .dispatch_rpc(&cache_stats_request(3), &mut no_resize)
            .expect("response");
        let body = parse_response(&resp);
        let result = body.get("result").expect("dispatch ok");
        let dmg = result
            .get("last_damage_region")
            .expect("damage region emitted");
        assert_eq!(dmg.get("x").and_then(serde_json::Value::as_u64), Some(8));
        assert_eq!(dmg.get("y").and_then(serde_json::Value::as_u64), Some(16));
        assert_eq!(dmg.get("w").and_then(serde_json::Value::as_u64), Some(320));
        assert_eq!(dmg.get("h").and_then(serde_json::Value::as_u64), Some(200));
    }

    #[test]
    fn r682b_rpc_default_window_resolves_to_main() {
        // `scene/cache_stats` with no window param falls back to
        // `DEFAULT_WINDOW = "main"`. Publish under "main", dispatch
        // without window → finds the stats.
        let _g = super::TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.publish_fragment_cache_stats(
            "main",
            FragmentCacheStats {
                hits: 1,
                misses: 0,
                paint_count: 1,
                entries: 1,
                last_damage_region: None,
            },
        );
        let mut no_resize = |_: u32, _: u32| {};
        let resp = core
            .dispatch_rpc(&cache_stats_request(4), &mut no_resize)
            .expect("response");
        let body = parse_response(&resp);
        assert!(body.get("result").is_some(), "default window resolves to main");
        // hit_rate at hits=1/misses=0 = 1.0.
        let hr = body
            .get("result")
            .and_then(|r| r.get("hit_rate"))
            .and_then(serde_json::Value::as_f64)
            .unwrap();
        assert!((hr - 1.0).abs() < 1e-6);
    }
}

// ─────────────────────────────────────────────────────────────
// R683 §5.16 §5.41 — `ShellCore::remove_window` per-window state
// drain pin. The shell-side wrapper around
// `pinion_runtime::CoreShell::remove_window` cleans the four
// per-window HashMaps lifted onto `ShellCore` since R680 + R681 +
// R682:
//
//   - `redraw_requested_per_window`
//   - `last_paint_instants`
//   - `target_fps_per_window`
//   - `fragment_cache_stats_per_window`
//
// + forwards into the runtime substrate which drains `routers`
// + `window_owners`. The reconcile-windows Effect's drop pass
// (R683 atomic 1) calls this for every spec id that disappeared
// from the binding's `Signal<Vec<WindowSpec>>`.
// ─────────────────────────────────────────────────────────────

mod r683_remove_window_shell_side {
    use pinion_shell::{FragmentCacheStats, ShellCore};

    use super::TestView;
    use super::TEST_LOCK;

    #[test]
    fn r683_remove_window_refuses_default_window_at_shell_level() {
        // The shell-side wrapper enforces the same primary-protection
        // contract as the runtime substrate. Even after publishing
        // per-window state into all four HashMaps, removing
        // DEFAULT_WINDOW must report `false` + the state must
        // survive.
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.request_redraw_for_window("main");
        core.set_target_fps_for_window("main", 60);
        core.publish_fragment_cache_stats(
            "main",
            FragmentCacheStats {
                hits: 1,
                misses: 0,
                paint_count: 1,
                entries: 1,
                last_damage_region: None,
            },
        );
        let removed = core.remove_window("main");
        assert!(!removed, "DEFAULT_WINDOW is primary-protected at shell level too");
        // Per-window state survives.
        assert!(core.redraw_requested_for_window("main"));
        assert_eq!(core.target_fps_for_window("main"), Some(60));
        assert!(core.fragment_cache_stats_for_window("main").is_some());
    }

    #[test]
    fn r683_remove_window_drains_per_window_maps() {
        // Publish a secondary entry into every per-window map +
        // remove. Every getter then reports the empty / None default.
        // (R831 added `pending_immediate_dt` + `sim_accumulator` to the
        // drained set; they have no public getter, so the three with
        // getters stand in for the whole cluster.)
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.request_redraw_for_window("inspector");
        core.set_target_fps_for_window("inspector", 30);
        core.publish_fragment_cache_stats(
            "inspector",
            FragmentCacheStats {
                hits: 2,
                misses: 1,
                paint_count: 3,
                entries: 2,
                last_damage_region: None,
            },
        );
        // Pre-state pin.
        assert!(core.redraw_requested_for_window("inspector"));
        assert_eq!(core.target_fps_for_window("inspector"), Some(30));
        assert!(core.fragment_cache_stats_for_window("inspector").is_some());
        // Removal.
        let removed = core.remove_window("inspector");
        assert!(removed, "secondary window with published state reports `true`");
        // Post-state — every map drained.
        assert!(
            !core.redraw_requested_for_window("inspector"),
            "redraw_requested_per_window cleared",
        );
        assert_eq!(
            core.target_fps_for_window("inspector"),
            None,
            "target_fps_per_window cleared",
        );
        assert!(
            core.fragment_cache_stats_for_window("inspector").is_none(),
            "fragment_cache_stats_per_window cleared",
        );
    }

    #[test]
    fn r683_remove_window_unknown_id_no_op() {
        // Defensive — the reconcile-windows drop pass may call
        // remove_window for an id that disappeared from the signal
        // before any per-window state was ever published (a
        // degenerate corner case during binding boot). Must not
        // panic + reports `false`.
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        let removed = core.remove_window("never-touched");
        assert!(!removed);
    }

    #[test]
    fn r683_remove_window_isolates_to_target_id() {
        // Sibling secondary scopes survive a remove targeting one of
        // them. Pins the per-id keying contract — the map removal
        // does not cascade across keys.
        let _guard = TEST_LOCK.lock().unwrap();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.request_redraw_for_window("inspector");
        core.request_redraw_for_window("palette");
        core.set_target_fps_for_window("inspector", 30);
        core.set_target_fps_for_window("palette", 144);
        assert!(core.remove_window("inspector"));
        // Palette intact.
        assert!(core.redraw_requested_for_window("palette"));
        assert_eq!(core.target_fps_for_window("palette"), Some(144));
        // Inspector gone.
        assert!(!core.redraw_requested_for_window("inspector"));
        assert_eq!(core.target_fps_for_window("inspector"), None);
    }
}

/// R684 §5.16 §5.41 §5.49 atomic 3 — headless-RPC floating-window
/// paint cycle. The substrate hook in
/// [`pinion_shell::ShellCore::dispatch_rpc_scoped`] re-runs the
/// paint pipeline + finalizes the addressed window's
/// [`pinion_runtime::InputRouter`] after the produce closure was
/// invoked. The tests below pin the contract:
///
/// 1. Calling `dispatch_rpc_scoped` with a paint-touching method
///    (e.g. `scene/snapshot {viewport: {…}}`) populates the
///    addressed window's `last_paint_scene` even when no winit
///    `RedrawRequested` cycle ever fires (the headless RPC case).
/// 2. Single-window `dispatch_rpc` (without window scope) does NOT
///    finalize on its own — single-window bindings drive finalize
///    through their `AppShell`'s winit paint loop; preserving the
///    legacy behaviour is a backward-compatibility guarantee.
/// 3. Non-paint-touching RPC methods (e.g. `focus/set`) do NOT
///    trigger the finalize because the produce closure is never
///    called (avoids gratuitous paint cycles on read-only dispatches).
/// 4. Sibling windows stay isolated — a dispatch to window A leaves
///    window B's router empty.
/// 5. Repeat dispatches keep the router populated (idempotent).
/// 6. (R889) The floating window must be REGISTERED first
///    (`ShellCore::register_window`, the `AppShell::resume_spec`
///    creation edge) — dispatch scoped to an unregistered id is
///    rejected with `-32602 unknown_window` before method routing,
///    so no router slot materialises for bogus ids.
mod r684_headless_rpc_floating_window_finalize {
    use super::*;
    use pinion_rpc::parse_request;

    fn snapshot_request(id: u64, window: &str, w: u32, h: u32) -> String {
        // R684 atomic 3 — use `scene/layout` because it ALWAYS calls
        // the produce closure when `viewport` is supplied (the only
        // path that triggers the post-dispatch finalize hook). The
        // alternative — `scene/snapshot {from: "paint", ...}` —
        // would also work but `scene/layout` is the simpler form
        // for these substrate-behaviour tests (no `path` /
        // `from` axes to thread).
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"scene/layout","params":{{"window":"{window}","viewport":{{"width":{w},"height":{h}}}}}}}"#,
        )
    }

    fn focus_get_request(id: u64, window: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"focus/get","params":{{"window":"{window}"}}}}"#,
        )
    }

    fn no_resize(_w: u32, _h: u32) {}

    #[test]
    fn r684_dispatch_rpc_scoped_populates_input_router_for_addressed_window() {
        // R684 atomic 3 anchor — the first-paint contract for
        // headless-RPC floating windows. Before this dispatch the
        // addressed window's router is empty (no winit paint
        // fired). After, the post-dispatch finalize hook re-runs
        // the paint pipeline + writes the result into the router.
        let _guard = TEST_LOCK.lock().unwrap();
        reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        // R889 — the floating window exists (registered at creation,
        // the AppShell::resume_spec edge) but has never painted.
        core.register_window("floating");
        assert!(
            !core.has_last_paint_scene_for_window("floating"),
            "fresh router for newly-spawned floating window is empty",
        );
        let req = parse_request(&snapshot_request(1, "floating", 320, 200))
            .expect("snapshot request parses");
        let _ = core.dispatch_rpc_scoped(req, &mut no_resize);
        // Post-condition: produce ran (snapshot calls it), so the
        // post-dispatch finalize populated the router.
        assert!(
            core.has_last_paint_scene_for_window("floating"),
            "post-dispatch finalize must populate the addressed window's router",
        );
    }

    #[test]
    fn r684_dispatch_rpc_single_window_does_not_finalize_default_window_router() {
        // Single-window `dispatch_rpc` passes `window_id = None`.
        // R684 atomic 3 explicitly skips the finalize in this path
        // so the legacy single-window behaviour stays bit-identical
        // — single-window bindings drive finalize through their
        // AppShell's winit paint loop, not through RPC dispatch.
        let _guard = TEST_LOCK.lock().unwrap();
        reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        assert!(
            !core.has_last_paint_scene_for_window(pinion_runtime::DEFAULT_WINDOW),
            "fresh DEFAULT_WINDOW router starts empty",
        );
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"scene/layout","params":{"viewport":{"width":320,"height":200}}}"#;
        let _ = core.dispatch_rpc(req, &mut no_resize);
        // R684 atomic 3 contract: single-window path does NOT
        // finalize. This is intentional backward-compat — changing
        // it would regress every single-window binding's interaction
        // with its AppShell-driven paint loop.
        assert!(
            !core.has_last_paint_scene_for_window(pinion_runtime::DEFAULT_WINDOW),
            "single-window dispatch_rpc must NOT finalize the DEFAULT_WINDOW router",
        );
    }

    #[test]
    fn r684_dispatch_rpc_scoped_focus_get_does_not_trigger_paint_finalize() {
        // `focus/get` is a pure substrate read — it never asks for
        // the paint scene. The produce closure stays unevoked, so
        // R684 atomic 3's post-dispatch finalize must NOT run
        // (avoids paint-cycle overhead for read-only RPCs).
        let _guard = TEST_LOCK.lock().unwrap();
        reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.register_window("floating");
        let req = parse_request(&focus_get_request(1, "floating"))
            .expect("focus_get request parses");
        let _ = core.dispatch_rpc_scoped(req, &mut no_resize);
        assert!(
            !core.has_last_paint_scene_for_window("floating"),
            "focus/get must not call produce → no post-dispatch finalize",
        );
    }

    #[test]
    fn r684_dispatch_rpc_scoped_isolates_finalize_to_addressed_window() {
        // Sibling-isolation pin — a dispatch to window A must NOT
        // populate window B's router. Critical for multi-window
        // bindings where each window has its own widget tree.
        let _guard = TEST_LOCK.lock().unwrap();
        reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        // R889 — both siblings registered (both exist); only A is
        // dispatched to, so the isolation pin is strictly about the
        // finalize hook, not about registration state.
        core.register_window("panel_a");
        core.register_window("panel_b");
        let req = parse_request(&snapshot_request(1, "panel_a", 200, 100))
            .expect("snapshot request parses");
        let _ = core.dispatch_rpc_scoped(req, &mut no_resize);
        assert!(
            core.has_last_paint_scene_for_window("panel_a"),
            "addressed window's router populated",
        );
        assert!(
            !core.has_last_paint_scene_for_window("panel_b"),
            "sibling window's router stays empty (no cross-window leak)",
        );
    }

    #[test]
    fn r684_dispatch_rpc_scoped_repeat_dispatches_keep_router_populated() {
        // Idempotent under repeat dispatch — the second snapshot
        // overwrites the first with the same scene (same view fn,
        // same state). Post-condition stays consistent.
        let _guard = TEST_LOCK.lock().unwrap();
        reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.register_window("floating");
        for id in 1_u64..=3 {
            let req = parse_request(&snapshot_request(id, "floating", 320, 200))
                .expect("snapshot request parses");
            let _ = core.dispatch_rpc_scoped(req, &mut no_resize);
            assert!(
                core.has_last_paint_scene_for_window("floating"),
                "router stays populated across repeat dispatches (iter {id})",
            );
        }
    }

    #[test]
    fn r684_dispatch_rpc_scoped_finalize_feeds_the_layout_projection() {
        // R890 — the finalize hook stores the addressed window's
        // paint scene, and THAT is what `scene/layout {viewport:
        // null}` projects (per-window; the pre-R890 binding-wide
        // `last_paint_layout` mirror is gone). The projection answers
        // only for the addressed window — a sibling stays honestly
        // absent instead of inheriting the last writer's tree.
        let _guard = TEST_LOCK.lock().unwrap();
        reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.register_window("floating");
        core.register_window("sibling");
        assert!(
            core.last_paint_layout_for_window("floating").is_none(),
            "never-painted window has no layout projection",
        );
        let req = parse_request(&snapshot_request(1, "floating", 320, 200))
            .expect("snapshot request parses");
        let _ = core.dispatch_rpc_scoped(req, &mut no_resize);
        let layout = core
            .last_paint_layout_for_window("floating")
            .expect("post-dispatch finalize feeds the projection");
        assert_eq!(
            (layout.rect.w, layout.rect.h),
            (320, 200),
            "projection carries the addressed window's own viewport",
        );
        assert!(
            core.last_paint_layout_for_window("sibling").is_none(),
            "sibling window does NOT inherit the finalize (no cross-window mirror)",
        );
    }

    #[test]
    fn r890_layout_viewport_null_answers_per_window_not_last_writer() {
        // THE R890 wire pin: paint window A at one size, then read
        // `scene/layout {viewport: null}` scoped to known-but-
        // unpainted window B. Pre-R890 B answered with A's tree (the
        // last-writer-wins mirror / slot fallback); post-R890 B gets
        // the honest NoLastPaintLayout and A keeps its own geometry.
        let _guard = TEST_LOCK.lock().unwrap();
        reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        core.register_window("win_a");
        core.register_window("win_b");
        // Paint A via a viewport-supplied layout call (runs produce +
        // the R684 finalize stores A's scene).
        let req = parse_request(&snapshot_request(1, "win_a", 320, 200))
            .expect("frame parses");
        let _ = core.dispatch_rpc_scoped(req, &mut no_resize);
        // A's viewport:null read = A's own frame.
        let req = parse_request(
            r#"{"jsonrpc":"2.0","id":2,"method":"scene/layout","params":{"window":"win_a"}}"#,
        )
        .expect("frame parses");
        let resp = core
            .dispatch_rpc_scoped(req, &mut no_resize)
            .expect("response");
        assert!(
            resp.contains(r#""result""#) && resp.contains(r#""w":320"#),
            "win_a viewport:null answers its own painted frame: {resp}",
        );
        // B (known, never painted) must NOT inherit A's tree.
        let req = parse_request(
            r#"{"jsonrpc":"2.0","id":3,"method":"scene/layout","params":{"window":"win_b"}}"#,
        )
        .expect("frame parses");
        let resp = core
            .dispatch_rpc_scoped(req, &mut no_resize)
            .expect("response");
        assert!(
            resp.contains(r#""error""#) && resp.contains("NoLastPaintLayout"),
            "known-unpainted window answers honestly, not with A's tree: {resp}",
        );
    }

    #[test]
    fn r889_dispatch_rpc_scoped_unknown_window_id_is_rejected_without_finalize() {
        // R889 — REPLACES the pre-R889 pin "invalid window id still
        // finalizes under that key": the substrate now validates the
        // window scope against the window-known registry
        // (`CoreShell::is_window_known`) and rejects unknown ids with
        // `-32602 unknown_window` BEFORE method routing. No router
        // slot materialises for the bogus id, so an AI client typo
        // can neither read another window's state nor leave per-id
        // residue in the substrate.
        let _guard = TEST_LOCK.lock().unwrap();
        reset_mocks();
        let mut core: ShellCore<TestView> = ShellCore::new();
        let req = parse_request(&snapshot_request(1, "any_unknown_window_id", 400, 300))
            .expect("snapshot request parses");
        let resp = core
            .dispatch_rpc_scoped(req, &mut no_resize)
            .expect("call requests always get a response frame");
        assert!(
            resp.contains(r#""code":-32602"#) && resp.contains("unknown_window"),
            "unknown window scope is rejected wholesale: {resp}",
        );
        assert!(
            resp.contains("any_unknown_window_id"),
            "error data names the supplied id: {resp}",
        );
        assert!(
            !core.has_last_paint_scene_for_window("any_unknown_window_id"),
            "no router slot materialises for a rejected window id",
        );
    }
}
