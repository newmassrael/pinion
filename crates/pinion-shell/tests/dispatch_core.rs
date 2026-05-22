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
            .with_style(BoxStyle::filled(Color::rgb(0x00, 0x00, 0x00))),
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

    fn apply_key(_scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
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

    fn initial_size() -> (u32, u32) {
        (8, 8)
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
    let scene = core.compute_paint_scene(64, 32);
    core.finalize_frame(scene);

    // The §5.12 last-paint snapshot drives RPC `scene/layout
    // {viewport: null}` — finalize must populate it so the AI client
    // can read the frame the user actually sees.
    // (We only assert the substrate-visible side effect: that some
    // other dispatch chain that depends on the snapshot doesn't
    // panic. Direct access is `pub(crate)` so the assertion is
    // indirect — re-running finalize_frame again must remain safe
    // and idempotent.)
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
    let (nodes, focus) = core.collect_access_emit_inputs(&scene);

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
    use pinion_core::test_fixtures::EchoButtonFixture;
    use pinion_core::Intent;
    use pinion_shell::ShellCore;

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
