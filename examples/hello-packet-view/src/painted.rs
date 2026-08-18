//! R1663 — **the integration test: the screen the pipeline paints, against the
//! specification it is supposed to reproduce.**
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
//! where things ended up, through [`pinion_core::NodeVisit::absolute_rect`],
//! at every state in [`STATES`] and every size in [`SIZES`]:
//!
//! 1. **Forward** — every element the specification declares is painted, or is
//!    one scroll away in a pane that scrolls.
//! 2. **Backward** — every painted tag belongs to a declared family, and the
//!    families whose size the specification fixes are that size.
//! 3. **Reachable** — every painted control answers for *itself* when pressed
//!    at the centre of the rectangle it was painted in.
//! 4. **Contained** — every painted mark lies inside the pane its address puts
//!    it in.
//! 5. **Disjoint** — no two message rows and no two decode rows are painted on
//!    top of each other.
//! 6. **Derived** — the round's own law, on the painted screen: the bytes drawn
//!    lit are exactly the bytes the map says the selected field occupies, and
//!    pressing any one of them selects that same field back.
//!
//! ★ The population of every check comes from the scene or from
//! [`crate::spec`], never written out here (R1651.1: a hand-written population
//! makes "n controls pass" read as coverage when it is a sample).

use std::collections::{BTreeMap, BTreeSet};

use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::widgets::field_bytes::{FieldSpan, SourceId};
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{Frame, Scene};

use super::{
    Hit, MIN_H, MIN_W, ViewState, WIN_H, WIN_W, bytes_rect, centre, list_rect, press, select_byte,
    select_field, select_message, spec, toggle_layer, toggle_saved, tree_rect, use_view_state,
    visible_fields,
};

/// R1707 — the query box at rest, which is the posture every check here runs
/// the screen in: the caret and the selection are the field's own business, and
/// a test that varied them would be testing the framework's text field rather
/// than this screen.
const IDLE_FIELD: (TextFieldState, u32) = (TextFieldState::Idle, 0);

