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
    // ★★★★★ R1747 — recording what was painted, exactly as the shell does on
    // the same pass it announces the sizes. The screen judges its own paint
    // now, so a sweep that skipped this line would be asking the verdict of a
    // store this frame never filled.
    //
    // BOTH surfaces, and the plural call is the whole point: the query box is
    // an `External` of its own, so in a window it is a surface and the screen's
    // own store holds neither it nor the run inside it. Recording only the
    // screen would file that run under the screen, and the sweep would then be
    // reading a surface the window does not have.
    assert_eq!(
        pinion_runtime::record_painted_surfaces(&scene, &[super::VIEW_TAG, super::QUERY_TAG]),
        2,
        "the screen and its query box are both painted, or the verdict below is \
         asked of a store this frame never filled",
    );
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
        // ★★★★★ R1721 — the bar's stops are its rule's, not a list here. Three
        // became one, and the arrows, `Home`, `End` and `Enter` that one stop
        // carries are what a keyboard got in exchange.
        let mut want: Vec<String> = super::saved_stops();
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

// ── 7. Judged: the screen against a specification another hand wrote ────────

/// ★★★★★ R1747 — **the integration gate: the capture viewer the pipeline
/// paints, against `docs/analyzer-packets-spec.json`, in every state and at
/// every size the sweep drives.**
///
/// Everything above this line compares the screen with `crate::spec` — the
/// screen's own table, written in the same edit as the painter it feeds. This
/// one compares it with a pin extracted from the behaviour reference, and the
/// difference is the whole reason both exist: the first says this build is
/// self-consistent, and only the second can say it is the reference.
///
/// Judged as an equality in both directions, through
/// [`crate::judge::built`] — the SAME function the running window and the
/// assembled application answer from, so a copy of these readings kept here
/// would be the second account whose disagreement nobody notices, because the
/// one running in a window is not the one anybody runs.
///
/// ★ Two things are asserted about the ABSENCES rather than left implicit:
/// that only the surface whose parts a session can take away is ever away, and
/// that the sweep reaches **both** of the reasons it can be away for. A gate
/// that tolerated any away would pass a screen that had stopped painting.
///
/// # 🟥★★★★★ What the size axis measured, and why the rule below is in two parts
///
/// At the size the screen opens in and maximised, the whole document
/// reconciles. **At the floor this screen declares it does not**, and the
/// reason is not a defect: the decode pane is short enough there that three of
/// the four layer headings are below its fold, one scroll away. A verdict read
/// from the PAINT is about the frame, so it reports them absent — and that is
/// the honest reading, because they are not on that frame.
///
/// R1742 met the same thing on the sibling screen (a 74-pixel inspector) and
/// **reported it rather than adding an away for it**, which is the precedent
/// followed here: an away-condition of *the pane cannot show all of them at
/// once* is a condition under which the verdict fails rather than a state the
/// screen can point at, and it would swallow a pane that had genuinely stopped
/// drawing its headings.
///
/// So the rule is two-part, and the second part is the one with teeth:
///
/// 1. at the size the specification describes, the document **reconciles**;
/// 2. at every size, an undeclared difference is an **absence**, or a
///    reordering with an absence beside it to explain it — shrinking a window
///    may take a part off the frame, and it must never rename one or grow one
///    the reference has not. (A part that goes off the frame shifts every part
///    after it, so a reordering is a consequence rather than a claim; a
///    reordering with nothing missing is a renaming in disguise.)
///
/// ⚠ The framework cannot yet close the gap: `pinion_core::reach` knows a mark
/// that is one scroll away, and the paint store this verdict reads holds only
/// what was drawn. A section judging itself has no scene to ask.
#[test]
fn r1747_the_capture_viewer_reproduces_its_specification_or_says_why_not() {
    use pinion_core::conformance::{Built, PartDivergence, Unreconciled};

    let mut reasons: BTreeSet<String> = BTreeSet::new();
    let mut judged = 0usize;
    let mut at_declared = 0usize;
    let mut off_frame: BTreeSet<String> = BTreeSet::new();
    sweep(|_, _, _, _, case| {
        let regions =
            pinion_core::painted::painted_regions(super::VIEW_TAG).expect("the sweep just painted");
        let doc = spec::packets_document();
        let opening = at_the_declared_extent(&regions, &doc);
        at_declared += usize::from(opening);
        for surface in doc.surfaces() {
            match super::judge::built(&regions, surface) {
                Built::Away(why) => {
                    assert_eq!(
                        surface, "selection",
                        "{case}: only the surface a session can take away may be \
                         away, and `{surface}` said: {why}",
                    );
                    assert!(!why.is_empty(), "{case}: an away carries its own reason");
                    reasons.insert(why);
                }
                Built::Standing(parts) => {
                    let said: Vec<String> = doc
                        .unreconciled(surface, &parts)
                        .iter()
                        .map(Unreconciled::sentence)
                        .collect();
                    assert!(
                        !opening || said.is_empty(),
                        "{case}: `{surface}` is not what \
                         docs/analyzer-packets-spec.json declares:\n  {}",
                        said.join("\n  "),
                    );
                    // Part 2: at ANY size, an UNDECLARED difference may only be
                    // a part that is not on this frame. The ledger's own
                    // entries are excluded here rather than everywhere: part 1
                    // above is what holds them to being exactly right, and
                    // re-reporting one as a forbidden reordering would make the
                    // declared remainder impossible to declare.
                    let canon = doc.canon(surface).expect("the document fixes it");
                    let undeclared: BTreeSet<String> = doc
                        .unreconciled(surface, &parts)
                        .into_iter()
                        .filter_map(|entry| match entry {
                            Unreconciled::Undeclared { sentence, .. } => Some(sentence),
                            // ★ R1770 — a refusal to judge a sized entry is not
                            // a difference this build has, and this set is the
                            // differences.
                            Unreconciled::Paid { .. }
                            | Unreconciled::Reworded { .. }
                            | Unreconciled::Unsized { .. } => None,
                        })
                        .collect();
                    let divergences: Vec<PartDivergence> = canon
                        .diff(&parts)
                        .into_iter()
                        .filter(|d| undeclared.contains(&d.sentence()))
                        .collect();
                    // ★ A part that is off the frame necessarily shifts every
                    // part after it, so a reordering is a CONSEQUENCE of an
                    // absence rather than a claim of its own. Measured: at the
                    // floor the decode row the tree draws open is off the frame,
                    // and the pane's readout after it moves from part 1 to part
                    // 0. What must never happen is a renaming or an arrival, and
                    // a reordering with no absence beside it is one of those in
                    // disguise.
                    let short = divergences
                        .iter()
                        .any(|d| matches!(d, PartDivergence::Absent { .. }));
                    for divergence in &divergences {
                        match divergence {
                            PartDivergence::Absent { key, .. } => {
                                assert!(
                                    !opening,
                                    "{case}: `{surface}` is short `{key}` at the size \
                                     the specification describes",
                                );
                                off_frame.insert(format!("{surface}.{key}"));
                            }
                            PartDivergence::OutOfOrder { key, .. } => assert!(
                                short,
                                "{case}: `{surface}` moved `{key}` with nothing off \
                                 the frame to explain it",
                            ),
                            other => panic!(
                                "{case}: shrinking the window may take a part off the \
                                 frame and nothing else, and `{surface}` says: {}",
                                other.sentence(),
                            ),
                        }
                    }
                    judged += 1;
                }
            }
        }
    });
    assert_eq!(
        reasons.len(),
        2,
        "★ the swept states reach BOTH reasons a decode row can light no bytes \
         -- a value the decoder worked out, and one read from a source this \
         pane is not showing. Reached: {reasons:?}",
    );
    assert!(
        judged >= 100,
        "and the sweep judged {judged} standing surface(s), which is enough for \
         the sentence above to be about a screen rather than about one frame",
    );
    // ★ Measured rather than asserted as a bound: what the smaller sizes put
    // out of the frame, printed so the number is a fact somebody can read
    // rather than a threshold nobody re-derives.
    assert!(
        !off_frame.is_empty(),
        "★ and at least one part IS off the frame at a smaller size -- if this \
         ever becomes empty the two-part rule above has stopped distinguishing \
         anything and should be collapsed into part 1",
    );
    assert_swept_the_declared_extent(at_declared);
    println!("off the frame at a smaller size: {off_frame:?}");
}

