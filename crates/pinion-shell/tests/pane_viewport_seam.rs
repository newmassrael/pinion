// Prose mentions of widget type names (TextGrid, …) read fine un-backticked in
// a test binding — same looser doc-markdown lid the hello-* example bindings use.
#![allow(clippy::doc_markdown)]

//! R1012 §5.23 §5.22 — per-pane viewport seam, end-to-end forcing consumer.
//!
//! A two-pane horizontal split (a `3 : 5` flex divide of the layout viewport)
//! where each pane is a [`Scene::TextGrid`] hosting a mock PTY whose
//! `(cols, rows)` reflows from **its own** measured pane rect via
//! [`use_pane_viewport_size`] — the sprag multi-pane terminal model. The root
//! fills the window with R1006 [`use_viewport_size`]; R1012 hands each pane its
//! sub-rect. Both seams compose here.
//!
//! These tests drive [`ShellCore::compute_paint_scene`] (the *live* paint path
//! that publishes pane rects — the side-effect-free mirror never reaches it) and
//! assert that:
//!   1. each pane reflows to ITS measured `(cols, rows)`, and the two differ
//!      (per-pane, not the window-global value R1006 alone would give);
//!   2. the painted grid reflects the post-reflow dims on the SAME paint (the
//!      scroll-dirty same-frame re-pass the publish dirty-bit drives);
//!   3. a window resize re-divides the panes and re-fires each reflow
//!      independently;
//!   4. an unchanged repaint is inert (Signal equality-skip — no extra reflow,
//!      no re-pass loop).

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, TextGridNode};
use pinion_core::style::{AlignItems, FlexDirection, LayoutStyle, Size, SizeValue};
use pinion_core::{
    CellMetric, Effect, Frame, GridBuffer, Owner, Scene, Signal, WidgetCore,
    use_pane_viewport_size, use_viewport_size,
};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};
use std::sync::Mutex;

/// The 8×16 baseline metric: `cell_w = 8`, `cell_h = 16`.
const CELL: CellMetric = CellMetric::DEFAULT;
const LEFT_TAG: &str = "pane.left";
const RIGHT_TAG: &str = "pane.right";
const LEFT_GRID_TAG: &str = "pane.left.grid";
const RIGHT_GRID_TAG: &str = "pane.right.grid";
/// R1021 — the secondary (torn-off) window id hosting ONLY the left pane.
const FLOAT_LEFT_WINDOW: &str = "float.left";

/// Serialises the file's tests: the binding writes the reflow log through a
/// process-global static, and `compute_paint_scene` touches no per-test owner
/// the lock could not isolate, but the log must not interleave.
static TEST_LOCK: Mutex<()> = Mutex::new(());
/// Every per-pane reflow Effect appends `(tag, (cols, rows))` here when it fires
/// with a measured (non-`(0, 0)`) size — the witness that the reflow ran, with
/// what dimensions, and how many times.
static PTY_LOG: Mutex<Vec<(String, (u16, u16))>> = Mutex::new(Vec::new());

/// Retains a pane's reflow [`Effect`] for the life of the owner (the owner-cache
/// slot owns this marker; dropping it would unsubscribe the reflow).
struct ReflowMarker {
    _effect: Effect,
}

/// The pane's mock-PTY winsize signal, lazily registered once per owner.
fn pty_dims(tag: &'static str) -> Signal<(u16, u16)> {
    Owner::current()
        .expect("pty_dims requires an active Owner scope")
        .cache(tag, || Signal::new((0_u16, 0_u16)))
        .as_ref()
        .clone()
}

