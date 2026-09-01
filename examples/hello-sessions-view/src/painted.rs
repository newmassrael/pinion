//! ★★★★★ R1948 — **the integration test: the section the pipeline paints,
//! against the specification somebody wrote down for it.**
//!
//! `tests.rs` asserts the model — the population, the join with the topology
//! section, the derived count. This module runs the real pipeline and asks the
//! resulting scene where things ended up, at every state in [`STATES`] and
//! every size in [`SIZES`].
//!
//! ⚠ **This is the half that has to touch pixels.** "Did not panic" and "is
//! reachable" go green on every defect R1947's sweep caught, and this section
//! was written against that list — so what is asserted here is where marks
//! landed: a row inside the body that holds it, cells inside their columns, a
//! detail that describes the row the list marks as picked.

use std::rc::Rc;

use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::test_fixtures::screen_ink::{assert_boxes_hold_their_text, assert_contained_ink};
use pinion_core::{Frame, Scene};

use super::{
    DETAIL_TAG, LIST_TAG, ROWS_TAG, VIEW_TAG, ViewState, WIN_H, WIN_W, spec, use_view_state,
};

/// How many runs on this screen sit in a box too short for their own face.
///
/// R1800's ratchet. Zero from the first round here, because every run goes
/// through a helper that derives its height — R1947 measured what the other
/// convention costs (53 of 57).
const SHORT_BOX_BUDGET: usize = 0;

/// The sizes the section is swept at.
///
/// ⚠ The middle one is DERIVED as the midpoint rather than written. The first
/// draft picked `(1080, 640)` and the sweep failed there — not because the
/// screen was wrong but because that pair is BELOW the floor, which moved when
/// `min_width` and `min_height` stopped being guesses. A swept size that is
/// smaller than what the section declares it needs is not a test of the
/// section; it is a test of what happens outside its own contract.
const SIZES: [(&str, (u32, u32)); 3] = [
    ("declared", (WIN_W, WIN_H)),
    (
        "between",
        (
            u32::midpoint(super::MIN_W, WIN_W),
            u32::midpoint(super::MIN_H, WIN_H),
        ),
    ),
    ("floor", (super::MIN_W, super::MIN_H)),
];