/// ★★★★★ R1770 — whether this frame is the one the specification describes.
///
/// Read off the PIN and compared with what the frame recorded, rather than
/// being this module's own `(WIN_W, WIN_H)`. Two constants in two files
/// agreeing is not the same claim as the surface actually having that extent,
/// and the difference between them is what let the assembled tool judge one pin
/// at a fifth of its area without anything noticing.
#[cfg(test)]
fn at_the_declared_extent(
    regions: &pinion_core::painted::PaintedRegions,
    doc: &pinion_core::conformance::SpecDocument,
) -> bool {
    regions.extent().is_some() && regions.extent() == doc.written_at()
}

/// ★ R1770 — the sweep visited the size the pin was written at.
///
/// Counted rather than asserted per case, because this sweep visits several
/// sizes on purpose: a verdict read anywhere else is a different claim and the
/// report says so, but the strict reading has to happen somewhere or the pin's
/// `$at` is a number nothing checks.
#[cfg(test)]
fn assert_swept_the_declared_extent(at_declared: usize) {
    assert!(
        at_declared > 0,
        "★★★★★ R1770 — this sweep never painted at the extent \
         docs/analyzer-packets-spec.json says its canon was written against \
         ({:?}), so `opening` was false everywhere and the strict half of that \
         gate never ran.",
        spec::packets_document().written_at(),
    );
}