/// The owner-cache key retaining the pane's reflow Effect marker.
fn reflow_key(tag: &'static str) -> &'static str {
    match tag {
        LEFT_TAG => "pane.left.reflow",
        RIGHT_TAG => "pane.right.reflow",
        other => unreachable!("only the two pane tags are paneled, got {other:?}"),
    }
}

/// Install the pane's reflow [`Effect`] exactly once. The Effect's body reads
/// [`use_pane_viewport_size`] (the sprag PTY model: on a measured size it
/// reflows the mock PTY's `(cols, rows)` and logs it; it skips the `(0, 0)`
/// "unmeasured" sentinel so no spurious `1 x 1` reflow fires at boot).
///
/// Because that body resolves an `Owner::cache` slot (the pane registry), the
/// Effect must NOT be created inside an `Owner::cache` factory (the R666
/// no-nested-cache rule — its eager run would re-enter `cache`). So it is
/// created here at view scope, guarded by `cache_contains` for once-install,
/// exactly as R1006's window reflow Effect is created in `create_extra_externals`
/// rather than a cache factory.
fn install_reflow(tag: &'static str) {
    let owner = Owner::current().expect("install_reflow requires an active Owner scope");
    if owner.cache_contains::<ReflowMarker>(reflow_key(tag)) {
        return; // already installed — do not re-create the Effect each view
    }
    let dims = pty_dims(tag);
    let effect = Effect::new(&owner, move || {
        let (w, h) = use_pane_viewport_size(tag);
        if w == 0 || h == 0 {
            return; // pane unmeasured — skip
        }
        let reflowed = (CELL.cols_for(w), CELL.rows_for(h));
        dims.set(reflowed);
        PTY_LOG
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((tag.to_owned(), reflowed));
    });
    owner.cache(reflow_key(tag), move || ReflowMarker { _effect: effect });
}

/// One pane: a tagged Container (the measured rect target) holding a TextGrid
/// sized to the pane's mock-PTY `(cols, rows)`. `flex_basis(0) + flex_grow(g)`
/// is the CSS proportional-split idiom; the pane's width is its flex share of
/// the row, its height the row's stretched cross-axis.
fn pane(tag: &'static str, grid_tag: &'static str, grow: f32) -> Scene {
    install_reflow(tag);
    let (cols, rows) = pty_dims(tag).get(); // the re-pass reads the post-reflow dims
    let grid = Scene::TextGrid(
        TextGridNode::new(CELL)
            .with_tag(grid_tag)
            .with_cells(GridBuffer::new(cols.max(1), rows.max(1))),
    );
    Scene::Container(
        ContainerNode::new(vec![grid]).with_tag(tag).with_layout(
            LayoutStyle::new()
                .with_flex_basis(SizeValue::Px(0))
                .with_flex_grow(grow),
        ),
    )
}

/// A no-op primary external: the seam is exercised through the TextGrid panes
/// and their per-pane reflow Effects, not this handle, but `WidgetCore` requires
/// a primary external to carry the state scene.
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

struct PaneView;

impl WidgetCore for PaneView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        "panes"
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }

    fn read_state(_scene: &Scene) {}

    fn view(_state: (), _frame: &Frame) -> Scene {
        // R1006: the root fills the window from the published viewport size;
        // R1012: each pane flex-splits that into its own measured rect.
        let (w, h) = use_viewport_size();
        Scene::Container(
            ContainerNode::new(vec![
                pane(LEFT_TAG, LEFT_GRID_TAG, 3.0),
                pane(RIGHT_TAG, RIGHT_GRID_TAG, 5.0),
            ])
            .with_tag("panes")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Stretch)
                    .with_size(Size::px(w.max(1), h.max(1))),
            ),
        )
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion pane-viewport seam (R1012)"
    }
}

impl WidgetA11y for PaneView {}

impl WidgetView for PaneView {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: 640,
            height: 384,
        }
    }

    /// R1021 — a torn-off (undock) secondary window hosting ONLY the left pane.
    /// The pane Container is the window root, so `compute_layout`'s root-fill
    /// stretches it to the window `(w, h)` (the declared flex size is ignored at
    /// the top level) — no `use_viewport_size` (which stays primary-gated). The
    /// per-window pane publish (R1021, gate removed) then reflows the left pane to
    /// the secondary window's size. The main window keeps the two-pane split.
    fn view_for_window(window_id: &str, state: Self::State, frame: &Frame) -> Scene {
        if window_id == FLOAT_LEFT_WINDOW {
            pane(LEFT_TAG, LEFT_GRID_TAG, 1.0)
        } else {
            Self::view(state, frame)
        }
    }
}

/// Find the `tag`ged TextGrid in a painted scene and return its buffer dims —
/// the proof that the paint reflects the post-reflow producer state (the
/// same-frame re-pass), not a one-frame-late grid.
fn painted_grid_dims(scene: &Scene, tag: &str) -> Option<(u16, u16)> {
    match scene {
        Scene::TextGrid(g) if g.tag.as_deref() == Some(tag) => {
            Some((g.cells.cols(), g.cells.rows()))
        }
        Scene::Container(c) => c.children.iter().find_map(|ch| painted_grid_dims(ch, tag)),
        _ => None,
    }
}

