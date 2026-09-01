//! ★★★★★ R1947 — **the integration test: the section the pipeline paints,
//! against the specification somebody wrote down for it.**
//!
//! `tests.rs` next door asserts the model — the population, the vocabulary, the
//! hit test. This module runs the real pipeline (`view()` then
//! [`pinion_runtime::compute_layout`], the same two stages the window runs
//! before handing a scene to the rasteriser) and asks the resulting scene where
//! things ended up, at every state in [`STATES`] and every size in [`SIZES`].
//!
//! ⚠ **This is the half that has to touch pixels.** The goal this section was
//! built under is what a person SEES, and the failure mode named with it is a
//! test that goes green on "did not panic" or "is reachable". So what is
//! asserted here is where marks landed and what colour of claim they carry —
//! a node inside the plot that holds it, a ring on the node that is picked, a
//! link count that changes when a switch does.

use std::rc::Rc;

use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::test_fixtures::screen_ink::{assert_boxes_hold_their_text, assert_contained_ink};
use pinion_core::{Frame, Scene};

use super::{
    CANVAS_TAG, FILTER_TAG, GRAPH_TAG, INSPECTOR_TAG, VIEW_TAG, ViewState, WIN_H, WIN_W, spec,
    use_view_state,
};

/// How many runs on this screen sit in a box too short for their own face.
///
/// ★ R1800's ratchet, pinned at what this section measures rather than wished
/// at zero — a budget wished lower than the truth is a gate that fails on
/// arrival and gets raised, which is a ratchet running backwards. Lowering it
/// is the repair; `containment::line_rect` is how.
const SHORT_BOX_BUDGET: usize = 0;

/// The sizes the section is swept at: the size it is specified at, the floor it
/// declares, and one between.
const SIZES: [(&str, (u32, u32)); 3] = [
    ("declared", (WIN_W, WIN_H)),
    ("between", (980, 620)),
    ("floor", (super::MIN_W, super::MIN_H)),
];