/// ★★★★★ R1747, restated by R1758 — the query box is **a mark of the screen
/// that drew it and a surface of its own**, and those are two facts rather than
/// a contradiction.
///
/// R1747 asserted the first half backwards. A widget that owns focus is an
/// `External`, so the paint store registers it as a surface; attribution was
/// geometric, so the box's own node fell inside its own rectangle and was
/// dropped from its host — and the bar reported its query absent. That was
/// written down here as the framework's intended behaviour, with a note saying
/// a change that "folded nested surfaces back into their host" should fail
/// here. R1758 made exactly that change deliberately, from the other side of
/// the same class (a host's box HOLDING a nested surface was being attributed
/// to the child), and this gate is what caught the consequence — which is what
/// it was for.
///
/// The rule now: a mark belongs to the nearest ancestor that is a surface. So
/// the box belongs to the screen, everything drawn INSIDE the box belongs to
/// the box, and the bar can say where in itself the box sits.
#[test]
fn r1747_a_focus_owning_widget_is_a_surface_and_not_a_mark_of_its_host() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        painted_at(&state, (WIN_W, WIN_H));
        let screen =
            pinion_core::painted::painted_regions(super::VIEW_TAG).expect("the sweep just painted");
        assert!(
            screen.marks().any(|(tag, _)| tag == super::QUERY_TAG),
            "the box the screen drew is a mark of the screen",
        );
        assert!(
            !screen
                .marks()
                .any(|(tag, _)| tag.starts_with(&format!("{}.", super::QUERY_TAG))),
            "while everything drawn INSIDE the box belongs to the box's own store",
        );
        // ★ The presence of an ENTRY is the fact `judge::filter_bar` rests on,
        // and it is a different fact from the entry holding anything. Measured
        // when this test first ran: at rest the box holds an empty run, so its
        // store is present and empty, and an assertion that it held a mark was
        // asserting more than is true. What the verdict asks is whether the
        // surface painted at all.
        assert!(
            pinion_core::painted::painted_regions(super::QUERY_TAG).is_some(),
            "and the query box painted a surface of its own, which is what says \
             the bar has a query on it",
        );
        assert!(
            pinion_core::painted::painted_regions("pv.filter.saved").is_none(),
            "while a part that is NOT a surface has no store of its own -- \
             without this the assertion above would pass for any tag at all",
        );
    });
}

// ── R1772: the operations this screen offers, and by which cause ────────────