/// One swept state: what to call it, and the edit that reaches it.
type SweptState = (&'static str, fn(&Rc<ViewState>));

/// The states this section is swept in, cumulative.
const STATES: [SweptState; 4] = [
    ("opening", |_| {}),
    ("a closed session picked", |state| {
        super::select_session(state, "S-06");
    }),
    ("kept to established", |state| super::choose_chip(state, 1)),
    ("kept to reconnecting", |state| super::choose_chip(state, 2)),
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

/// The state as the section opens.
fn reset(state: &Rc<ViewState>) {
    state.selected.set(spec::OPENS_ON.to_owned());
    state.chip.set(0);
    state.crossing.set(None);
    state.pointer_inside.set(false);
    state.resting.set(None);
}

/// Record what this frame painted, the way the window does.
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

/// ★★★★★ R1948 — **the round's claim, checked against the paint.**
#[test]
fn r1948_every_specified_surface_is_the_one_the_paint_draws() {
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
                "{case}: the {surface} surface reports away ({:?}), and both panes are \
                 drawn whenever this section is showing",
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
    assert_eq!(
        cases,
        u32::try_from(SIZES.len() * STATES.len()).unwrap_or(0),
        "the sweep did not run every case",
    );
}

// ── 2. The two panes are where the reference puts them ──────────────────────

/// ★★★★★ R1948 — **the list and the detail sit side by side, neither over the
/// other, and the detail never leaves the window.**
#[test]
fn r1948_the_two_panes_tile_the_window_without_overlapping() {
    sweep(|_, scene, size, case| {
        let list = rect_of(scene, LIST_TAG)
            .unwrap_or_else(|| panic!("{case}: the list pane is not painted"));
        let detail = rect_of(scene, DETAIL_TAG)
            .unwrap_or_else(|| panic!("{case}: the detail pane is not painted"));
        assert!(
            list.x + list.w <= detail.x,
            "{case}: the list runs into the detail ({list:?} / {detail:?})",
        );
        assert!(
            detail.x + detail.w <= size.0,
            "{case}: the detail runs off the window",
        );
        assert!(
            list.w > 0,
            "{case}: the list has no width left for its grid"
        );
    });
}

// ── 3. Every kept session is drawn, inside the body that holds it ───────────

/// ★★★★★ R1948 — **every session the chip keeps is painted inside the rows
/// body, and every session it drops is not painted at all.**
///
/// Both directions. A row drawn outside its body is reachable, announced, and
/// not on the screen; a row that survives a filter is a filter that does not
/// filter.
#[test]
fn r1948_every_kept_session_is_painted_inside_the_body_and_dropped_ones_are_not() {
    sweep(|state, scene, _, case| {
        let body = rect_of(scene, ROWS_TAG)
            .unwrap_or_else(|| panic!("{case}: the rows body is not painted"));
        let kept: Vec<&str> = state.kept().iter().map(|s| s.id).collect();
        for session in spec::SESSIONS {
            let at = rect_of(scene, &format!("sv.row.{}", session.id));
            if kept.contains(&session.id) {
                let at =
                    at.unwrap_or_else(|| panic!("{case}: {} is kept and not painted", session.id));
                assert!(
                    at.x >= body.x
                        && at.y >= body.y
                        && at.x + at.w <= body.x + body.w
                        && at.y + at.h <= body.y + body.h,
                    "{case}: {} is painted at {at:?}, outside the body {body:?}",
                    session.id,
                );
            } else {
                assert!(
                    at.is_none(),
                    "{case}: {} was filtered away and is still painted",
                    session.id,
                );
            }
        }
        assert!(!kept.is_empty(), "{case}: no session is kept at all");
    });
}

/// ★★★★★ R1948 — **no two rows are painted over each other.**
#[test]
fn r1948_no_two_rows_are_painted_over_each_other() {
    let overlaps =
        |a: Rect, b: Rect| a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    sweep(|_, scene, _, case| {
        let mut drawn: Vec<(&str, Rect)> = Vec::new();
        for session in spec::SESSIONS {
            if let Some(at) = rect_of(scene, &format!("sv.row.{}", session.id)) {
                drawn.push((session.id, at));
            }
        }
        for (n, (one, a)) in drawn.iter().enumerate() {
            for (other, b) in &drawn[n + 1..] {
                assert!(
                    !overlaps(*a, *b),
                    "{case}: {one} and {other} are painted over each other",
                );
            }
        }
    });
}

// ── 4. The grid's columns line up with their headings ───────────────────────

/// ★★★★★ R1948 — **every column heading is over the cells it heads.**
///
/// The property a grid's whole claim rests on, and the one a screenshot makes
/// look right while the geometry disagrees: the reference gives one track the
/// slack, so a heading and a cell that each resolved that slack separately
/// would drift apart only at some widths. Swept at three.
#[test]
fn r1948_every_column_heading_sits_over_its_own_cells() {
    sweep(|_, scene, _, case| {
        let strip = rect_of(scene, "sv.list.columns")
            .unwrap_or_else(|| panic!("{case}: the column headings are not painted"));
        let body = rect_of(scene, ROWS_TAG)
            .unwrap_or_else(|| panic!("{case}: the rows body is not painted"));
        assert!(
            strip.y + strip.h <= body.y,
            "{case}: the headings run into the rows",
        );
        // Every heading is inside the list, and the columns march left to
        // right without a gap that swallows one.
        let mut previous_right = strip.x;
        for (n, column) in spec::COLUMNS.iter().enumerate() {
            let cell = spec::column_rect(n, super::list_rect());
            assert!(
                cell.x >= previous_right.saturating_sub(1),
                "{case}: the {} column starts left of the one before it",
                column.key,
            );
            assert!(
                cell.x + cell.w <= strip.x + strip.w,
                "{case}: the {} column runs off the list pane",
                column.key,
            );
            assert!(cell.w > 0, "{case}: the {} column has no width", column.key);
            previous_right = cell.x + cell.w;
        }
    });
}

// ── 5. Picking a row changes what the detail shows ──────────────────────────

/// ★★★★★ R1948 — **the detail describes the row the list marks as picked.**
///
/// Two independent readings of one fact, compared through the paint: a screen
/// where they can disagree tells a reader the list is about one session while
/// the panel beside it is about another, and neither is wrong on its own.
#[test]
fn r1948_the_marked_row_and_the_detail_agree_on_one_session() {
    sweep(|state, scene, _, case| {
        let picked = state.picked();
        assert!(
            rect_of(scene, &format!("sv.row.{}", picked.id)).is_some(),
            "{case}: the detail describes {}, which the list does not draw",
            picked.id,
        );
        assert!(
            rect_of(scene, "sv.detail.id").is_some(),
            "{case}: the detail draws no identifier",
        );
        // The peer line names the peer of THAT session, read off the frame.
        let mut said: Vec<String> = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let pinion_core::Scene::Text(text) = visit.node
                && text.content.starts_with(super::PEER_LEAD)
            {
                said.push(text.content.clone());
            }
        });
        assert_eq!(
            said.len(),
            1,
            "{case}: {} peer lines — two would be two accounts of one fact",
            said.len(),
        );
        assert!(
            said[0].contains(picked.peer),
            "{case}: the peer line {:?} does not name {}",
            said[0],
            picked.peer,
        );
    });
}

// ── 6. Captions are bound to their boxes, not merely near them ──────────────

/// ★★★★★ R1948 — **no caption in this section is paired with its box by nothing
/// but where it landed.**
///
/// The shell's own ratchet (`r1812`) counts this across the assembled
/// application and moves in units of the whole tree, so a section that adds ten
/// adjacent pairs is a number a reader has to attribute by hand. This asserts
/// it HERE, where the population is one section's and the count is actionable:
/// R1948 spent three rounds of guessing at the shell's figure before measuring
/// it locally, and the local measurement found the sites in one run.
///
/// ⚠ Zero, not a budget. Every run in this section that lands in a box goes
/// through `bound_run`, so a new site that does not is a defect rather than a
/// backlog item — the distinction R1800's own ratchet had to be pinned above
/// because it inherited a convention, and this section never had one.
#[test]
fn r1948_no_caption_in_this_section_is_paired_by_where_it_landed() {
    sweep(|_, scene, _, case| {
        let survey = pinion_widget_paint::caption::Survey::of(scene);
        assert_eq!(
            survey.adjacent(),
            0,
            "{case}: {} of {} caption/box pair(s) are held together by geometry \
             alone — `bound_line` and `captioned_box` are what make the pairing \
             a fact of the scene",
            survey.adjacent(),
            survey.pairs(),
        );
        assert!(
            survey.pairs() > 0,
            "{case}: no caption/box pair at all, so the assertion above judges \
             an empty population",
        );
    });
}

// ── 7. The letters fit the boxes that hold them ─────────────────────────────

/// ★★★★★ R1948 — **every run is inside the box that owns it, and no box is too
/// short for its own face.**
#[test]
fn r1948_every_run_is_contained_and_no_box_is_too_short_for_its_face() {
    sweep(|_, scene, size, case| {
        assert_contained_ink(case, scene, size);
        assert_boxes_hold_their_text(case, scene, SHORT_BOX_BUDGET);
    });
}