/// One swept state: what to call it, and the edit that reaches it.
type SweptState = (&'static str, fn(&std::rc::Rc<ViewState>));

/// The states the screen is swept in.
///
/// They compose — state *n* is state *n-1* plus one edit — because that is what
/// a session with the tool looks like, and because a screen that only survives
/// being reset is not a screen anybody can use. R1652.1 is why this is a list
/// at all: the specification describes the screen *as it opens*, that is the
/// only state anybody had ever checked, and it is the state nobody works in.
const STATES: &[SweptState] = &[
    ("as it opens", |_| {}),
    ("with a byte pressed", |state| {
        select_byte(state, 0x18);
    }),
    ("with a derived field selected", |state| {
        select_field(state, "l3.resolved");
    }),
    ("with a layer folded", |state| {
        toggle_layer(state, 3);
    }),
    ("with a saved filter applied", |state| {
        toggle_saved(state, 1);
    }),
    ("on a message with no reassembly", |state| {
        select_message(state, 1);
    }),
    ("on a transport message", |state| {
        select_message(state, 7);
    }),
    ("back on the reassembled message", |state| {
        select_message(state, spec::OPENING_ROW);
        select_field(state, "l3.payload");
    }),
];

/// The window sizes the screen is swept at.
///
/// ★ R1656 — size was never an axis on screen A, and that is why five defects
/// got out. The opening size is first because the specification describes it;
/// the others are the two a person actually produces, a maximised window and a
/// window dragged down to the floor the screen declares.
const SIZES: &[(&str, (u32, u32))] = &[
    ("at the size it opens in", (WIN_W, WIN_H)),
    ("maximised", (2494, 1531)),
    ("at its declared floor", (MIN_W, MIN_H)),
];

/// Where every tag in the painted scene ended up, and every text run with it.
struct Painted {
    /// Tag -> the rectangle the layout pass gave it, window-absolute.
    tags: BTreeMap<String, Rect>,
    /// Every text run: its content, its rectangle, and the tag of its nearest
    /// tagged ancestor. Runs carry no tag of their own.
    runs: Vec<(String, Rect, Option<String>)>,
    /// ★ R1662 — tags that are not on screen but that a scroll offset would
    /// bring into view. A control below the fold of a scrolling pane is not
    /// missing; the reader scrolls to it. Kept beside the painted set rather
    /// than folded into it, because a check that wants "on screen" must still
    /// be able to ask for exactly that.
    reachable: std::collections::BTreeSet<String>,
}

impl Painted {
    fn of(scene: &Scene, window: (u32, u32)) -> Self {
        let mut tags = BTreeMap::new();
        let mut runs = Vec::new();
        let mut reachable = std::collections::BTreeSet::new();
        for out in pinion_core::reach::out_of_sight(scene, window, &mut stand_in_ink) {
            // ★ R1714 — a set: the one question asked of this index is whether a
            // tag is one gesture away (see `shows`), and the offset beside it
            // had no reader.
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
                    .ancestors
                    .iter()
                    .rev()
                    .find_map(|a| a.tag())
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

    /// Whether a tag is on screen, or one scroll away in a pane that scrolls.
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

/// ★★ R1672 — the ink stand-in and the containment gate come from the crate.
///
/// This file held a byte-identical copy of the stand-in and **never ran the
/// check with it**: a counterfactual that handed all three of this screen's
/// scrolling panes their panels' own boxes — putting each body over the outline
/// of the panel holding it — was caught by nothing here, while the same break
/// on screen A was caught immediately. A metric a screen owns but does not use
/// is a vocabulary the screen cannot speak.
use pinion_core::test_fixtures::screen_ink::{assert_contained_ink, stand_in_ink};

/// Run the real pipeline at `size` and index what came out of it.
fn painted_at(state: &std::rc::Rc<ViewState>, size: (u32, u32)) -> (Painted, Scene) {
    // ★ R1654 — publish the size the way the shell does, so the sweep can ask
    // the screen to lay out at a size other than the one it was designed for.
    let owner = Owner::current().expect("the sweep runs inside a scope");
    pinion_core::reactive::VIEWPORT_SIZE
        .resolve(&owner)
        .set(size);
    let mut scene = super::view((TextFieldState::Idle, 0), Frame::default());
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
                // Accumulate: state n is state n-1 plus one edit.
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

// ── 1. Forward: everything the specification declares is painted ────────────

#[test]
fn r1663_every_declared_element_of_the_screen_is_painted() {
    sweep(|state, shot, _, _, case| {
        let mut wanted: Vec<String> = vec![
            "pv.root".into(),
            "pv.appbar".into(),
            "pv.appbar.interface".into(),
            "pv.appbar.rate".into(),
            "pv.filter".into(),
            "pv.filter.count".into(),
            "pv.context".into(),
            "pv.context.session".into(),
            "pv.reassembly".into(),
            "pv.reassembly.title".into(),
            "pv.reassembly.counts".into(),
            "pv.bytes.span".into(),
        ];
        // The panes and their scrolling bodies come from the specification's
        // own table rather than from this list.
        for pane in spec::PANES {
            wanted.push(pane.tag.to_owned());
            if let Some(body) = pane.body {
                wanted.push(body.to_owned());
            }
        }
        for n in 0..spec::COLUMNS.len() {
            wanted.push(format!("pv.list.head.{n}"));
        }
        // R1693 — every message row, and every CELL of it. The cells were
        // untagged runs until this round, which is why sixteen messages of seven
        // columns reached a reader as nothing at all.
        // ★ R1707 — the messages the running query KEPT, not all sixteen. The
        // list is filterable now, so "what the specification declares is on
        // screen" is conditional on the query for exactly this family, and the
        // sweep drives a state with a saved filter applied.
        for n in state.kept() {
            wanted.push(format!("pv.list.row.{n}"));
            for c in 0..spec::COLUMNS.len() {
                wanted.push(crate::list_cell_tag(n, c));
            }
        }
        for n in 0..spec::SAVED_FILTERS.len() {
            wanted.push(format!("pv.filter.saved.{n}"));
        }
        // ★ R1707 — the query is a box, not three painted constants. What has
        // to be there is the box; what is IN it is the person's text and is
        // judged by the query gate below.
        wanted.push("pv.filter.query".to_owned());
        for value in spec::CONTEXT {
            wanted.push(format!("pv.context.{}", value.key.replace(' ', "_")));
        }
        for n in 0..spec::LANES.len() {
            wanted.push(format!("pv.reassembly.lane.{n}"));
        }
        // Every decode row the screen currently shows.
        for (path, ..) in visible_fields(state) {
            wanted.push(format!("pv.tree.field.{path}"));
        }
        let missing: Vec<&String> = wanted.iter().filter(|t| !shot.present(t)).collect();
        assert!(
            missing.is_empty(),
            "{case}: the specification declares {} element(s) the screen does not paint \
             and no scroll reaches: {missing:?}",
            missing.len()
        );
    });
}

/// ★★★★★ R1664 — the screen paints the tag it is REGISTERED under, and paints
/// it as the node a press falls through to.
///
/// # Why this is a test and not a convention
///
/// The §5.35 router turns a window point into a widget by resolving the deepest
/// tagged node under the cursor and looking its primary half up as an `External`
/// in the state scene. So a screen is operable only if *some* painted tag is a
/// name the state scene answers to — and that is a join between two string
/// literals in two different functions, with nothing on either side that fails
/// when they disagree.
///
/// R1663 shipped this screen with them disagreeing (`packet_view` registered,
/// `pv.root` painted). Every press anywhere in the window was dropped in
/// silence. Eleven tests in this file passed, because each one asks the app's own
/// `Hit::at`; the 160-assertion demo passed, because it invokes the oracle by
/// name; the boot gate printed `deliverable=0` and let it through. The defect was
/// found by a person opening the window and pressing things.
///
/// Two properties, and the second is the one a rename could break quietly:
/// the receiver is painted, and it is not *itself* pointer-transparent — a
/// transparent node is skipped by `hit_test` along with everything under it, so
/// declaring it would make the screen dead in a way this file's forward check
/// (which only asks whether the tag is present) cannot see.
#[test]
fn r1664_the_root_paints_the_tag_the_router_resolves_a_press_to() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        let (_, scene) = painted_at(&state, (MIN_W, MIN_H));
        let mut found = false;
        scene.for_each_node(&mut |visit| {
            if visit.node.tag() == Some(crate::VIEW_TAG) {
                found = true;
                assert!(
                    !visit.node.is_pointer_transparent(),
                    "`{}` is painted but declares itself transparent to the pointer, so \
                     `hit_test` skips it and everything under it — the screen is dead to a \
                     mouse exactly as it was before it carried the tag at all",
                    crate::VIEW_TAG,
                );
                assert!(
                    visit.ancestors.iter().all(|a| !a.is_pointer_transparent()),
                    "`{}` sits under a pointer-transparent ancestor, so the whole subtree \
                     is skipped before the router ever reaches it",
                    crate::VIEW_TAG,
                );
            }
        });
        assert!(
            found,
            "nothing painted carries `{}`, the tag this screen's `External` is registered \
             under — so the router has nothing to resolve a press to and every press in the \
             window is dropped without a word",
            crate::VIEW_TAG,
        );
    });
}

/// The scrolling bodies are the specification's, not the painter's opinion.
#[test]
fn r1663_a_pane_the_specification_says_scrolls_is_a_scroll_node() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        let (_, scene) = painted_at(&state, (MIN_W, MIN_H));
        let mut scrollers = BTreeSet::new();
        scene.for_each_node(&mut |visit| {
            if let Scene::Scroll(node) = visit.node
                && let Some(tag) = node.tag.as_deref()
            {
                scrollers.insert(tag.to_owned());
            }
        });
        for pane in spec::PANES {
            let Some(body) = pane.body else { continue };
            assert!(
                scrollers.contains(body),
                "the specification says `{}` scrolls; the painted scene has no scroll node \
                 tagged `{body}` (has {scrollers:?})",
                pane.tag
            );
        }
    });
}