#[test]
fn each_pane_reflows_to_its_own_measured_rect() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();

    let mut core = ShellCore::<PaneView>::new();
    // A 640×384 window: the 3:5 split is 240px : 400px wide, both 384px tall =>
    // 30×24 and 50×24 cells at the 8×16 metric.
    let scene = core.compute_paint_scene(640, 384);

    // (1) per-pane reflow: the two panes reflow to DIFFERENT dims — the value a
    // window-global seam (R1006 alone) could never produce.
    let left = core.root_owner().run(|| pty_dims(LEFT_TAG).get());
    let right = core.root_owner().run(|| pty_dims(RIGHT_TAG).get());
    assert_eq!(left, (30, 24), "left pane: 240/8 x 384/16");
    assert_eq!(right, (50, 24), "right pane: 400/8 x 384/16");
    assert_ne!(
        left, right,
        "panes reflow independently, not to the window size"
    );

    // (2) the painted grids reflect the post-reflow dims on THIS paint (the
    // same-frame re-pass the pane-dirty bit drove), not one frame late.
    assert_eq!(painted_grid_dims(&scene, LEFT_GRID_TAG), Some((30, 24)));
    assert_eq!(painted_grid_dims(&scene, RIGHT_GRID_TAG), Some((50, 24)));

    // The reflow log holds exactly one reflow per pane for this first paint.
    // Snapshot to a local Vec so the guard is not held across an assert (a
    // panic mid-assert must not poison the mutex for the sibling tests).
    let log = PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(log.contains(&(LEFT_TAG.to_owned(), (30, 24))));
    assert!(log.contains(&(RIGHT_TAG.to_owned(), (50, 24))));
    assert_eq!(
        log.len(),
        2,
        "one reflow per pane, no spurious (0,0) reflow"
    );
}

#[test]
fn resize_redivides_and_refires_each_pane() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();

    let mut core = ShellCore::<PaneView>::new();
    core.compute_paint_scene(640, 384);
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear(); // drop the first-paint reflows

    // Resize to 800×384: 3:5 of 800 = 300px : 500px => 37×24 and 62×24 cells
    // (300/8 = 37.5 floors to 37; 500/8 = 62.5 floors to 62).
    let scene = core.compute_paint_scene(800, 384);
    let left = core.root_owner().run(|| pty_dims(LEFT_TAG).get());
    let right = core.root_owner().run(|| pty_dims(RIGHT_TAG).get());
    assert_eq!(left, (37, 24), "left re-divides on resize");
    assert_eq!(right, (62, 24), "right re-divides on resize");
    assert_eq!(painted_grid_dims(&scene, LEFT_GRID_TAG), Some((37, 24)));
    assert_eq!(painted_grid_dims(&scene, RIGHT_GRID_TAG), Some((62, 24)));

    // Snapshot to a local Vec so the guard is not held across an assert (a
    // panic mid-assert must not poison the mutex for the sibling tests).
    let log = PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(log.contains(&(LEFT_TAG.to_owned(), (37, 24))));
    assert!(log.contains(&(RIGHT_TAG.to_owned(), (62, 24))));
    assert_eq!(
        log.len(),
        2,
        "each pane re-fires exactly once on the resize"
    );
}

#[test]
fn unchanged_repaint_is_inert() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();

    let mut core = ShellCore::<PaneView>::new();
    core.compute_paint_scene(640, 384);
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear(); // drop the first-paint reflows

    // Two more paints at the SAME size: the pane signals equality-skip, so no
    // reflow Effect re-fires and the pane-dirty bit floors to false (the single
    // re-pass `if` is then skipped — there is no loop to guard, only an
    // idempotent steady state).
    core.compute_paint_scene(640, 384);
    core.compute_paint_scene(640, 384);
    let reflowed = PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(
        reflowed.is_empty(),
        "an unchanged repaint publishes the same pane rects (no re-fire)"
    );
}

#[test]
fn nondivisible_width_floors_each_pane_independently() {
    // R1012.2 (clearance) — the happy-path 640/800 widths are exact multiples of
    // the 3:5 split AND the 8px cell, so neither the flex rounding nor the
    // cols_for floor ever bites (R988.1: a "clean" size hides off-by-one). 638
    // is not 8-divisible after the split: 3:5 of 638 lays out to 239px : 399px,
    // and cols_for floors a partial trailing cell => 29 and 49 cols (not 30/50).
    // This pins the flex-round x winsize-floor interaction at a boundary width.
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();

    let mut core = ShellCore::<PaneView>::new();
    let scene = core.compute_paint_scene(638, 384);
    let left = core.root_owner().run(|| pty_dims(LEFT_TAG).get());
    let right = core.root_owner().run(|| pty_dims(RIGHT_TAG).get());
    assert_eq!(left, (29, 24), "239px / 8 floors the partial cell");
    assert_eq!(right, (49, 24), "399px / 8 floors the partial cell");
    // The painted grid floors identically (publish + paint read the same rect),
    // so a non-cell-aligned pane never diverges hit-vs-paint.
    assert_eq!(painted_grid_dims(&scene, LEFT_GRID_TAG), Some((29, 24)));
    assert_eq!(painted_grid_dims(&scene, RIGHT_GRID_TAG), Some((49, 24)));
}

