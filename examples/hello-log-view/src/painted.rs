//! ★★★★★ R1731 — **the integration test: the section the pipeline paints,
//! against the specification somebody wrote down for it.**
//!
//! `tests.rs` next door asserts the model. This module runs the real pipeline —
//! `view()` then [`pinion_runtime::compute_layout`], the same two stages the
//! window runs before handing a scene to the rasteriser — and asks the
//! resulting scene where things ended up, at every state in `STATES` and every
//! size in `SIZES`.
//!
//! # What is different from the sibling section's peer, and it is the point
//!
//! Nothing here reads a surface out of the paint by hand. R1730 wrote that walk
//! and its reading-order rule inside the first screen that needed one; this is
//! the second, and it calls
//! [`painted_surface`](pinion_core::test_fixtures::surface::painted_surface).
//! Two screens reading a roster differently would disagree about the same
//! defect, and the one nobody ran would be the one that was wrong.

use std::collections::{BTreeMap, BTreeSet};

use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::test_fixtures::screen_ink::{assert_contained_ink, stand_in_ink};
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{Frame, Scene};

use super::{
    DETAIL_TAG, HEADER_TAG, Hit, LIST_TAG, ROOT_TAG, VIEW_TAG, ViewState, WIN_H, WIN_W, centre,
    choose_severity, detail_rect, list_rect, select_event, set_capturing, set_query, spec,
    use_view_state,
};

/// The filter box at rest, which is the posture every check here runs in.
const IDLE_FIELD: (TextFieldState, u32) = (TextFieldState::Idle, 0);