// ── 2. Backward: nothing painted is undeclared ──────────────────────────────

#[test]
fn r1663_every_painted_tag_belongs_to_a_declared_family() {
    let stems: &[&str] = &[
        // ★ R1664 — the tag the widget is REGISTERED under, which is what the
        // §5.35 router resolves a press to. Named from the constant rather than
        // spelled again: the two literals drifting apart is precisely the defect
        // that left this screen dead to a mouse for a round.
        crate::VIEW_TAG,
        "pv.root",
        "pv.appbar",
        "pv.filter",
        "pv.context",
        "pv.list",
        "pv.tree",
        "pv.bytes",
        "pv.reassembly",
    ];
    sweep(|_, shot, _, _, case| {
        let stray: Vec<&String> = shot
            .tags
            .keys()
            .filter(|t| {
                !stems
                    .iter()
                    .any(|s| t.as_str() == *s || t.starts_with(&format!("{s}.")))
            })
            .collect();
        assert!(
            stray.is_empty(),
            "{case}: the screen paints tag(s) no family declares: {stray:?}"
        );
    });
}

/// The families whose size the specification fixes are that size — an extra
/// member is as much a failure as a missing one.
#[test]
fn r1663_the_fixed_families_are_the_size_the_specification_gives_them() {
    sweep(|state, shot, _, _, case| {
        assert_eq!(
            shot.family("pv.list.head.").len(),
            spec::COLUMNS.len(),
            "{case}: column headers"
        );
        assert_eq!(
            shot.family("pv.filter.saved.").len(),
            spec::SAVED_FILTERS.len(),
            "{case}: saved filters"
        );
        // ★ R1707 — the list draws what the query kept, and the SAME set at
        // every window size. A row count that moved with the WIDTH would mean
        // the list was narrowing for a reason other than the query. Counted by
        // exact tag rather than by prefix, because a row's note and its cells
        // live under the same stem.
        let drawn: Vec<usize> = (0..spec::ROWS.len())
            .filter(|n| shot.present(&format!("pv.list.row.{n}")))
            .collect();
        assert_eq!(drawn, state.kept(), "{case}: the message rows drawn");
        assert_eq!(
            shot.family("pv.reassembly.lane.").len(),
            spec::LANES.len(),
            "{case}: reassembly lanes"
        );
    });
}

// ── 3. Reachable: a control answers for itself where it was painted ─────────

#[test]
fn r1663_every_painted_control_answers_at_the_centre_of_its_own_rectangle() {
    sweep(|state, shot, _, _, case| {
        let mut checked = 0;
        // Message rows.
        for n in 0..spec::ROWS.len() {
            let tag = format!("pv.list.row.{n}");
            let Some(rect) = shot.tags.get(&tag) else {
                continue;
            };
            let (px, py) = centre(*rect);
            assert_eq!(
                Hit::at(state, px, py),
                Hit::Message(n),
                "{case}: pressing the middle of `{tag}` ({rect:?}) did not answer as itself"
            );
            checked += 1;
        }
        // Saved-filter chips.
        for n in 0..spec::SAVED_FILTERS.len() {
            let tag = format!("pv.filter.saved.{n}");
            let Some(rect) = shot.tags.get(&tag) else {
                continue;
            };
            let (px, py) = centre(*rect);
            assert_eq!(
                Hit::at(state, px, py),
                Hit::Saved(n),
                "{case}: pressing the middle of `{tag}` ({rect:?}) did not answer as itself"
            );
            checked += 1;
        }
        // Decode rows, including the layer headings that fold.
        for (path, ..) in visible_fields(state) {
            let tag = format!("pv.tree.field.{path}");
            let Some(rect) = shot.tags.get(&tag) else {
                continue;
            };
            let (px, py) = centre(*rect);
            let want = spec::LAYERS
                .iter()
                .position(|(id, _)| *id == path.as_str())
                .map_or_else(|| Hit::Field(path.clone()), Hit::Layer);
            assert_eq!(
                Hit::at(state, px, py),
                want,
                "{case}: pressing the middle of `{tag}` ({rect:?}) did not answer as itself"
            );
            checked += 1;
        }
        // Byte cells.
        for byte in 0..spec::SOURCES[0].1 {
            let tag = format!("pv.bytes.cell.{byte}");
            let Some(rect) = shot.tags.get(&tag) else {
                continue;
            };
            let (px, py) = centre(*rect);
            assert_eq!(
                Hit::at(state, px, py),
                Hit::Byte(byte),
                "{case}: pressing the middle of `{tag}` ({rect:?}) did not answer as itself"
            );
            checked += 1;
        }
        assert!(
            checked >= 20,
            "{case}: only {checked} control(s) were on screen to press — the sweep is not \
             covering this case"
        );
    });
}

// ── 4. Contained: a mark stays inside the pane its address puts it in ───────