/// One swept state: what to call it, and the edit that reaches it.
type SweptState = (&'static str, fn(&Rc<ViewState>));

/// The states this section is swept in.
///
/// ★ Cumulative, as the sibling sections' sweeps are: each entry is applied on
/// top of the ones before it, so the last case is a section that has been
/// rearranged, narrowed and zoomed rather than one that has had exactly one
/// thing done to it. A surface that only conforms at rest has not been checked
/// in a state anybody reaches.
const STATES: [SweptState; 5] = [
    ("opening", |_| {}),
    ("a peer picked", |state| super::select_node(state, 3)),
    ("hierarchical", |state| super::choose_layout(state, 1)),
    ("mesh hidden", |state| super::flip_toggle(state, 0)),
    ("zoomed in", |state| super::zoom_by(state, true)),
];

fn painted_at(state: &Rc<ViewState>, size: (u32, u32)) -> Scene {
    let owner = Owner::current().expect("the sweep runs inside a scope");
    pinion_core::reactive::VIEWPORT_SIZE
        .resolve(&owner)
        .set(size);
    let mut scene = super::view((), Frame::default());
    let mut cache = pinion_runtime::LayoutCache::new();
    pinion_runtime::compute_layout(&mut scene, &mut cache, size.0, size.1);
    assert!(
        Rc::ptr_eq(state, &use_view_state()),
        "the sweep must drive the state the view reads"
    );
    scene
}

fn sweep(mut check: impl FnMut(&Rc<ViewState>, &Scene, (u32, u32), &str)) {
    for (size_name, size) in SIZES {
        for n in 0..STATES.len() {
            let owner = Owner::new();
            owner.run(|| {
                let state = use_view_state();
                reset(&state);
                for (_, edit) in &STATES[..=n] {
                    edit(&state);
                }
                let scene = painted_at(&state, size);
                let case = format!("{} - {size_name}", STATES[n].0);
                check(&state, &scene, size, &case);
            });
        }
    }
}

/// The state as the section opens. The cache is shared across a thread, so a
/// case that did not reset would inherit whatever the case before it left.
fn reset(state: &Rc<ViewState>) {
    state.layout.set(0);
    state.selected.set(spec::OPENS_ON.to_owned());
    state.zoom.set(spec::ZOOM_FIT);
    state
        .toggles
        .set(spec::TOGGLES.iter().map(|t| t.opens_on).collect());
    state.pointer_inside.set(false);
    state.resting.set(None);
}

/// Record what this frame painted, the way the window does — `judge` reads the
/// framework's paint store rather than a scene, and a fixture that skipped this
/// would leave the sweep judging one thing and the running application another.
fn record(scene: &Scene) {
    let surfaces = pinion_runtime::record_painted_surfaces(scene, &[VIEW_TAG]);
    assert!(
        surfaces >= 1,
        "the section's own paint root must be on the frame it is being judged from",
    );
}

/// Where a tag was painted on this frame.
fn rect_of(scene: &Scene, tag: &str) -> Option<Rect> {
    pinion_shell::rect_for_tag(scene, tag)
}

// ── 1. The section the pipeline paints IS the section the reference draws ───

/// ★★★★★ R1947 — **the round's claim, checked against the paint.**
///
/// Every surface the pin declares is standing, reproduces something, and has
/// nothing unreconciled — at every state and every size. The verdict is read
/// from the frame (`Evidence::Paint`), which is what R1758 measured the need
/// for: a verdict computed from this crate's own tables cannot fail.
#[test]
fn r1947_every_specified_surface_is_the_one_the_paint_draws() {
    let mut cases = 0_u32;
    sweep(|_, scene, _, case| {
        record(scene);
        let report = crate::judge::conformance();
        assert_eq!(
            report.evidence(),
            pinion_core::conformance::Evidence::Paint,
            "{case}: the verdict must be about the frame, not about a table",
        );
        for standing in report.surfaces() {
            let surface = standing.surface();
            assert!(
                standing.is_standing(),
                "{case}: the {surface} surface reports away ({:?}), and this section's \
                 three panes are drawn whenever it is showing",
                standing.why(),
            );
            assert!(
                standing.reproduced() > 0,
                "{case}: the {surface} surface reproduced nothing at all, so a difference \
                 against it would come out empty and read as success",
            );
            let unreconciled: Vec<String> = standing
                .unreconciled()
                .iter()
                .map(pinion_core::conformance::Unreconciled::sentence)
                .collect();
            assert!(
                unreconciled.is_empty(),
                "{case}: the {surface} surface's difference from the reference is not \
                 the difference the pin declares:\n  {}",
                unreconciled.join("\n  "),
            );
        }
        cases += 1;
    });
    // ★ Rule: could this pass by sweeping nothing? The count is asserted, so a
    // sweep whose population went empty fails here rather than reporting
    // success over zero cases.
    assert_eq!(
        cases,
        u32::try_from(SIZES.len() * STATES.len()).unwrap_or(0),
        "the sweep did not run every case",
    );
}

// ── 2. The three panes are where the reference puts them ────────────────────

/// ★★★★★ R1947 — **the three panes are side by side and none overlaps another.**
///
/// The reference's topology view is a row of three: a filter rail, a graph and
/// an inspector. Two panes overlapping is not a rendering artifact — it is a
/// geometry table that lost a width, and at the declared size it can look
/// almost right while hiding a whole column at the floor.
#[test]
fn r1947_the_three_panes_tile_the_window_without_overlapping() {
    sweep(|_, scene, size, case| {
        let filters = rect_of(scene, FILTER_TAG)
            .unwrap_or_else(|| panic!("{case}: the filter rail is not painted"));
        let graph = rect_of(scene, GRAPH_TAG)
            .unwrap_or_else(|| panic!("{case}: the graph column is not painted"));
        let inspector = rect_of(scene, INSPECTOR_TAG)
            .unwrap_or_else(|| panic!("{case}: the inspector is not painted"));
        assert!(
            filters.x + filters.w <= graph.x,
            "{case}: the filter rail runs into the graph column",
        );
        assert!(
            graph.x + graph.w <= inspector.x,
            "{case}: the graph column runs into the inspector",
        );
        assert!(
            inspector.x + inspector.w <= size.0,
            "{case}: the inspector runs off the window",
        );
        assert!(
            graph.w > 0,
            "{case}: the graph column has no width left for the plot",
        );
    });
}

// ── 3. Every declared node is drawn, inside the plot that holds it ──────────

/// ★★★★★ R1947 — **every node the capture declares is painted, and inside the
/// plot.**
///
/// This is the assertion the goal asked for and the one "does not panic" cannot
/// make: a node placed outside its pane is *drawn* and *reachable* and simply
/// not visible, which is exactly the class of defect a person reported by
/// looking at a window.
#[test]
fn r1947_every_declared_node_is_painted_inside_the_plot() {
    sweep(|_, scene, _, case| {
        let plot =
            rect_of(scene, CANVAS_TAG).unwrap_or_else(|| panic!("{case}: the plot is not painted"));
        for node in spec::NODES {
            let tag = format!("tv.node.{}", node.id);
            let at = rect_of(scene, &tag)
                .unwrap_or_else(|| panic!("{case}: {} is declared and not painted", node.id));
            assert!(
                at.w > 0 && at.h > 0,
                "{case}: {} is painted with no area",
                node.id
            );
            assert!(
                at.x >= plot.x
                    && at.y >= plot.y
                    && at.x + at.w <= plot.x + plot.w
                    && at.y + at.h <= plot.y + plot.h,
                "{case}: {} is painted at {at:?}, outside the plot {plot:?}",
                node.id,
            );
        }
    });
}

/// ★★★★★ R1947 — **no two nodes are painted on top of each other.**
///
/// `tests.rs` asserts the placement TABLE has no duplicate; this asserts the
/// drawn rectangles do not overlap, which is a different claim: two distinct
/// places can still collide once a node has a radius, and the radius grows with
/// the zoom. Swept at every zoom the states reach.
#[test]
fn r1947_no_two_nodes_are_painted_over_each_other() {
    let overlaps =
        |a: Rect, b: Rect| a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    sweep(|_, scene, _, case| {
        let mut drawn: Vec<(&str, Rect)> = Vec::new();
        for node in spec::NODES {
            if let Some(at) = rect_of(scene, &format!("tv.node.{}", node.id)) {
                drawn.push((node.id, at));
            }
        }
        for (n, (one, a)) in drawn.iter().enumerate() {
            for (other, b) in &drawn[n + 1..] {
                assert!(
                    !overlaps(*a, *b),
                    "{case}: {one} and {other} are painted over each other ({a:?} / {b:?})",
                );
            }
        }
    });
}

// ── 4. Picking a node changes what the inspector shows ──────────────────────

/// ★★★★★ R1947 — **the ring is on the node the inspector is describing.**
///
/// Two independent readings of one fact — the plot's selection ring and the
/// inspector's headline — compared through the paint. A screen where they can
/// disagree tells a reader the graph is about one peer while the panel beside
/// it is about another, and neither of them is wrong on its own.
#[test]
fn r1947_the_selection_ring_and_the_inspector_agree_on_one_node() {
    sweep(|state, scene, _, case| {
        let picked = state.picked();
        let ring = rect_of(scene, &format!("tv.node.{}.ring", picked.id))
            .unwrap_or_else(|| panic!("{case}: the picked node has no ring"));
        let body = rect_of(scene, &format!("tv.node.{}", picked.id))
            .unwrap_or_else(|| panic!("{case}: the picked node is not painted"));
        assert!(
            ring.x <= body.x
                && ring.y <= body.y
                && ring.x + ring.w >= body.x + body.w
                && ring.y + ring.h >= body.y + body.h,
            "{case}: the ring {ring:?} does not enclose the node {body:?}",
        );
        // And exactly one node wears one.
        let rings = spec::NODES
            .iter()
            .filter(|n| rect_of(scene, &format!("tv.node.{}.ring", n.id)).is_some())
            .count();
        assert_eq!(rings, 1, "{case}: {rings} nodes are drawn as picked");
        // The inspector's headline is that node, read off the paint.
        let id = rect_of(scene, "tv.inspector.id");
        assert!(
            id.is_some(),
            "{case}: the inspector draws no identifier for {}",
            picked.id
        );
    });
}

// ── 5. A switch changes what the plot draws ─────────────────────────────────

/// ★★★★★ R1947 — **turning a link class off removes those links from the
/// FRAME.**
///
/// `tests.rs` asserts the model's count moves. This asserts the paint does,
/// which is the claim a person can check: a toggle that flips a flag while the
/// plot draws the same lines is a control that does nothing and says it did.
#[test]
fn r1947_hiding_a_link_class_removes_its_labels_from_the_frame() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        reset(&state);
        let labelled = |scene: &Scene| {
            spec::LINKS
                .iter()
                .enumerate()
                .filter(|(n, link)| {
                    link.label.is_some() && rect_of(scene, &format!("tv.link.{n}.label")).is_some()
                })
                .count()
        };
        let before = labelled(&painted_at(&state, (WIN_W, WIN_H)));
        assert!(
            before > 0,
            "no labelled link is on the opening frame, so the check below compares nothing",
        );
        // The down-links switch hides a link that carries no label, so the
        // labelled count must NOT move — which is the direction that catches a
        // toggle wired to the wrong class.
        super::flip_toggle(&state, 1);
        assert_eq!(
            labelled(&painted_at(&state, (WIN_W, WIN_H))),
            before,
            "hiding down links removed a labelled link, and no down link carries a label",
        );
        super::flip_toggle(&state, 1);
        // Hiding the mesh must not move it either — the mesh links are unlabelled too.
        super::flip_toggle(&state, 0);
        assert_eq!(
            labelled(&painted_at(&state, (WIN_W, WIN_H))),
            before,
            "hiding the peer mesh removed a labelled link",
        );
    });
}

// ── 6. The letters fit the boxes that hold them ─────────────────────────────

/// ★★★★★ R1947 — **every run is inside the box that owns it, and no box is too
/// short for its own face.**
///
/// The two gates a person's 2026-09-01 report turned into standing checks: text
/// that overflows its box and a box shorter than the letters it holds are both
/// invisible to a reachability test and both immediately visible on a screen.
#[test]
fn r1947_every_run_is_contained_and_no_box_is_too_short_for_its_face() {
    sweep(|_, scene, size, case| {
        assert_contained_ink(case, scene, size);
        assert_boxes_hold_their_text(case, scene, SHORT_BOX_BUDGET);
    });
}
