//! ★★★★★ R1730 — **the integration test: the section the pipeline paints,
//! against the specification somebody wrote down for it.**
//!
//! `tests.rs` next door asserts the model. Neither it nor anything else renders
//! anything, and R1653 measured what that costs: three consecutive rounds of
//! screen A shipped a painter that disagreed with its own hit test while every
//! model test passed, because every one of them asked the geometry *helper*
//! where a control was. The helper was right each time.
//!
//! So this module runs the real pipeline — `view()` then
//! [`pinion_runtime::compute_layout`], the same two stages the window runs
//! before handing a scene to the rasteriser — and asks the resulting scene
//! where things ended up, at every state in `STATES` and every size in `SIZES`.
//!
//! # What is new here, and it is the round's own claim
//!
//! The first check does not compare the painted scene with a list written in
//! this file. It reads the **three surfaces back out of the paint** — a
//! surface's parts are the tags under its stem, ordered by where they were
//! painted — and hands them to `pinion_core::conformance`, which compares them
//! with `docs/analyzer-keys-spec.json` in both directions and holds the
//! difference against that document's own ledger.
//!
//! That closes the chain the round is about. The specification is a separate
//! reviewed artifact; `crate::spec`'s tables are what the running screen holds;
//! the paint is what a person sees. Every link is checked, so "this section
//! reproduces the reference" is a statement about pixels rather than about a
//! constant somebody could edit to make the test pass.

use std::collections::{BTreeMap, BTreeSet};

use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::test_fixtures::screen_ink::{
    assert_boxes_hold_their_text, assert_contained_ink, stand_in_ink,
};

/// How many runs on this screen sit in a box too short for their own face.
///
/// ★ R1800 — a ratchet PIN, measured rather than wished at zero. The population
/// across this tree was measured at 289 of 290 runs on one screen: not a
/// backlog of slips but a convention that never consulted the face. Lowering
/// this is the repair, and `containment::line_rect` is how.
const SHORT_BOX_BUDGET: usize = 82;
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{Frame, Scene};

use super::{
    DETAIL_TAG, HEADER_TAG, Hit, LIST_TAG, ROOT_TAG, VIEW_TAG, ViewState, WIN_H, WIN_W, centre,
    detail_rect, list_rect, select_declaration, set_query, show_declarer, spec, use_view_state,
};

/// The filter box at rest, which is the posture every check here runs the
/// screen in: the caret and the selection are the field's own business, and a
/// test that varied them would be testing the framework's text field rather
/// than this screen.
const IDLE_FIELD: (TextFieldState, u32) = (TextFieldState::Idle, 0);