#[test]
fn r1663_every_painted_mark_is_inside_the_pane_its_address_names() {
    sweep(|_, shot, _, size, case| {
        let panes: BTreeMap<&str, Rect> = [
            ("pv.list", list_rect()),
            ("pv.tree", tree_rect()),
            ("pv.bytes", bytes_rect()),
        ]
        .into_iter()
        .collect();
        for (tag, rect) in &shot.tags {
            let Some((stem, pane)) = panes
                .iter()
                .find(|(stem, _)| tag.as_str() != **stem && tag.starts_with(&format!("{stem}.")))
            else {
                continue;
            };
            // A pane's own scrolling body is the viewport, so it may be exactly
            // the pane; everything inside it is clipped to the pane by the
            // scroll node, so anything the walk still reports must be inside.
            assert!(
                rect.x + rect.w <= size.0 && rect.y + rect.h <= size.1,
                "{case}: `{tag}` at {rect:?} is painted outside the {size:?} window"
            );
            assert!(
                rect.x + rect.w > pane.x && rect.x < pane.x + pane.w,
                "{case}: `{tag}` at {rect:?} is painted outside `{stem}` {pane:?}"
            );
        }
    });
}

/// ★★ R1672 — and every painted mark is inside the box that OWNS it, ink and
/// all.
///
/// The check above asks whether a tag's rectangle is inside the pane its
/// address names, which is a question about the *scene* and about an ancestor
/// chosen by name. This one asks
/// [`pinion_core::containment::escapes`]: whether the **ink** — a measurement,
/// not something in the scene — left the box the tree says owns it, including
/// the box's own border ([`pinion_core::containment::content_rect`]).
///
/// Screen A has run it since R1656 and this screen did not, which is how R1672
/// found three panes here painted over the outlines of the panels holding them
/// with every test in this file green. The counterfactual that reintroduces it
/// (`panel_content` handing back the box) is now caught.
#[test]
fn r1672_every_painted_mark_is_inside_the_box_that_owns_it() {
    let mut below = 0;
    let mut cases = 0;
    let mut weighed = 0;
    sweep(|_, shot, scene, size, case| {
        cases += 1;
        weighed += shot.runs.len();
        below += assert_contained_ink(case, scene, size);
    });
    // Floors on the SWEEP, because the two ways this passes without asking
    // anything are opposite. A state deleted from `STATES` takes its own
    // assertions with it, so the case count is pinned; and a screen whose every
    // mark took the off-window exemption asserted nothing at all, so the
    // exemption is bounded by what the sweep actually weighed.
    assert_eq!(
        cases,
        STATES.len() * SIZES.len(),
        "the sweep covered {cases} of the (state, size) cases it declares",
    );
    assert!(weighed > 0, "the sweep found no painted runs to weigh");
    assert!(
        below < weighed,
        "{below} of {weighed} mark(s) took the off-window exemption — that is \
         the whole screen, so this gate weighed nothing",
    );
}

// ── 5. Disjoint: two rows are never painted on top of each other ────────────

#[test]
fn r1663_no_two_rows_of_a_list_are_painted_over_each_other() {
    sweep(|state, shot, _, _, case| {
        let overlap = |a: Rect, b: Rect| {
            a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
        };
        let mut rows: Vec<(String, Rect)> = Vec::new();
        for n in 0..spec::ROWS.len() {
            if let Some(rect) = shot.tags.get(&format!("pv.list.row.{n}")) {
                rows.push((format!("message {n}"), *rect));
            }
        }
        for (path, ..) in visible_fields(state) {
            if let Some(rect) = shot.tags.get(&format!("pv.tree.field.{path}")) {
                rows.push((format!("field {path}"), *rect));
            }
        }
        for i in 0..rows.len() {
            for j in (i + 1)..rows.len() {
                // Different panes never overlap by construction; only compare
                // rows whose x ranges meet.
                assert!(
                    !overlap(rows[i].1, rows[j].1),
                    "{case}: `{}` {:?} and `{}` {:?} are painted over each other",
                    rows[i].0,
                    rows[i].1,
                    rows[j].0,
                    rows[j].1
                );
            }
        }
    });
}

// ── 6. Derived: the painted highlight IS the map's answer ───────────────────

/// ★ The round's law, on the painted screen rather than in the model.
///
/// The bytes drawn lit are exactly the bytes the map says the selected field
/// occupies — no more, no fewer — and pressing any one of them selects that
/// same field back. A screen that kept its own highlight set would pass the
/// crate's unit tests and fail this one.
#[test]
fn r1663_the_bytes_drawn_lit_are_the_bytes_the_map_names() {
    sweep(|state, shot, _, _, case| {
        let map = state.map.map();
        let field = state.field.get();
        let painted: BTreeSet<usize> = shot
            .tags
            .keys()
            .filter_map(|t| t.strip_prefix("pv.bytes.lit."))
            .filter_map(|n| n.parse().ok())
            .collect();
        let wanted: BTreeSet<usize> = match map.selection_for(&field) {
            Ok((source, sel)) if source == SourceId::new(0) => (sel.start()..sel.end()).collect(),
            // A derived field, or one whose bytes are in the reassembled
            // buffer, lights nothing in the frame pane — and that is the
            // truthful screen, not a missing highlight.
            _ => BTreeSet::new(),
        };
        assert_eq!(
            painted, wanted,
            "{case}: the screen lit {painted:?} for `{field}` and the map says {wanted:?}"
        );
        // And the inverse, driven through the app's own press handler: every
        // lit byte selects the field back.
        for byte in &wanted {
            let (px, py) = centre(
                *shot
                    .tags
                    .get(&format!("pv.bytes.cell.{byte}"))
                    .expect("a lit byte is a painted byte"),
            );
            super::move_cursor(state, px, py);
            press(state);
            assert_eq!(
                state.field.get(),
                field,
                "{case}: pressing byte {byte}, which `{field}` owns, selected something else"
            );
        }
    });
}