/// One swept state: what to call it, and the edit that reaches it.
type SweptState = (&'static str, fn(&std::rc::Rc<ViewState>));

/// The states the section is swept in. They compose, because that is what a
/// session with the tool looks like.
const STATES: &[SweptState] = &[
    ("as it opens", |_| {}),
    ("on the event with no frame", |state| {
        select_event(state, 5);
    }),
    ("on the error that closed a session", |state| {
        select_event(state, 7);
    }),
    ("kept to warnings and worse", |state| {
        choose_severity(state, 1);
    }),
    ("kept to errors only", |state| {
        choose_severity(state, 2);
    }),
    ("with the capture paused", |state| {
        set_capturing(state, false);
    }),
    ("all severities again, filtered to the pushes", |state| {
        choose_severity(state, 0);
        set_query(state, "type in (Data)");
    }),
    ("filtered to nothing at all", |state| {
        set_query(state, "type in (Nothing)");
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
    tags: BTreeMap<String, Rect>,
    runs: Vec<(String, Rect, Option<String>)>,
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

    fn family(&self, stem: &str) -> Vec<&str> {
        self.tags
            .keys()
            .map(String::as_str)
            .filter(|t| t.starts_with(stem))
            .collect()
    }
}

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
/// See the key-pattern section's fixture of the same name for why: the
/// judgment below is `crate::judge`'s, which reads the framework's paint store
/// rather than a scene, and a fixture that skipped this would leave the sweep
/// judging one thing and the running application another. The filter box is an
/// [`External`](pinion_core::external::External) of its own, so both surfaces
/// go in one call — a mark belongs to the nearest surface that is its ancestor,
/// and that attribution is only right when every surface is named.
fn record(scene: &Scene) {
    let surfaces = pinion_runtime::record_painted_surfaces(scene, &[VIEW_TAG, super::QUERY_TAG]);
    assert!(
        surfaces >= 1,
        "the section's own paint root must be on the frame it is being judged from",
    );
}

// ── 1. The section the pipeline paints IS the section the reference draws ───

/// ★★★★★ R1731 — **the round's claim, checked against the paint.**
///
/// Swept across every state and every size. The severity states are the ones
/// worth the sweep: a surface that only conforms while everything is shown has
/// not been checked in the state a person reaches for when something is wrong.
///
/// ★★★★★ R1758 — and it is now **the same reading the wire publishes**. It was
/// `painted_surface(scene, …)` here and this screen's own tables on the wire,
/// so the strong claim ran in `cargo test` while the running application
/// answered a weak one.
#[test]
fn r1731_every_specified_surface_is_the_one_the_paint_draws() {
    let doc = spec::document();
    sweep(|_, _, scene, _, case| {
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
                 docs/analyzer-logs-spec.json declares:\n  {}",
                unreconciled.join("\n  "),
            );
        }
        assert_eq!(
            report.surfaces().len(),
            doc.surfaces().count(),
            "{case}: every surface the pin fixes is in the verdict",
        );
    });
}

/// ★★ The specification itself is the reference's, rather than whatever this
/// build happens to hold.
#[test]
fn r1731_the_specification_is_the_references_own_section() {
    let doc = spec::document();
    let columns = doc.canon("columns").expect("the pin fixes the columns");
    assert_eq!(
        columns
            .parts()
            .iter()
            .map(|p| p.key.as_ref())
            .collect::<Vec<_>>(),
        ["time", "severity", "source", "type", "message"],
        "the specification is the reference's five columns in the reference's order",
    );
    assert_eq!(
        doc.canon("detail").expect("the pin fixes the pane").len(),
        6,
        "the reference's decode pane has six parts",
    );
    assert_eq!(
        doc.canon("header").expect("the pin fixes the header").len(),
        4,
        "the reference's header has a name, a capture mark, a filter and a severity",
    );
}

// ── 2. What the columns are called is what is painted ───────────────────────

#[test]
fn r1731_a_column_header_reads_what_the_specification_calls_it() {
    let doc = spec::document();
    let columns = doc.canon("columns").expect("the pin fixes the columns");
    sweep(|_, shot, _, _, case| {
        for part in columns.parts() {
            let tag = format!("lv.column.{}", part.key);
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

#[test]
fn r1731_every_painted_control_answers_for_itself() {
    sweep(|state, shot, _, _, case| {
        let mut pressed = 0;
        for tag in shot.family("lv.list.row.") {
            let Some(&rect) = shot.tags.get(tag) else {
                continue;
            };
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
            "{case}: the rows pressed are not the rows the section is showing",
        );
        // ★ And the severity choice, which is the control this section has and
        // its sibling does not.
        for choice in spec::CHOICES {
            let tag = format!("lv.severity.{}", choice.key);
            let rect = shot
                .tags
                .get(tag.as_str())
                .copied()
                .unwrap_or_else(|| panic!("{case}: {tag} is not painted"));
            let (x, y) = centre(rect);
            assert_eq!(
                Hit::at(state, x, y),
                Hit::of_tag(&tag),
                "{case}: pressing the middle of {tag} does not reach it",
            );
        }
    });
}

fn visible_rows() -> usize {
    (list_rect().h / spec::ROW_H) as usize
}

const fn rect_inside(inner: Rect, outer: Rect) -> bool {
    inner.y >= outer.y && inner.y + inner.h <= outer.y + outer.h
}

// ── 4. Contained ────────────────────────────────────────────────────────────

/// The exemption is the sibling section's, derived the same way and for the
/// same measured reason: a single-line text field does not clip, so its content
/// escapes its own box to the right. Everything else must stay inside.
#[test]
fn r1731_every_mark_lies_inside_the_region_its_address_names() {
    let entries: Vec<String> = Owner::new().run(|| {
        <super::LogView as pinion_core::WidgetCore>::create_extra_externals()
            .iter()
            .map(|extra| extra.tag.to_string())
            .collect()
    });
    assert!(
        !entries.is_empty(),
        "the exemption is derived from this screen's text entries and it declares none",
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
                 edge — {:?}",
                escape.over,
            );
        }
    });
}

#[test]
fn r1731_with_nothing_typed_no_mark_escapes_at_all() {
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

/// Each surface's parts are inside the region that owns them.
#[test]
fn r1731_each_surfaces_parts_are_inside_the_region_that_owns_them() {
    sweep(|state, shot, _, _, case| {
        let _ = state;
        for (stem, owner) in [
            ("lv.detail.", detail_rect()),
            ("lv.column.", super::colhead_rect()),
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

// ── 5. Disjoint ─────────────────────────────────────────────────────────────

#[test]
fn r1731_no_two_events_share_a_band() {
    sweep(|_, shot, _, _, case| {
        let mut bands: Vec<(u32, u32, &str)> = shot
            .family("lv.list.row.")
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

/// ★★★★★ Every region this section paints either speaks or declares why it is
/// quiet.
///
/// The sibling section learned this the hard way — five undecided regions that
/// only a *consumer's* demo judged — so this screen carries the gate from its
/// first round rather than from its second.
#[test]
fn r1731_every_region_either_speaks_or_says_why_it_is_quiet() {
    use pinion_a11y::WidgetA11y;
    use pinion_core::voice::Voice;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        let (_, scene) = painted_at(&state, (WIN_W, WIN_H));
        let mut nodes = super::LogView::access_node(&IDLE_FIELD, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        let announced = pinion_a11y::announcements(&nodes);
        let referenced = pinion_a11y::referenced_tags(&nodes);
        let census = pinion_core::voice::voice_census(&scene, &announced, &referenced);
        for fault in [Voice::Unvoiced, Voice::Mumbled, Voice::Hollow, Voice::Ghost] {
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
            "more of this section is declared quiet than speaks ({speaks} announced)",
        );
    });
}

// ── 7. The regions that hold the surfaces ───────────────────────────────────

#[test]
fn r1731_the_regions_that_hold_the_surfaces_are_painted() {
    sweep(|_, shot, _, _, case| {
        for tag in [ROOT_TAG, HEADER_TAG, LIST_TAG, DETAIL_TAG, "lv.colhead"] {
            assert!(shot.present(tag), "{case}: {tag} is not painted");
        }
        assert!(
            shot.present(VIEW_TAG),
            "{case}: the receiver a press resolves to is not in the scene",
        );
    });
}
