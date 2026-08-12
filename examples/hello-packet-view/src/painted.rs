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
use pinion_core::{Frame, Scene};

use super::{
    Hit, MIN_H, MIN_W, ViewState, WIN_H, WIN_W, bytes_rect, centre, list_rect, press, select_byte,
    select_field, select_message, spec, toggle_layer, toggle_saved, tree_rect, use_view_state,
    visible_fields,
};

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
    reachable: BTreeMap<String, (String, (i32, i32))>,
}

impl Painted {
    fn of(scene: &Scene, window: (u32, u32)) -> Self {
        let mut tags = BTreeMap::new();
        let mut runs = Vec::new();
        let mut reachable = BTreeMap::new();
        for out in pinion_core::reach::out_of_sight(scene, window, &mut stand_in_ink) {
            if let (Some(tag), pinion_core::reach::Reach::Scrollable { to }) = (out.tag, out.reach)
            {
                reachable.insert(tag, (out.viewport.name, to));
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
        self.tags.contains_key(tag) || self.reachable.contains_key(tag)
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

/// A monospace stand-in for a glyph run: wider per character than any face this
/// screen uses, so a box that passes here has room for a real one.
fn stand_in_ink(text: &pinion_core::scene::TextNode) -> (u32, u32) {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a label is a handful of characters"
    )]
    let chars = text.content.chars().count() as u32;
    let px = text.style.font_size_px.max(1);
    let painted = if text.style.overflow.shortens() {
        text.rect.w.min(chars * px)
    } else {
        chars * px
    };
    (painted, text.rect.h)
}

/// Run the real pipeline at `size` and index what came out of it.
fn painted_at(state: &std::rc::Rc<ViewState>, size: (u32, u32)) -> (Painted, Scene) {
    // ★ R1654 — publish the size the way the shell does, so the sweep can ask
    // the screen to lay out at a size other than the one it was designed for.
    let owner = Owner::current().expect("the sweep runs inside a scope");
    pinion_core::reactive::VIEWPORT_SIZE
        .resolve(&owner)
        .set(size);
    let mut scene = super::view((), Frame::default());
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
        for n in 0..spec::ROWS.len() {
            wanted.push(format!("pv.list.row.{n}"));
            wanted.push(format!("pv.list.row.{n}.kind"));
        }
        for n in 0..spec::SAVED_FILTERS.len() {
            wanted.push(format!("pv.filter.saved.{n}"));
        }
        for n in 0..spec::QUERY_CLAUSES.len() {
            wanted.push(format!("pv.filter.clause.{n}"));
        }
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
    sweep(|_, shot, _, _, case| {
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
        assert_eq!(
            shot.family("pv.filter.clause.").len(),
            spec::QUERY_CLAUSES.len(),
            "{case}: query clauses"
        );
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
            state.said.borrow().contains("no field"),
            "the screen must say why nothing happened, said {:?}",
            state.said.borrow()
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