/// One swept state: what to call it, and the edit that reaches it.
type SweptState = (&'static str, fn(&std::rc::Rc<ViewState>));

/// The states the section is swept in.
///
/// They compose — state *n* is state *n-1* plus one edit — because that is what
/// a session with the tool looks like, and because a screen that only survives
/// being reset is not a screen anybody can use.
const STATES: &[SweptState] = &[
    ("as it opens", |_| {}),
    ("on the administrative declaration", |state| {
        select_declaration(state, 0);
    }),
    ("on the one that resolved to a number only", |state| {
        select_declaration(state, 4);
    }),
    ("filtered to the publishers", |state| {
        set_query(state, "direction in (declare publish)");
    }),
    ("filtered to nothing at all", |state| {
        set_query(state, "pattern in (nothing/matches/this)");
    }),
    ("unfiltered again, on the last declaration", |state| {
        set_query(state, "");
        select_declaration(state, spec::ROWS.len() - 1);
    }),
    ("having asked for the declarer", |state| {
        show_declarer(state);
    }),
];

/// The window sizes the section is swept at.
const SIZES: &[(&str, (u32, u32))] = &[
    ("at the size it opens in", (WIN_W, WIN_H)),
    ("maximised", (2494, 1531)),
    (
        "at the narrowest complete layout",
        (super::MIN_W, super::MIN_H),
    ),
];

/// Where every tag in the painted scene ended up, and every text run with it.
struct Painted {
    /// Tag -> the rectangle the layout pass gave it, window-absolute.
    tags: BTreeMap<String, Rect>,
    /// Every text run: its content, its rectangle, and the tag of its nearest
    /// tagged ancestor. Runs carry no tag of their own.
    runs: Vec<(String, Rect, Option<String>)>,
    /// Tags that are not on screen but that a scroll offset would bring into
    /// view. A row below the fold is not missing; the reader scrolls to it.
    reachable: BTreeSet<String>,
}

impl Painted {
    fn of(scene: &Scene, window: (u32, u32)) -> Self {
        let mut tags = BTreeMap::new();
        let mut runs = Vec::new();
        let mut reachable = BTreeSet::new();
        for out in pinion_core::reach::out_of_sight(scene, window, &mut stand_in_ink) {
            if let (Some(tag), pinion_core::reach::Reach::Scrollable { .. }) = (out.tag, &out.reach)
            {
                reachable.insert(tag);
            }
        }
        scene.for_each_node(&mut |visit| {
            let Some(rect) = visit.absolute_rect() else {
                return;
            };
            if let Some(tag) = visit.node.tag() {
                tags.entry(tag.to_owned()).or_insert(rect);
            }
            if let Scene::Text(text) = visit.node {
                // ★ Its OWN tag first. A run painted through
                // `pinion_widget_paint::run::text_run` carries the tag on the
                // text node itself, and a walk that only looked at ancestors
                // attributed every such run to the panel around it — which is
                // how the column-header check first came out empty rather than
                // wrong.
                let owner = visit
                    .node
                    .tag()
                    .or_else(|| visit.ancestors.iter().rev().find_map(|a| a.tag()))
                    .map(str::to_owned);
                runs.push((text.content.clone(), rect, owner));
            }
        });
        Self {
            tags,
            runs,
            reachable,
        }
    }

    fn present(&self, tag: &str) -> bool {
        self.tags.contains_key(tag) || self.reachable.contains(tag)
    }

    /// Every painted tag beginning with `stem`, which is how a family's size is
    /// counted without writing the members down.
    fn family(&self, stem: &str) -> Vec<&str> {
        self.tags
            .keys()
            .map(String::as_str)
            .filter(|t| t.starts_with(stem))
            .collect()
    }
}

/// Run the real pipeline at `size` and index what came out of it.
fn painted_at(state: &std::rc::Rc<ViewState>, size: (u32, u32)) -> (Painted, Scene) {
    let owner = Owner::current().expect("the sweep runs inside a scope");
    pinion_core::reactive::VIEWPORT_SIZE
        .resolve(&owner)
        .set(size);
    let mut scene = super::view(IDLE_FIELD, Frame::default());
    let mut cache = pinion_runtime::LayoutCache::new();
    pinion_runtime::compute_layout(&mut scene, &mut cache, size.0, size.1);
    let shot = Painted::of(&scene, size);
    assert!(
        std::rc::Rc::ptr_eq(state, &use_view_state()),
        "the sweep must drive the state the view reads"
    );
    (shot, scene)
}

/// Run `check` over every state at every size, naming the case in the message.
fn sweep(mut check: impl FnMut(&std::rc::Rc<ViewState>, &Painted, &Scene, (u32, u32), &str)) {
    for (size_name, size) in SIZES {
        for n in 0..STATES.len() {
            let owner = Owner::new();
            owner.run(|| {
                let state = use_view_state();
                for (_, edit) in &STATES[..=n] {
                    edit(&state);
                }
                let (shot, scene) = painted_at(&state, *size);
                let case = format!("{} — {size_name}", STATES[n].0);
                check(&state, &shot, &scene, *size, &case);
            });
        }
    }
}

/// ★★★★★ R1758 — record what this frame painted, the way the WINDOW does.
///
/// The judgment below is `crate::judge`'s, which reads the framework's paint
/// store rather than a scene — because that is the only evidence a
/// `conformance` hook has between frames. A fixture that skipped this would
/// leave the sweep judging one thing and the running application judging
/// another, which is two builds under one name; R1747 measured the shape and
/// `record_painted_surfaces` exists so a fixture can take the window's path.
///
/// **Both surfaces, in one call.** The filter box is an
/// [`External`](pinion_core::external::External) of its own — it owns focus and
/// takes keystrokes — so in a window it is a surface, and a mark goes to the
/// smallest surface containing it. Recording the host alone would file the
/// field's marks under the host and make the fixture read a surface the window
/// does not have.
fn record(scene: &Scene) {
    let surfaces = pinion_runtime::record_painted_surfaces(scene, &[VIEW_TAG, super::QUERY_TAG]);
    assert!(
        surfaces >= 1,
        "the section's own paint root must be on the frame it is being judged from",
    );
}

// ── 1. The section the pipeline paints IS the section the reference draws ───

/// ★★★★★ R1730 — **the round's claim, checked against the paint.**
///
/// For each of the three surfaces the specification fixes, the parts are read
/// back out of the painted scene and compared with
/// `docs/analyzer-keys-spec.json` in **both** directions: a part the reference
/// has and this build does not paint, and a part this build paints that no
/// specification declares, are both failures — as are order and title.
///
/// The assertion is *equality* with that document's declared remainder rather
/// than containment. Equality is what makes paying a divergence off fail too:
/// the gate then says "you built it, record it", which is the direction a floor
/// cannot see.
///
/// Swept across every state and every size, because a surface that only
/// conforms in the state it opens in has not been checked in the state anybody
/// works in — and because a part that vanishes when the window is dragged to
/// the floor is exactly the kind of divergence nobody notices.
///
/// ★★★★★ R1758 — and it is now **the same reading the wire publishes**, not a
/// second one. It was `painted_surface(scene, …)` here and this screen's own
/// tables on the wire, so the strong claim ran in `cargo test` and the running
/// application answered a weak one. `crate::judge::built` is asked here, over
/// marks recorded exactly as the window records them.
#[test]
fn r1730_every_specified_surface_is_the_one_the_paint_draws() {
    let doc = spec::document();
    // ★★★★★ R1770 — see the log section's gate of the same name: how many of
    // the swept sizes are the size the pin was written at, counted rather than
    // assumed, because a window constant and a surface extent are two things.
    let at_declared = std::cell::Cell::new(0_u32);
    sweep(|_, _, scene, _, case| {
        record(scene);
        let report = crate::judge::conformance();
        assert!(
            report.at().is_some(),
            "{case}: ★ R1770 — a verdict read from a frame names the extent it was \
             read at, and this one does not",
        );
        if report.read_where_written() {
            at_declared.set(at_declared.get() + 1);
        }
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
                 surfaces are drawn whenever it is showing",
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
                "{case}: the painted {surface} surface is not what \
                 docs/analyzer-keys-spec.json declares:\n  {}",
                unreconciled.join("\n  "),
            );
        }
        assert_eq!(
            report.surfaces().len(),
            doc.surfaces().count(),
            "{case}: every surface the pin fixes is in the verdict",
        );
    });
    assert!(
        at_declared.get() > 0,
        "★★★★★ R1770 — this sweep never painted at the extent \
         docs/analyzer-keys-spec.json says its canon was written against ({:?}), \
         so every case above judged a size the specification does not describe.",
        doc.written_at(),
    );
}