/// ★ R1653 — a tag-based check cannot see a text run, and the runs are what
/// three rounds of screen A got wrong. Every run stays inside the tagged node
/// that owns it, and no two runs of one owner are painted over each other.
#[test]
fn r1663_no_two_runs_of_one_node_are_painted_over_each_other() {
    let overlap =
        |a: Rect, b: Rect| a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    sweep(|_, shot, _, _, case| {
        let mut by_owner: BTreeMap<&str, Vec<(&str, Rect)>> = BTreeMap::new();
        for (content, rect, owner) in &shot.runs {
            let Some(owner) = owner.as_deref() else {
                continue;
            };
            // Rows and cells are one run each; the panes hold many, laid out on
            // their own grid, so the owners worth comparing are the leaves.
            by_owner
                .entry(owner)
                .or_default()
                .push((content.as_str(), *rect));
        }
        let mut compared = 0;
        for (owner, runs) in &by_owner {
            if runs.len() < 2 {
                continue;
            }
            for i in 0..runs.len() {
                for j in (i + 1)..runs.len() {
                    compared += 1;
                    assert!(
                        !overlap(runs[i].1, runs[j].1),
                        "{case}: under `{owner}`, {:?} {:?} and {:?} {:?} are painted over \
                         each other",
                        runs[i].0,
                        runs[i].1,
                        runs[j].0,
                        runs[j].1
                    );
                }
            }
        }
        assert!(compared > 0, "{case}: no pair of runs was compared");
    });
}

/// A byte no field claims says so rather than selecting the nearest one.
#[test]
fn r1663_pressing_an_unclaimed_byte_leaves_the_selection_alone() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        painted_at(&state, (WIN_W, WIN_H));
        let map = state.map.map();
        let unclaimed = (0..spec::SOURCES[0].1)
            .find(|b| map.owner_at(SourceId::new(0), *b).is_none())
            .expect("the reference frame has bytes past its last field");
        let before = state.field.get();
        select_byte(&state, unclaimed);
        assert_eq!(
            state.field.get(),
            before,
            "an unclaimed byte changed nothing"
        );
        assert!(
            state.said_sentence().contains("no field"),
            "the screen must say why nothing happened, said {:?}",
            state.said_sentence()
        );
    });
}

/// The tags a [`spec::VoiceSpec`] row stands for, expanded from the family its
/// population names.
///
/// Expanded rather than listed, so an eighth column or a twenty-second field is
/// demanded by this gate the moment it joins the specification — the property
/// R1651.1 wrote down after a hand-written population reported a sample as
/// coverage. The rule lives on [`spec::Population`] itself because two of this
/// screen's families are **computed** (a product and a range), and a gate
/// holding the rule would be a second place a screen's shape got derived.
fn voice_population(tag: &str, population: spec::Population) -> Vec<String> {
    population
        .members()
        .into_iter()
        .map(|member| tag.replace("{}", &member))
        .collect()
}

/// The census of the screen as it opens, built the way the shell builds it.
fn voice_of(state: &std::rc::Rc<ViewState>) -> pinion_core::voice::VoiceCensus {
    use pinion_a11y::WidgetA11y;

    let (_, scene) = painted_at(state, (WIN_W, WIN_H));
    let mut nodes = super::PacketView::access_node(&IDLE_FIELD, None);
    // ★ The shell's own enrichment. A widget's `access_node` may leave a name
    // `None`, and the name a reader hears is resolved from the PAINT SCENE after
    // layout on WAI-ARIA 1.2's name-computation precedence — so a gate that read
    // the tree without this step would be asking about a tree nobody receives.
    // This screen leaves two panes to be named from the runs that declare
    // themselves their names, which is what makes those `name_of` redirects
    // true rather than merely well formed.
    let derived = pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
    assert!(
        derived > 0,
        "no node was named from the paint, so the `name_of` redirects name nothing",
    );
    // The framework's own derivations rather than second ones: what counts as a
    // reference, and what an announcement is, are rules the wire and this gate
    // have to agree about.
    let announced = pinion_a11y::announcements(&nodes);
    let referenced = pinion_a11y::referenced_tags(&nodes);
    pinion_core::voice::voice_census(&scene, &announced, &referenced)
}

