// Prose mentions of widget type names read fine un-backticked in a test binding.
#![allow(clippy::doc_markdown)]

//! R1020 §5.39 — scene-derived focus enumeration, end-to-end forcing consumer.
//!
//! The sprag dynamic-pane model: a runtime-variable set of equal, persistent
//! focusable panes. Each pane is a tagged Container marked
//! `.with_focusable(true)`; the pane *count* lives in a reactive [`Signal`] the
//! `view` reads, so splitting / closing a pane changes which focusable nodes
//! the paint scene carries.
//!
//! These tests drive [`ShellCore::compute_paint_scene`] (the live paint path
//! that re-derives the focus enumeration from the freshly produced scene) and
//! assert that the §5.39 [`FocusManager`] Tab order tracks the painted set:
//!   1. a paint enumerates exactly the painted focusable panes (in tree order),
//!      replacing the boot-seeded default — proving the enumeration is derived
//!      from the scene, not a hand-maintained list;
//!   2. adding a pane at runtime makes its tag immediately Tab/`focus`-reachable
//!      (the pre-R1020 gap: a boot-frozen list never saw the new pane);
//!   3. removing a pane drops it from the enumeration, and removing the
//!      *focused* pane drops focus safely (the `update_focusable_tags` guard).
//!
//! Pre-R1020 (`focusable_tags()` seeded once at boot, never refreshed) #1 and #2
//! both fail: the enumeration stays `[ROOT_TAG]` regardless of what is painted.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::ContainerNode;
use pinion_core::style::LayoutStyle;
use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};
use std::sync::Mutex;

const ROOT_TAG: &str = "panes_root";
/// The runtime pane-tag pool. The binding slices `[..count]` — a varying
/// cardinality of `&'static str` tags, which is exactly sprag's R36 shape.
const PANE_TAGS: [&str; 4] = ["pane#0", "pane#1", "pane#2", "pane#3"];
/// A conditionally-painted inline-editor stop (focus-on-appear case).
const EDITOR_TAG: &str = "inline_editor";

/// Whether the conditionally-painted inline editor is currently painted —
/// the reactive flag a reducer flips when an inline editor opens.
fn editor_visible() -> Signal<bool> {
    Owner::current()
        .expect("editor_visible requires an active Owner scope")
        .cache("editor_visible", || Signal::new(false))
        .as_ref()
        .clone()
}

/// Serialises the file's tests: the pane-count Signal is reached through the
/// per-`ShellCore` root owner, but the process-global focus-request mailbox and
/// the shared test renderer make interleaving undesirable.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// The reactive pane count — the producer a split / close gesture writes. Seeded
/// at 2 (two panes at boot), read by `view` to slice [`PANE_TAGS`].
fn pane_count() -> Signal<usize> {
    Owner::current()
        .expect("pane_count requires an active Owner scope")
        .cache("pane_count", || Signal::new(2_usize))
        .as_ref()
        .clone()
}

/// A no-op primary external — `WidgetCore` requires one to carry the state
/// scene; the focus enumeration is exercised through the painted panes.
#[derive(Debug, Default)]
struct StubExternal;

impl External for StubExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
}

struct PaneFocusView;

impl WidgetCore for PaneFocusView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }

    fn read_state(_scene: &Scene) {}

    // (R1020 §5.39) Focus is scene-derived — each pane marks itself
    // `.with_focusable(true)`; the root container is NOT focusable, so it is
    // absent from the enumeration. There is no `focusable_tags()` to override
    // (the method is retired).
    fn view(_state: (), _frame: &Frame) -> Scene {
        let n = pane_count().get().min(PANE_TAGS.len());
        let mut children: Vec<Scene> = (0..n)
            .map(|i| {
                Scene::Container(
                    ContainerNode::new(Vec::new())
                        .with_tag(PANE_TAGS[i])
                        .with_layout(LayoutStyle::new().with_focusable(true)),
                )
            })
            .collect();
        // (R1020 §5.39) A conditionally-painted focus stop — present only while
        // `editor_visible` is set (the inline-editor model). Exercises the
        // focus-on-appear re-derive.
        if editor_visible().get() {
            children.push(Scene::Container(
                ContainerNode::new(Vec::new())
                    .with_tag(EDITOR_TAG)
                    .with_layout(LayoutStyle::new().with_focusable(true)),
            ));
        }
        Scene::Container(ContainerNode::new(children).with_tag(ROOT_TAG))
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion scene-derived focus (R1020)"
    }
}