/// ★★ The specification itself is the reference's, rather than whatever this
/// build happens to hold.
///
/// Separate from the conformance check above on purpose: if the pin were
/// malformed or truncated, a difference against it could come out empty and
/// read as success. This asserts the thing being compared against is the right
/// size and shape first.
#[test]
fn r1730_the_specification_is_the_references_own_section() {
    let doc = spec::document();
    let columns = doc.canon("columns").expect("the pin fixes the columns");
    assert_eq!(
        columns
            .parts()
            .iter()
            .map(|p| p.key.as_ref())
            .collect::<Vec<_>>(),
        [
            "id",
            "pattern",
            "by",
            "direction",
            "matches",
            "rate",
            "status"
        ],
        "the specification is the reference's seven columns in the reference's order",
    );
    assert_eq!(
        doc.canon("detail").expect("the pin fixes the pane").len(),
        11,
        "the reference's record pane has eleven parts",
    );
    assert_eq!(
        doc.canon("header").expect("the pin fixes the header").len(),
        3,
        "the reference's section header has a name, a summary and a filter",
    );
}

// ── 2. What the columns are called is what is painted ───────────────────────

/// ★★★ The one surface whose titles the reference actually draws.
///
/// The conformance check above takes a part's title from this screen's tables,
/// because the reference draws no label for a record-pane fact or for the
/// header's summary. The column headers ARE drawn, so what the reader sees is
/// held against the specification directly — otherwise a painter could label
/// the fourth column anything at all and the difference would be invisible.
#[test]
fn r1730_a_column_header_reads_what_the_specification_calls_it() {
    let doc = spec::document();
    let columns = doc.canon("columns").expect("the pin fixes the columns");
    sweep(|_, shot, _, _, case| {
        for part in columns.parts() {
            let tag = format!("kp.column.{}", part.key);
            let painted: Vec<&str> = shot
                .runs
                .iter()
                .filter(|(_, _, owner)| owner.as_deref() == Some(tag.as_str()))
                .map(|(text, _, _)| text.as_str())
                .collect();
            assert_eq!(
                painted,
                [part.title.as_ref()],
                "{case}: the {} column header does not read what the specification calls it",
                part.key,
            );
        }
    });
}