/// A screen as it opens, with `earlier` — the operation this one `needs` — done
/// first, through that operation's own declared cause.
///
/// A real property of the tool rather than a convenience: selecting a field is
/// only possible once a message is selected, and a gate that reached the
/// precondition by writing the state directly would be testing a screen no
/// person can be in.
#[cfg(test)]
fn fresh_state_reaching(earlier: Option<&str>) -> std::rc::Rc<ViewState> {
    // ★★★★★ `use_view_state` is a HOOK: one state per owner scope, so this
    // RESETS rather than constructs, and saying so matters. The first draft
    // cleared the field with `select_field(&state, "")`, which returns early
    // for a path the decode map does not hold — so the field kept whatever the
    // previous row had left, and the gate reported *the gesture ran and
    // `selected_field` did not move* for a gesture that worked perfectly. A
    // reset that can silently not reset is worse than none.
    let state = use_view_state();
    super::run_filter(&state, "").expect("an empty query is the whole capture");
    select_message(&state, 0);
    state.field.set(String::new());
    state.list_scroll.scroll_to(0, 0);
    state.tree_scroll.scroll_to(0, 0);
    state.bytes_scroll.scroll_to(0, 0);
    if let Some(earlier) = earlier {
        let op = spec::OPERATIONS
            .iter()
            .find(|op| op.name == earlier)
            .expect("the gate asserts a `needs` names a row of this table");
        if let Some((verb, arg)) = op.verb {
            invoke_for_test(&state, verb, arg).expect("the precondition's own verb is routed");
        } else {
            let drive = OPERATION_GESTURES
                .iter()
                .find(|(name, _)| *name == earlier)
                .map(|(_, drive)| *drive)
                .expect("a row with no verb declares a gesture, and the gate checks it");
            drive(&state);
        }
    }
    state
}

/// The value of one published slot, as a client sees it.
///
/// Read through the screen's own introspection rather than off the state, so a
/// slot that moved internally and is not published reads as *did not move* —
/// which is the honest answer to *can an agent see this happen*, and the whole
/// reason the three scroll rows needed `scroll` published before they could
/// exist.
#[cfg(test)]
fn witness_of(state: &std::rc::Rc<ViewState>, slot: &str) -> String {
    let mut oracle = super::ViewOracle::new();
    oracle.attach(std::rc::Rc::clone(state));
    format!(
        "{:?}",
        pinion_core::external::ExternalIntrospect::query(&oracle, slot)
    )
}

/// One action, invoked the way an agent invokes it — **with the argument in the
/// type the screen DECLARES for it**.
///
/// ★★★★★ R1772 — the type is read out of the published schema rather than
/// guessed, and the first draft guessed. It sent every argument as text, and
/// `select_message` answered `expected a row index`: a real disagreement
/// between the table's textual argument and the action's declared `int`, caught
/// by running. Deriving it means a row cannot be written whose argument the
/// screen would refuse, and it means this driver keeps working when an action
/// changes type — the schema is the one place that says so.
#[cfg(test)]
fn invoke_for_test(state: &std::rc::Rc<ViewState>, verb: &str, arg: &str) -> Result<(), String> {
    use pinion_core::external::{ExternalIntrospect, IntrospectValue};

    let mut oracle = super::ViewOracle::new();
    oracle.attach(std::rc::Rc::clone(state));
    let declared = oracle
        .schema()
        .field_for(verb)
        .map(|field| field.ty)
        .ok_or_else(|| format!("`{verb}` is not a declared action of this screen"))?;
    let value =
        match declared {
            "int" => IntrospectValue::Int(arg.parse::<i64>().map_err(|_| {
                format!("`{verb}` declares `int` and the table's argument is {arg:?}")
            })?),
            _ => IntrospectValue::Text(arg.to_owned()),
        };
    ExternalIntrospect::invoke(&mut oracle, verb, value)
        .map(|_| ())
        .map_err(|why| format!("{why:?}"))
}