#[test]
fn secondary_window_publishes_its_pane_without_clobbering_absent_tags() {
    // R1021 §5.23 §5.16 — the per-window pane publish (the DEFAULT_WINDOW gate on
    // `publish_pane_viewports` removed). The forcing case: sprag R37 undock, where
    // a pane torn off into its own OS window must reflow to THAT window's size.
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();

    let mut core = ShellCore::<PaneView>::new();

    // (1) Main window paints the 3:5 split: both panes register + reflow to their
    //     split rects (the docked baseline).
    core.compute_paint_scene(640, 384);
    assert_eq!(core.root_owner().run(|| pty_dims(LEFT_TAG).get()), (30, 24));
    assert_eq!(
        core.root_owner().run(|| pty_dims(RIGHT_TAG).get()),
        (50, 24)
    );
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear(); // drop first-paint reflows

    // (2) The left pane is torn off into its own 320×192 secondary window. Before
    //     R1021 this published nothing (gated to DEFAULT_WINDOW); now the secondary
    //     window publishes the rect of the tag IT draws. The pane fills the window
    //     via layout root-fill (no use_viewport_size), so it reflows to the
    //     SECONDARY window size: 320/8 × 192/16 = 40×12.
    let scene = core.compute_paint_scene_for_window(FLOAT_LEFT_WINDOW, 320, 192);
    assert_eq!(
        core.root_owner().run(|| pty_dims(LEFT_TAG).get()),
        (40, 12),
        "the torn-off pane reflows to its secondary window's size"
    );
    // The same-frame re-pass makes the painted grid reflect the post-reflow dims.
    assert_eq!(painted_grid_dims(&scene, LEFT_GRID_TAG), Some((40, 12)));

    // (3) The right pane is absent from the secondary window's scene →
    //     rect_for_tag_absolute resolves None → skipped → it RETAINS its last
    //     docked size, NOT clobbered by the foreign window's paint.
    assert_eq!(
        core.root_owner().run(|| pty_dims(RIGHT_TAG).get()),
        (50, 24),
        "a tag absent from the painting window is skipped, never clobbered"
    );

    // Only the drawn pane re-fires; the absent pane is inert.
    let log = PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(log, vec![(LEFT_TAG.to_owned(), (40, 12))]);
}

#[test]
fn same_tag_in_two_windows_is_last_writer_wins() {
    // R1021.1 — pins the UNCHECKED PRECONDITION documented on
    // CoreShell::publish_pane_viewports: a pane tag is drawn in at most one window
    // per frame. The registry is tag-keyed (one signal per tag, shared across
    // windows — it must be, since the consumer resolves under the binding-wide
    // root_owner with no window to disambiguate by, R680). So if the SAME tag is
    // drawn in two windows, the last window to paint wins, and the pane's reflow
    // oscillates as the windows alternate painting. This test makes that teeth
    // explicit: the dock/tear-off model avoids it by drawing each tag in exactly
    // one window (the primary drops a pane when it floats); a future "mirror one
    // pane in N windows" feature cannot use this seam as-is.
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PTY_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();

    let mut core = ShellCore::<PaneView>::new();

    // Primary draws LEFT_TAG (in the 3:5 split) -> 240x384 -> 30x24.
    core.compute_paint_scene(640, 384);
    assert_eq!(core.root_owner().run(|| pty_dims(LEFT_TAG).get()), (30, 24));

    // The FLOAT window also draws LEFT_TAG (filling 320x192) -> 40x12. Because the
    // signal is shared per tag, the float paint (last writer) clobbers the
    // primary's value — this is the precondition violation made visible.
    core.compute_paint_scene_for_window(FLOAT_LEFT_WINDOW, 320, 192);
    assert_eq!(
        core.root_owner().run(|| pty_dims(LEFT_TAG).get()),
        (40, 12),
        "two windows on one tag: the last painter wins (float clobbers primary)",
    );

    // Re-painting the primary flips it straight back -> 30x24. Across alternating
    // frames the tag would oscillate; this is exactly why the dock model keeps a
    // tag in one window at a time.
    core.compute_paint_scene(640, 384);
    assert_eq!(
        core.root_owner().run(|| pty_dims(LEFT_TAG).get()),
        (30, 24),
        "re-painting the primary re-clobbers: last-writer-wins, not stable",
    );
}