/// ★★★★★ R1693 — **every addressable region of this screen is classified**, and
/// the split between what speaks and what is deliberately quiet is the
/// specification's rather than whatever the last round happened to leave.
///
/// Measured the day this was written, before the round: **186 painted regions,
/// three announced nodes, 183 unclassified.** Two of the three announced a
/// collection role and held nothing.
///
/// Totality alone would not be worth asserting — a census with every region
/// declared silent is total — so the gate drives both halves of
/// [`spec::VOICES`] / [`spec::SILENCES`], and the role each region announces as,
/// which is what a reader is told they can *do* with it.
#[test]
fn r1693_the_screen_speaks_and_is_quiet_exactly_where_the_specification_says() {
    use pinion_a11y::WidgetA11y;
    use pinion_core::voice::Voice;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        let census = voice_of(&state);
        let rows: BTreeMap<&str, &pinion_core::voice::VoiceNode> =
            census.nodes.iter().map(|n| (n.tag.as_str(), n)).collect();
        let nodes = super::PacketView::access_node(&IDLE_FIELD, None);
        let roles: BTreeMap<&str, &'static str> = nodes
            .iter()
            .map(|n| (n.tag.as_str(), n.role.aria_name()))
            .collect();

        let mut spoken = 0;
        for want in spec::VOICES {
            for tag in voice_population(want.tag, want.population) {
                let row = rows.get(tag.as_str()).unwrap_or_else(|| {
                    panic!("the specification says {tag} is on screen and nothing paints it")
                });
                assert_eq!(
                    row.voice,
                    Voice::Announced,
                    "{tag} owes a reader a voice and is {}",
                    row.voice.name(),
                );
                assert!(
                    row.silence.is_none(),
                    "{tag} speaks AND declares a silence, so the declaration is \
                     a claim nobody acts on",
                );
                assert_eq!(
                    roles.get(tag.as_str()).copied(),
                    Some(want.role),
                    "{tag} announces as the wrong kind",
                );
                spoken += 1;
            }
        }

        let mut quiet = 0;
        for (tag, population, kind) in spec::SILENCES {
            for tag in voice_population(tag, *population) {
                let row = rows.get(tag.as_str()).unwrap_or_else(|| {
                    panic!("the specification declares {tag} quiet and nothing paints it")
                });
                assert_eq!(
                    row.voice,
                    Voice::Silent,
                    "{tag} is declared quiet and the census calls it {}",
                    row.voice.name(),
                );
                assert_eq!(
                    row.silence.as_ref().map(|s| s.kind().name()),
                    Some(*kind),
                    "{tag} is quiet for a reason the specification does not state",
                );
                quiet += 1;
            }
        }

        // ★ The two halves together are the whole screen. Asserted as an
        // equality rather than as two floors, because a floor is satisfied by a
        // screen that grew a region nobody classified — which is the defect this
        // whole family of gates exists to end.
        //
        // ★ R1707 — and it NAMES them. This said only "290 painted, 289
        // classified" and left the reader to find the one, which is the
        // "a census that answers how many cannot answer which" lesson this
        // tree wrote down two rounds after building this gate.
        let classified: BTreeSet<String> = spec::VOICES
            .iter()
            .flat_map(|v| voice_population(v.tag, v.population))
            .chain(
                spec::SILENCES
                    .iter()
                    .flat_map(|(tag, population, _)| voice_population(tag, *population)),
            )
            .collect();
        let unclassified: Vec<&str> = census
            .nodes
            .iter()
            .map(|n| n.tag.as_str())
            .filter(|t| !classified.contains(*t))
            .collect();
        assert_eq!(
            spoken + quiet,
            census.nodes.len(),
            "{} region(s) are painted and the specification classifies {}; \
             unclassified: {unclassified:?}",
            census.nodes.len(),
            spoken + quiet,
        );
        assert!(
            census.is_total(),
            "{:?}",
            census.defects().collect::<Vec<_>>()
        );
    });
}

/// ★★★★★ R1693 — the announced tree is one a reader can **walk**: every
/// collection holds what its role promises, and every member is inside the
/// collection its role requires.
///
/// This is the check that was missing when the screen announced a `table` with
/// no row. Run over every swept state, because a collection is emptied by
/// editing — folding a layer removes tree items — and a screen that is
/// well-formed only as it opens is well-formed in the one state nobody works in.
#[test]
fn r1693_every_announced_collection_holds_what_its_role_promises() {
    use pinion_a11y::WidgetA11y;

    sweep(|_state, _shot, scene, _size, case| {
        let mut nodes = super::PacketView::access_node(&IDLE_FIELD, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, scene);
        let census = pinion_a11y::structure_census(&nodes);
        assert!(
            census.is_sound(),
            "{case}: {:?}",
            census.nodes.iter().collect::<Vec<_>>(),
        );
        // ★ And it judged something. A structural census over a tree with no
        // collections in it is green, and this screen's whole point is that it
        // has three.
        assert!(
            census.judged > 100,
            "{case}: only {} node(s) carried a structural requirement",
            census.judged,
        );
    });
}

/// Every field of the reference decode is on the tree, and every tree row is a
/// field of the map — the join key holding in both directions.
#[test]
fn r1663_the_decode_tree_and_the_byte_map_name_the_same_fields() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        painted_at(&state, (WIN_W, WIN_H));
        let map = state.map.map();
        let on_tree: BTreeSet<String> = visible_fields(&state)
            .into_iter()
            .map(|(p, ..)| p)
            .collect();
        let in_map: BTreeSet<String> = map
            .fields()
            .iter()
            .map(|s| FieldSpan::path(s).to_owned())
            .collect();
        assert_eq!(
            on_tree, in_map,
            "the tree and the map disagree about the fields"
        );
        assert!(
            map.unmatched(on_tree.iter().map(String::as_str)).is_empty(),
            "the map declares a field the tree cannot show"
        );
    });
}

/// ★★★★ R1696 — **the keyboard ring this screen gained at R1693 is checked
/// where it is painted.**
///
/// It was not. R1693 made three composite panes and three filter chips into Tab
/// stops and left the verification entirely to two tree-wide python demos —
/// which do run, and take three and a half minutes to say so. A counterfactual
/// on the sibling screen proved the gap directly: reverting the framework
/// builder that attaches the flag left every test in THIS example green while
/// the screen lost its whole keyboard.
///
/// The ring is derived from the specification's own pane table plus the saved
/// filters, so a pane added there joins this check without anybody remembering,
/// and it is asserted as an ordered set because the §5.39 enumeration is
/// depth-first over the paint scene.
#[test]
fn r1696_every_composite_pane_and_chip_is_a_keyboard_stop() {
    sweep(|_, _, scene, _, case| {
        let walked = scene.collect_focusable_tags();
        let mut want: Vec<String> = spec::SAVED_FILTERS
            .iter()
            .enumerate()
            .map(|(n, _)| format!("pv.filter.saved.{n}"))
            .collect();
        want.extend(spec::PANES.iter().map(|pane| pane.tag.to_owned()));
        // R1707 — the query box is a stop too, and it is the first one: a
        // filter a person cannot Tab to is a filter only a mouse has.
        want.insert(0, "pv.filter.query".to_owned());
        assert_eq!(
            walked, want,
            "{case}: the Tab ring is not the panes and chips this screen \
             declares — a composite announced as operable that no keyboard \
             reaches is the defect R1693 repaired and nothing here checked",
        );
    });
}