/// Scroll one pane at a size where its content actually overflows.
///
/// ★★★★★ R1772 — written because the gate said *the gesture ran and `scroll`
/// did not move*, three times, and the cause was neither the screen nor the
/// wire. `ScrollState::scroll_by` clamps to a maximum the LAYOUT measures, so a
/// pane whose content fits cannot be scrolled — correctly. Whether it fits
/// depends on the window, and this build's capture holds sixteen messages
/// where the reference's holds a hundred and eighty thousand, so at the size
/// this screen opens in there is genuinely nothing to scroll.
///
/// Sweeping the sizes this screen already declares it supports is the honest
/// claim: *a person can scroll this pane at some size this screen is laid out
/// for*. Hard-coding one size would have made the gate assert a fact about that
/// size while reading as a fact about the screen — and if no swept size
/// overflows, the driver moves nothing and the gate fails, which is the answer
/// that would mean the row must not claim the gesture.
#[cfg(test)]
fn scroll_somewhere_it_overflows(
    state: &std::rc::Rc<ViewState>,
    pane: fn(&ViewState) -> &std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
) {
    for (_, size) in SIZES {
        painted_at(state, *size);
        let scroll = pane(state);
        let before = scroll.offset();
        scroll.scroll_by(0, 40);
        if scroll.offset() != before {
            return;
        }
    }
}

/// The gesture that causes one operation, joined to [`spec::OPERATIONS`] by the
/// operation's name.
///
/// A table beside the specification rather than a column inside it, for the
/// reason the sibling screen states: a gesture is a press at a place or a drag
/// between two, and that is not a value a `const` row can hold. The gate
/// asserts the two tables name the same operations in **both** directions, so a
/// `gesture: true` with nothing behind it fails and so does a driver for a row
/// that claims no gesture.
#[cfg(test)]
type GestureDriver = fn(&std::rc::Rc<ViewState>);

#[cfg(test)]
const OPERATION_GESTURES: &[(&str, GestureDriver)] = &[
    ("filter the capture", |state| {
        super::run_filter(state, "type=data").expect("the query parses");
    }),
    ("clear the filter", |state| {
        super::run_filter(state, "").expect("an empty query is the whole capture");
    }),
    ("select a message", |state| {
        select_message(state, 3);
    }),
    ("select a decoded field", |state| {
        select_field(state, "l0.link");
    }),
    // ★★★★★ The three the reference offers that no ACTION reaches. The driver
    // is the pane's own scroll state, which is what a pointer moves — and until
    // this round nothing published that it had moved, so these rows had no
    // witness to name and could not have been written at all.
    //
    // ★★ Each PAINTS first, and finding out why is the gate earning its place.
    // `ScrollState::scroll_by` clamps to a maximum the LAYOUT measures, so on a
    // screen that has never been laid out the maximum is zero and a scroll
    // moves nothing. A driver that skipped the paint would have been asserting
    // that this screen cannot scroll, when what could not scroll was the
    // fixture — and a pointer scroll only ever happens on a laid-out frame
    // anyway, so painting is what makes this driver the gesture it claims.
    ("scroll the message list", |state| {
        scroll_somewhere_it_overflows(state, |s| &s.list_scroll);
    }),
    ("scroll the decode tree", |state| {
        scroll_somewhere_it_overflows(state, |s| &s.tree_scroll);
    }),
    ("scroll the byte pane", |state| {
        scroll_somewhere_it_overflows(state, |s| &s.bytes_scroll);
    }),
];