// ── 3. Reachable: every painted control answers for itself ──────────────────

/// Every declaration the query kept answers its own press at the centre of the
/// rectangle it was painted in, and the record pane's action answers at the
/// centre of its.
///
/// The population comes from the paint, not from a list written here: a check
/// over a hand-written set of rows reads as coverage when it is a sample.
#[test]
fn r1730_every_painted_control_answers_for_itself() {
    sweep(|state, shot, _, _, case| {
        let mut pressed = 0;
        for tag in shot.family("kp.list.row.") {
            let Some(&rect) = shot.tags.get(tag) else {
                continue;
            };
            // A row scrolled out of the list is reachable rather than pressed:
            // the point at its centre is over whatever the list is showing
            // there instead.
            if !rect_inside(rect, list_rect()) {
                continue;
            }
            let (x, y) = centre(rect);
            assert_eq!(
                Hit::at(state, x, y),
                Hit::of_tag(tag),
                "{case}: pressing the middle of {tag} does not reach {tag}",
            );
            pressed += 1;
        }
        assert_eq!(
            pressed,
            state.kept().len().min(visible_rows()),
            "{case}: the rows pressed are not the rows the query kept and the list shows",
        );
        let action = shot
            .tags
            .get("kp.detail.declarer")
            .copied()
            .unwrap_or_else(|| panic!("{case}: the record pane's action is not painted"));
        let (x, y) = centre(action);
        assert_eq!(
            Hit::at(state, x, y),
            Hit::Declarer,
            "{case}: pressing the middle of the record pane's action does not reach it",
        );
    });
}

/// How many rows the list can show at the size it is being swept at.
fn visible_rows() -> usize {
    (list_rect().h / spec::ROW_H) as usize
}

const fn rect_inside(inner: Rect, outer: Rect) -> bool {
    inner.y >= outer.y && inner.y + inner.h <= outer.y + outer.h
}

// ── 4. Contained: nothing is painted outside the pane its address names ─────