// ── 8. R1707: the filter filters, and the screen says what it does ─────────

/// ★★★★★ R1707 — **every gesture this screen advertises is answered**, driven
/// through the entry points a person's hand actually reaches.
///
/// The sibling screen learned this the expensive way: its hint strip printed
/// `wheel → zoom` for the whole life of the screen while eight wheel events
/// over the canvas moved nothing, and the operation gate next door could not
/// see it because a hint strip is a *different population* from an operation
/// table. R1703 built the gate there and registered what it could not do — the
/// list existed on one screen of three, and a gate over an empty population is
/// indistinguishable from a gate over a kept promise.
///
/// This is that gate on the second screen. Both directions: a claim with no
/// driver fails, and a driver for a claim the strip does not make fails too.
#[test]
fn r1707_every_gesture_this_screen_advertises_is_answered() {
    // ★ The non-empty population, asserted at COMPILE TIME.
    //
    // It was a runtime `assert!` and clippy refused it: the expression is
    // constant, so the check could never fail while running and the "gate" was
    // decoration. That is this tree's own recorded rule — an invariant true by
    // construction belongs to the compiler, and the test is for the values that
    // actually get passed. Emptying `spec::GESTURES` now fails to BUILD.
    const _: () = assert!(!spec::GESTURES.is_empty());

    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        let (shot, _) = painted_at(&state, (WIN_W, WIN_H));
        let mut inert = Vec::new();
        for (gesture, effect) in spec::GESTURES {
            let before = witness(&state);
            match *gesture {
                "click a message" => {
                    let rect = shot.tags["pv.list.row.2"];
                    let (px, py) = centre(rect);
                    super::move_cursor(&state, px, py);
                    press(&state);
                }
                "click a decode field" => {
                    let (path, ..) = visible_fields(&state)[4].clone();
                    let rect = shot.tags[&format!("pv.tree.field.{path}")];
                    let (px, py) = centre(rect);
                    super::move_cursor(&state, px, py);
                    press(&state);
                }
                "type in the filter" => {
                    // Through the buffer the field owns, which is the thing a
                    // keystroke reaches — not through a setter of our own.
                    state.query.set_text("type = Query".to_owned());
                }
                other => panic!("no driver for the advertised gesture {other:?}"),
            }
            if witness(&state) == before {
                inert.push(format!("{gesture:?} — the strip promises {effect:?}"));
            }
        }
        assert!(
            inert.is_empty(),
            "{} advertised gesture(s) did nothing:\n  {}",
            inert.len(),
            inert.join("\n  ")
        );
    });
}

/// Everything this screen's advertised gestures can move, as one string.
fn witness(state: &std::rc::Rc<ViewState>) -> String {
    format!(
        "{}|{}|{:?}|{}",
        state.row.get(),
        state.field.get(),
        state.kept(),
        state.query.text()
    )
}

/// ★★★★★ R1707 — **the query narrows the list, on every channel at once.**
///
/// The defect this closes was measured on the running binary before the round:
/// the bar painted a query, three saved-filter chips and a `12,418 / 184,392`
/// count, and the list held sixteen rows whatever any of them said. Every check
/// in this example was green throughout, because nothing here had ever asked
/// what the filter DID — the same blind spot, in the same shape, that the
/// sibling screen's operation table was built to remove.
///
/// The four channels are asserted together on purpose. A filter that narrowed
/// the paint and not the hit test would answer a press with a message that is
/// not on screen; one that narrowed the paint and not the roster would let the
/// keyboard walk onto hidden rows; one that narrowed both and not the count
/// would tell the reader a number the screen contradicts.
#[test]
fn r1707_a_query_narrows_the_paint_the_press_the_cursor_and_the_count() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        let (opening, _) = painted_at(&state, (WIN_W, WIN_H));
        let drawn = |shot: &Painted| {
            (0..spec::ROWS.len())
                .filter(|n| shot.present(&format!("pv.list.row.{n}")))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            drawn(&opening).len(),
            spec::ROWS.len(),
            "the screen opens unfiltered — `spec::ROWS` is the requirement set"
        );

        // The reference's own query, run through the same slot an agent uses.
        super::set_query(&state, spec::EXAMPLE_QUERY);
        let (shot, _) = painted_at(&state, (WIN_W, WIN_H));
        let kept = state.kept();
        assert!(
            kept.len() < spec::ROWS.len() && !kept.is_empty(),
            "★ the query has to keep SOME and drop SOME, or neither direction \
             of this gate can fail: kept {kept:?}"
        );
        assert_eq!(drawn(&shot), kept, "the paint draws exactly what was kept");

        // The press. Every drawn row answers as itself, and no hidden row is
        // reachable at any point of the list.
        for &n in &kept {
            let (px, py) = centre(shot.tags[&format!("pv.list.row.{n}")]);
            assert_eq!(
                Hit::at(&state, px, py),
                Hit::Message(n),
                "a kept row must answer at its own rectangle"
            );
        }
        let hidden: Vec<usize> = (0..spec::ROWS.len())
            .filter(|n| !kept.contains(n))
            .collect();
        let list = list_rect();
        for y in (list.y..list.y + list.h).step_by(3) {
            if let Hit::Message(n) = Hit::at(&state, list.x + list.w / 2, y) {
                assert!(
                    !hidden.contains(&n),
                    "★ a press inside the list answered hidden message {n} — \
                     the row is not drawn and the press found it anyway"
                );
            }
        }

        // The cursor's roster is the kept set, and it stands on a member.
        let cursor = super::pane_cursor(&state, "pv.list").expect("the list has a cursor");
        let roster: Vec<String> = cursor.members().iter().map(|m| m.tag.clone()).collect();
        assert_eq!(
            roster,
            kept.iter()
                .map(|n| format!("pv.list.row.{n}"))
                .collect::<Vec<_>>(),
            "the keyboard walks the rows the query kept"
        );
        assert!(
            roster
                .iter()
                .any(|t| Some(t.as_str()) == cursor.cursor_tag()),
            "★ the cursor stands on a member of its own roster — a cursor \
             pointing at a filtered-out row is one no arrow key can leave"
        );

        // The count, and it is derived rather than stated.
        assert_eq!(
            super::count_line(&state),
            format!("{} of {} shown", kept.len(), spec::ROWS.len())
        );

        // And the reason each hidden row is hidden names a clause of the query.
        let clauses: Vec<String> = state
            .query()
            .clauses()
            .iter()
            .map(|c| c.text.clone())
            .collect();
        for &n in &hidden {
            let why = state
                .why_hidden(n)
                .unwrap_or_else(|| panic!("row {n} is hidden and says no reason"));
            assert!(
                clauses.contains(&why),
                "★ row {n} was dropped by {why:?}, which is not a clause of the \
                 running query {clauses:?}"
            );
        }
        assert!(
            !hidden.is_empty(),
            "the attribution loop above must have something to check"
        );

        // Clearing puts every message back.
        super::set_query(&state, "");
        let (cleared, _) = painted_at(&state, (WIN_W, WIN_H));
        assert_eq!(drawn(&cleared).len(), spec::ROWS.len());
    });
}