/// ★★★★★ R1772 — **every declared way of causing an operation causes it**, on
/// the third analyzer screen to be asked.
///
/// The rule is the sibling screens': a `verb` must be routed and must move the
/// slot the row names, a `gesture: true` must have a driver and that driver must
/// move the same slot, and the count of operations this screen cannot perform
/// at all is a RATCHET rather than a target — it fails when the number grows
/// and it fails when the number shrinks without the table being updated, so the
/// only way past it is to move a row.
#[test]
fn r1772_every_declared_way_of_causing_an_operation_causes_it() {
    let owner = Owner::new();
    owner.run(|| {
        let declared: BTreeSet<&str> = spec::OPERATIONS
            .iter()
            .filter(|op| op.gesture)
            .map(|op| op.name)
            .collect();
        let driven: BTreeSet<&str> = OPERATION_GESTURES.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            declared, driven,
            "★ the specification and the drivers name different operations — a \
             gesture declared with nothing behind it is a promise no gate holds \
             the screen to",
        );
        let names: BTreeSet<&str> = spec::OPERATIONS.iter().map(|op| op.name).collect();
        assert_eq!(
            names.len(),
            spec::OPERATIONS.len(),
            "the operations are named uniquely, or the join above is ambiguous",
        );
        for op in spec::OPERATIONS {
            if let Some(earlier) = op.needs {
                assert!(
                    names.contains(earlier),
                    "{:?} needs {earlier:?}, which this table does not hold",
                    op.name,
                );
            }
        }

        let mut inert: Vec<String> = Vec::new();
        let mut exercised = 0usize;

        for op in spec::OPERATIONS {
            // Each cause is measured from a fresh screen, so one row cannot
            // pass on the state another row left behind.
            if let Some((verb, arg)) = op.verb {
                let state = fresh_state_reaching(op.needs);
                let before = witness_of(&state, op.witness);
                let answer = invoke_for_test(&state, verb, arg);
                let after = witness_of(&state, op.witness);
                exercised += 1;
                if let Err(why) = answer {
                    inert.push(format!(
                        "{:?}: the wire refused `{verb} {arg}` ({why})",
                        op.name
                    ));
                } else if before == after {
                    inert.push(format!(
                        "{:?}: `{verb} {arg}` was accepted and `{}` did not move",
                        op.name, op.witness,
                    ));
                }
            }
            if op.gesture {
                let state = fresh_state_reaching(op.needs);
                let before = witness_of(&state, op.witness);
                let cause = OPERATION_GESTURES
                    .iter()
                    .find(|(name, _)| *name == op.name)
                    .map(|(_, drive)| *drive)
                    .expect("the two tables were just asserted to agree");
                cause(&state);
                let after = witness_of(&state, op.witness);
                exercised += 1;
                if before == after {
                    inert.push(format!(
                        "{:?}: the gesture ran and `{}` did not move",
                        op.name, op.witness,
                    ));
                }
            }
        }

        assert!(
            inert.is_empty(),
            "declared causes that cause nothing:\n  {}",
            inert.join("\n  ")
        );
        assert!(
            exercised >= spec::OPERATIONS.len(),
            "every operation was exercised by at least one of its declared causes",
        );

        // ★ The ratchet. Written as an equality on purpose: a row that becomes
        // reachable must be moved in the table, and a row that stops being
        // reachable must be too.
        let absent = spec::OPERATIONS.iter().filter(|op| !op.reachable()).count();
        assert_eq!(
            absent, 0,
            "★ every operation the reference's capture view offers is reachable \
             on this screen by SOME cause. If this number grows, a row went \
             absent; if the table grows a row that is absent, say so here.",
        );
        // ★★★★★ And the asymmetry the table exists to expose, which the count
        // above cannot see: three of the seven are reachable only by a person.
        let agentless = spec::OPERATIONS
            .iter()
            .filter(|op| op.verb.is_none())
            .count();
        assert_eq!(
            agentless, 3,
            "★ the three scrollable regions the reference offers have no action \
             on THIS SCREEN. Publishing `scroll` (R1772) made them observable; \
             a verb for them is what is still absent. ⚠ Not a claim that a \
             client cannot scroll them at all — the framework's own \
             `scene/scroll` reaches all three, which R1772's demo drives.",
        );
    });
}