/// ★★★★★ R1730 — **and the one place a mark does escape is measured, not
/// waved through.**
///
/// The plain `assert_contained_ink` is what a screen normally runs. This
/// screen cannot: on its first run the gate reported the filter's own text
/// **128 pixels past the right edge of the box holding it**, into the list's
/// column header, for a query of entirely ordinary length. It is not this
/// screen's defect — a single-line text field does not clip. The framework's
/// painter wraps a multi-line field's content in a viewport and keeps a flat
/// child list for a single-line one, so a value wider than the box is painted
/// straight over whatever is beside it, in every field in this tree.
///
/// Fixing it is its own round: what a single-line field actually needs is
/// horizontal scroll-into-view, which moves the caret rectangle, the click-to-
/// caret hit test and the IME anchor together, and reaches every consumer. See
/// the debt note this round opened.
///
/// So the exemption is a MEASUREMENT rather than a name: the escaping marks
/// must belong to a text entry this screen **declares** — derived from its own
/// extra externals, so a second field is covered the day it is added and a
/// mark escaping anything else is a failure — and they must escape to the
/// right only, which is what a value too long for its box does. A mark
/// escaping upward, or leftward, or out of an untagged box, is a different
/// defect and fails here.
#[test]
fn r1730_every_mark_lies_inside_the_region_its_address_names() {
    let entries: Vec<String> = Owner::new().run(|| {
        <super::KeyPatternView as pinion_core::WidgetCore>::create_extra_externals()
            .iter()
            .map(|extra| extra.tag.to_string())
            .collect()
    });
    assert!(
        !entries.is_empty(),
        "the exemption is derived from this screen's text entries and it declares none, \
         so the derivation would exempt everything",
    );
    sweep(|_, _, scene, size, case| {
        let (escaped, _) = pinion_core::test_fixtures::screen_ink::ink_escapes(scene, size);
        let (known, unknown): (Vec<_>, Vec<_>) = escaped
            .into_iter()
            .partition(|e| entries.iter().any(|tag| e.owner.starts_with(tag.as_str())));
        assert!(
            unknown.is_empty(),
            "{case}: {} painted mark(s) are outside the box that owns them — {:?}",
            unknown.len(),
            unknown
                .iter()
                .map(|e| (
                    e.content.clone().or_else(|| e.tag.clone()),
                    e.owner.clone(),
                    e.over
                ))
                .take(6)
                .collect::<Vec<_>>(),
        );
        for escape in &known {
            assert!(
                escape.over.left == 0 && escape.over.top == 0 && escape.over.bottom == 0,
                "{case}: the filter's content escapes somewhere other than past its right \
                 edge, which is a different defect from the one this exemption measures — {:?}",
                escape.over,
            );
        }
    });
}

/// The plain containment gate, run over the screen with **nothing in the
/// filter** — which is every state a person is in before they type.
///
/// Kept separate from the exemption above so the exemption cannot quietly
/// widen: this asserts that with the field empty the screen escapes its boxes
/// nowhere at all, so the one known escape is the field's *content* and not
/// something the field does by existing.
/// ★ R1800 — was each run's OWN box authored tall enough for its face?
///
/// A different question from every other check in this file, which ask whether
/// a mark left its PARENT. A pane can be roomy while the row inside it is three
/// pixels short of the line it holds, and that is what a reader sees as a cut
/// descender. Pure scene arithmetic, so it needs no font.
#[test]
fn r1800_no_run_sits_in_a_box_too_short_for_its_own_face() {
    let mut worst = 0;
    for (size_name, size) in SIZES {
        for (state_name, edit) in STATES {
            let owner = Owner::new();
            owner.run(|| {
                let state = use_view_state();
                edit(&state);
                let (_, scene) = painted_at(&state, *size);
                worst = worst.max(assert_boxes_hold_their_text(
                    &format!("{state_name} — {size_name}"),
                    &scene,
                    SHORT_BOX_BUDGET,
                ));
            });
        }
    }
    assert_eq!(
        worst, SHORT_BOX_BUDGET,
        "the budget is a PIN, not a ceiling: {worst} run(s) are short, so lower it"
    );
}

#[test]
fn r1730_with_nothing_typed_no_mark_escapes_at_all() {
    for (size_name, size) in SIZES {
        for (state_name, edit) in STATES {
            let owner = Owner::new();
            owner.run(|| {
                let state = use_view_state();
                edit(&state);
                set_query(&state, "");
                let (_, scene) = painted_at(&state, *size);
                assert_contained_ink(&format!("{state_name} — {size_name}"), &scene, *size);
            });
        }
    }
}

/// The record pane's parts are all inside the record pane, and the list's rows
/// are all inside the list.
///
/// The check `assert_contained_ink` cannot make: it holds a mark against the
/// region it was painted in, and a part painted in the wrong pane is inside
/// *that* pane perfectly well.
#[test]
fn r1730_each_surfaces_parts_are_inside_the_region_that_owns_them() {
    sweep(|_, shot, _, _, case| {
        for (stem, owner) in [
            ("kp.detail.", detail_rect()),
            ("kp.column.", super::colhead_rect()),
        ] {
            for tag in shot.family(stem) {
                let Some(&rect) = shot.tags.get(tag) else {
                    continue;
                };
                assert!(
                    rect.x >= owner.x && rect.x + rect.w <= owner.x + owner.w,
                    "{case}: {tag} is painted at {rect:?}, outside the {owner:?} that owns it",
                );
            }
        }
    });
}

