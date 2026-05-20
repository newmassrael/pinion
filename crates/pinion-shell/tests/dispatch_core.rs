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

use core::fmt;
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
use pinion_shell::{vello_renderer_impl, ShellCore, WidgetView};

// ---------- Test-fixture VelloRenderer ----------------------------------
//
// Mirrors the pinion-forge codegen template: an inherent
// `async new` / `render` / `resize` triple wrapped by
// `vello_renderer_impl!` to satisfy the `WidgetView::Renderer` bound.
// The shell's dispatch path never touches the renderer, so the bodies
// are empty.

struct TestRenderer;

#[derive(Debug)]
enum TestRendererError {}

impl fmt::Display for TestRendererError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl std::error::Error for TestRendererError {}

impl TestRenderer {
    #[allow(clippy::unused_async)]
    async fn new<W>(
        _target: W,
        _width: u32,
        _height: u32,
    ) -> Result<Self, TestRendererError>
    where
        W: Into<vello::wgpu::SurfaceTarget<'static>>,
    {
        Ok(Self)
    }

    fn render(
        &mut self,
        _scene: &vello::Scene,
        _base: vello::peniko::Color,
    ) -> Result<(), TestRendererError> {
        Ok(())
    }

    fn resize(&mut self, _w: u32, _h: u32) {}
}

vello_renderer_impl!(TestRenderer, TestRendererError);

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

fn reset_mocks() {
    APPLY_KEY_LOG.lock().unwrap().clear();
    APPLY_KEY_RETURNS.store(false, Ordering::SeqCst);
    CHILD_INVOKE_LOG.lock().unwrap().clear();
    CHILD_INVOKE_RETURNS.store(false, Ordering::SeqCst);
    KEYBINDING_RETURNS_SOME.store(false, Ordering::SeqCst);
    EVENT_NAME_LOG.lock().unwrap().clear();
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
        APPLY_KEY_RETURNS.load(Ordering::SeqCst)
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