/// ★★★★★ R1774 §5.32 §5.45 — **every clamp this screen has is reached from
/// both sides by the sweep above.**
///
/// Screen C asked this first (R1669) and found a real defect on its first run;
/// the debt that recorded it observed that screens A and B carry guards of the
/// same shape and **nobody asks them the question**. This is screen B's answer,
/// and the third consumer of the lifted vocabulary — the screen supplies only
/// its own observations, the rule lives in the crate.
///
/// # What a clamp is on THIS screen
///
/// Screen B's three panes SCROLL, so a row below the fold is not clamped — the
/// reader scrolls to it, and the painted index holds it either way because it
/// was built. What is clamped here is a member the screen declares and then
/// does not BUILD: a byte row the dump stops short of, a field a fold removes.
/// So each observable names the exact tags its family should contain and asks
/// how many of them the frame actually has — the guard's effect, never the
/// condition it tests.
///
/// # The split, derived rather than written down
///
/// ★★★★★ The first run of this gate reported four of seven families as *never
/// clamped*, and the cause was the observable rather than the screen: those
/// four have **no drop-branch at all**. The derivation is where the family's
/// size comes from. A family the STATE sizes — the rows a query kept, the
/// fields a fold left, the rows a frame's length needs — can come out shorter
/// than it asked for, so it is a clamp and belongs in the census. A family the
/// SPECIFICATION sizes is a fixed strip: `spec::COLUMNS`, `spec::SAVED_FILTERS`,
/// `spec::CONTEXT`, `spec::LANES` never change length, nothing tests whether
/// they fit, and folding them into the census would report an unexercised guard
/// that does not exist.
///
/// They are not dropped, though — they get the assertion that IS true of them
/// and would be false the moment someone added such a branch: every member is
/// built at every swept state and size, the declared floor included. What keeps
/// them inside their strip is the containment gate above, not a clamp; between
/// the two, a member can neither vanish silently nor be painted where nobody
/// can see it.
#[test]
fn r1774_the_sweep_reaches_both_sides_of_every_clamp() {
    let mut census = pinion_core::test_fixtures::clamp::ClampCensus::new();
    sweep(|state, shot, _, _, case| {
        // (name, the tags this family should hold, whether the STATE sizes it)
        let families: &[(&str, Vec<String>, bool)] = &[
            (
                "list: rows",
                state
                    .kept()
                    .iter()
                    .map(|n| format!("pv.list.row.{n}"))
                    .collect(),
                true,
            ),
            (
                "tree: fields",
                super::visible_fields(state)
                    .iter()
                    .map(|(path, ..)| format!("pv.tree.field.{path}"))
                    .collect(),
                true,
            ),
            (
                // ★★★★★ The OFFSET label, which is what a byte row paints.
                // The first draft asked for `pv.bytes.row.{n}` and the census
                // reported the family as clamped on every single frame — 0 of
                // N built, always. The cause was the observable for the third
                // time in this round: that tag is an ANNOUNCEMENT, and the
                // function producing it says so in its own doc — nothing
                // paints it, a byte row being the eight cells and the offset
                // beside them. An observable naming a tag no painter emits
                // reads as a permanent clamp, which is indistinguishable from
                // a screen that truly always truncates.
                //
                // Spelled through the screen's own helper for the reason that
                // helper exists: the paint and the census must not grow two
                // conventions for one address.
                "bytes: offsets",
                (0..state.frame_bytes().len().div_ceil(spec::BYTES_PER_ROW))
                    .map(super::bytes_offset_tag)
                    .collect(),
                true,
            ),
            (
                "list: columns",
                (0..spec::COLUMNS.len())
                    .map(|n| format!("pv.list.head.{n}"))
                    .collect(),
                false,
            ),
            (
                "filter: saved chips",
                (0..spec::SAVED_FILTERS.len())
                    .map(|n| format!("pv.filter.saved.{n}"))
                    .collect(),
                false,
            ),
            (
                "context: values",
                spec::CONTEXT
                    .iter()
                    .map(|v| format!("pv.context.{}", v.key.replace(' ', "_")))
                    .collect(),
                false,
            ),
            (
                "reassembly: lanes",
                (0..spec::LANES.len())
                    .map(|n| format!("pv.reassembly.lane.{n}"))
                    .collect(),
                false,
            ),
        ];

        for (what, declared, state_sized) in families {
            // A family with no members this frame says nothing either way, and
            // folding it in as "not clamped" would be the census asserting
            // completeness it did not observe.
            if declared.is_empty() {
                continue;
            }
            let missing: Vec<&String> = declared
                .iter()
                .filter(|t| !shot.tags.contains_key(*t))
                .collect();
            if *state_sized {
                census.note(*what, !missing.is_empty());
            } else {
                assert!(
                    missing.is_empty(),
                    "{case}: {what} is sized by the specification and has no \
                     branch that drops one, yet {} of its {} members were not \
                     built: {missing:?}. Either the screen grew a clamp — in \
                     which case this family belongs in the census beside it — \
                     or it is losing content silently",
                    missing.len(),
                    declared.len(),
                );
            }
        }
    });
    // Three, and the floor is what refuses a derivation that quietly yields
    // nothing (R1651.1): an empty census passes both other clauses vacuously
    // and reads as a screen with no unexercised guard.
    census.assert_both_sides_reached("capture viewer", 3);
}