// ── 5. Disjoint: no two declarations are painted on top of each other ───────

#[test]
fn r1730_no_two_declarations_share_a_band() {
    sweep(|_, shot, _, _, case| {
        let mut bands: Vec<(u32, u32, &str)> = shot
            .family("kp.list.row.")
            .into_iter()
            .filter_map(|tag| shot.tags.get(tag).map(|r| (r.y, r.h, tag)))
            .collect();
        bands.sort_unstable();
        for pair in bands.windows(2) {
            let (top, height, a) = pair[0];
            let (next, _, b) = pair[1];
            assert!(
                top + height <= next,
                "{case}: {a} and {b} overlap — {top}+{height} runs past {next}",
            );
        }
    });
}

// ── 6. Every addressable region is classified ───────────────────────────────

/// ★★★★★ R1730 — **every region this section paints either speaks or declares
/// why it is quiet.**
///
/// Written after the fact and the fact is the point: this gate did not exist,
/// the standalone screen's harness *reported* five undecided regions without
/// judging them, and what failed was a **sibling application's** demo — the
/// shell, at the seat this section was mounted at. A defect that only a
/// consumer's gate can see is a defect this screen cannot fix without being
/// mounted, which is the wrong way round.
///
/// The five were real: the section's heading and the pane's heading are the
/// names of the regions they sit in and would have been announced twice; the
/// filter's box and the column-header row place things and say nothing
/// themselves; and the declaration number is a fact a reader wants and was
/// reaching nobody.
///
/// Totality alone would be cheap — a census with every region declared silent
/// is total — so this also asserts a floor of regions that actually speak.
#[test]
fn r1730_every_region_either_speaks_or_says_why_it_is_quiet() {
    use pinion_a11y::WidgetA11y;
    use pinion_core::voice::Voice;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        let (_, scene) = painted_at(&state, (WIN_W, WIN_H));
        let mut nodes = super::KeyPatternView::access_node(&IDLE_FIELD, None);
        // The shell's own enrichment: a name may be `None` on the node and
        // resolved from the paint, so a gate reading the tree without this step
        // would be asking about a tree nobody receives.
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        let announced = pinion_a11y::announcements(&nodes);
        let referenced = pinion_a11y::referenced_tags(&nodes);
        let census = pinion_core::voice::voice_census(&scene, &announced, &referenced);
        let undecided: Vec<&str> = census
            .nodes
            .iter()
            .filter(|n| n.voice == Voice::Unvoiced)
            .map(|n| n.tag.as_str())
            .collect();
        assert!(
            undecided.is_empty(),
            "region(s) neither announced nor declared quiet: {undecided:?}",
        );
        for fault in [Voice::Mumbled, Voice::Hollow, Voice::Ghost] {
            let bad: Vec<&str> = census
                .nodes
                .iter()
                .filter(|n| n.voice == fault)
                .map(|n| n.tag.as_str())
                .collect();
            assert!(bad.is_empty(), "{fault:?} region(s): {bad:?}");
        }
        let speaks = census.count(Voice::Announced);
        assert!(
            speaks > census.count(Voice::Silent),
            "more of this section is declared quiet than speaks ({speaks} announced), \
             which is what a census made total by silencing everything looks like",
        );
    });
}

// ── 7. The screen's own root is where the host put it ───────────────────────

/// The root, the three regions and the receiver are all painted, at every size.
///
/// The smallest possible forward check, and it is here because the surfaces
/// above are read from the paint: a screen that painted *nothing* would have
/// three empty surfaces, and the emptiness assertion in the first check is what
/// catches that — this catches the case where the parts are painted and the
/// regions holding them are not.
#[test]
fn r1730_the_regions_that_hold_the_surfaces_are_painted() {
    sweep(|_, shot, _, _, case| {
        for tag in [ROOT_TAG, HEADER_TAG, LIST_TAG, DETAIL_TAG, "kp.colhead"] {
            assert!(shot.present(tag), "{case}: {tag} is not painted");
        }
        assert!(
            shot.tags.contains_key(VIEW_TAG) || shot.present(VIEW_TAG),
            "{case}: the receiver a press resolves to is not in the scene",
        );
    });
}