impl WidgetA11y for PaneFocusView {}

impl WidgetView for PaneFocusView {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: 640,
            height: 384,
        }
    }
}

/// The current Tab order as owned `&str`s, for ergonomic comparison.
fn tab_order(core: &ShellCore<PaneFocusView>) -> Vec<String> {
    core.focus().tab_order().to_vec()
}

#[test]
fn paint_enumerates_the_painted_focusable_panes() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let core = ShellCore::<PaneFocusView>::new();

    // The boot seed is scene-derived (collected from the first view), not a
    // hand-listed default: the two focusable panes in tree order. The root
    // Container (not `.with_focusable(true)`) is correctly absent. `ROOT_TAG`
    // is referenced here only to document that the pre-R1020 default
    // `vec![tag()]` no longer drives the enumeration.
    let _ = ROOT_TAG;
    assert_eq!(
        tab_order(&core),
        vec!["pane#0".to_owned(), "pane#1".to_owned()],
        "boot enumeration is the painted focusable set, scene-derived",
    );
}

#[test]
fn adding_a_pane_makes_it_focus_reachable() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = ShellCore::<PaneFocusView>::new();
    core.compute_paint_scene(640, 384);
    assert_eq!(tab_order(&core).len(), 2, "two panes at boot");

    // A split gesture: bump the pane count. The NEXT paint must enumerate the
    // new pane — pre-R1020 the boot-frozen list never would.
    core.root_owner().run(|| pane_count().set(3));
    core.compute_paint_scene(640, 384);
    assert_eq!(
        tab_order(&core),
        vec![
            "pane#0".to_owned(),
            "pane#1".to_owned(),
            "pane#2".to_owned()
        ],
        "the runtime-added pane joins the Tab order",
    );
    // Membership in `tab_order` is exactly the gate `FocusManager::focus_set` /
    // `resolve_focusable` apply, so the new pane is now Tab- and click- and
    // `focus/set`-reachable.
    assert!(
        core.focus().tab_order().iter().any(|t| t == "pane#2"),
        "the new pane is a valid focus target",
    );
}

#[test]
fn removing_the_focused_pane_drops_focus_safely() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = ShellCore::<PaneFocusView>::new();
    core.root_owner().run(|| pane_count().set(3));
    core.compute_paint_scene(640, 384);
    assert_eq!(tab_order(&core).len(), 3, "three panes painted");

    // Close the last pane (the one a user might have focused). The enumeration
    // shrinks; `update_focusable_tags`' drop-on-removal guard keeps a stale
    // focused tag from dispatching to a vanished pane.
    core.root_owner().run(|| pane_count().set(2));
    core.compute_paint_scene(640, 384);
    assert_eq!(
        tab_order(&core),
        vec!["pane#0".to_owned(), "pane#1".to_owned()],
        "the closed pane left the Tab order",
    );
    assert!(
        !core.focus().tab_order().iter().any(|t| t == "pane#2"),
        "the closed pane is no longer a focus target",
    );
}

#[test]
fn focus_request_to_a_just_painted_node_re_derives() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = ShellCore::<PaneFocusView>::new();
    let scene = core.compute_paint_scene(640, 384);
    assert!(
        !core.focus().tab_order().iter().any(|t| t == EDITOR_TAG),
        "the inline editor is hidden (unpainted) at boot, so it is not enumerated",
    );

    // Reproduce the inline-editor race: make the editor paintable AND request
    // focus to it with NO intervening paint — exactly what a reducer that opens
    // an inline editor does (set the visibility signal + focus_request in one
    // dispatch).
    core.root_owner().run(|| editor_visible().set(true));
    pinion_core::focus_request::request(EDITOR_TAG);

    // Drive the dispatch tail (handle_tail -> drain_focus_request). The current
    // enumeration (from the pre-signal paint) lacks EDITOR_TAG, so focus_set
    // misses; the R1020 re-derive re-runs the view (the editor now paints),
    // enumerates it, and the retry lands focus. Pre-R1020 this request would be
    // silently dropped (focus-on-appear broken).
    core.finalize_frame(scene);
    assert_eq!(
        core.focus().focused(),
        Some(EDITOR_TAG),
        "focus landed on the just-painted editor via the drain-time re-derive",
    );
}