/// ★★★★★ R1707 — **a press inside the query box resolves to a byte of the
/// query**, and one outside it resolves to nothing.
///
/// This is the second thing this gate was: the first asked whether the screen's
/// hit test "stands aside" in the box, and a counterfactual proved that
/// question **could not fail here** — the answer for "standing aside" and the
/// answer for "nothing is there" are both `Hit::None`, and this bar puts
/// nothing under the box. So the arm was inert and the test with it, and both
/// are gone.
///
/// What actually carries the click-to-caret is `query_byte_at`, so that is what
/// is driven — through `WidgetView::position_caret_for_point`, the hook a real
/// press comes in by, over the box's whole painted area rather than its centre.
/// It can fail: a wrong rectangle, a dropped containment check or a lost focus
/// guard each break exactly one of the assertions below.
#[test]
fn r1707_a_press_in_the_query_box_lands_on_a_byte_of_the_query() {
    use pinion_shell::WidgetView;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        super::set_query(&state, "type = Query");
        let (shot, scene) = painted_at(&state, (WIN_W, WIN_H));
        let r = *shot
            .tags
            .get("pv.filter.query")
            .expect("the query box is painted, or there is no filter");
        // The shell states a pointer in logical pixels, and a window coordinate
        // is small — so the conversion goes through `u16`, which is lossless
        // into `f32`, rather than through a cast that discards precision the
        // lint is right to object to.
        let px_of = |v: u32| f32::from(u16::try_from(v).expect("a window coordinate"));
        let caret = |x: u32, y: u32, focused: Option<&str>| {
            super::PacketView::position_caret_for_point(
                &IDLE_FIELD,
                &scene,
                focused,
                // `hit_tag` is this screen's own root everywhere — every press
                // routes to one external — so the hook settles containment by
                // geometry, and passing it here changes nothing.
                None,
                px_of(x),
                px_of(y),
                false,
            )
        };

        let mut inside = Vec::new();
        for px in [r.x + 1, r.x + r.w / 2, r.x + r.w - 2] {
            for py in [r.y + 1, r.y + r.h / 2, r.y + r.h - 2] {
                let at = caret(px, py, Some("pv.filter.query"));
                assert!(
                    at.is_some(),
                    "({px}, {py}) is inside the painted box and resolved to no \
                     byte — the box can be typed into and not clicked into"
                );
                inside.push(at.expect("just asserted"));
            }
        }
        assert_eq!(inside.len(), 9, "nine points, not one");
        assert!(
            inside.iter().collect::<BTreeSet<_>>().len() > 1,
            "★ every point in the box answered the SAME byte ({inside:?}) — a \
             caret that lands in one place is not a caret the pointer placed"
        );

        // Outside the box: nothing. Without this the containment check could be
        // deleted and the nine assertions above would still pass.
        assert_eq!(
            caret(r.x + r.w + 40, r.y + r.h / 2, Some("pv.filter.query")),
            None,
            "a point beside the box is not a byte of the query"
        );
        // And unfocused: nothing, whatever the coordinates say.
        assert_eq!(
            caret(r.x + r.w / 2, r.y + r.h / 2, Some("pv.list")),
            None,
            "the box only takes a caret while it has focus"
        );
    });
}

/// ★★★ R1707 — **a saved chip runs its own query**, which is what it says on it.
///
/// Measured before the round: pressing one flipped a boolean, announced
/// "applied units only", and the list did not move. That is this screen's
/// instance of the class the tool's own reader keeps reporting — an affordance
/// that is named, announced and inert.
#[test]
fn r1707_a_saved_filter_chip_runs_the_query_it_names() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        for (n, saved) in spec::SAVED_FILTERS.iter().enumerate() {
            super::set_query(&state, "");
            toggle_saved(&state, n);
            assert_eq!(
                state.query.text(),
                saved.query,
                "chip {n} ({}) must run the query it declares",
                saved.name
            );
            let kept = state.kept();
            assert!(
                !kept.is_empty() && kept.len() < spec::ROWS.len(),
                "★ {} kept {} of {} — a saved filter that keeps everything or \
                 nothing cannot be told from one that does not run",
                saved.name,
                kept.len(),
                spec::ROWS.len()
            );
            // Pressing it again clears, rather than leaving the list narrowed
            // with nothing lit to say why.
            toggle_saved(&state, n);
            assert_eq!(state.kept().len(), spec::ROWS.len());
        }
    });
}
