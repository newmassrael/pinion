//! R1668 — **the integration test: the screen the pipeline paints, against the
//! specification it is supposed to reproduce.**
//!
//! `tests.rs` next door asserts the model. Neither it nor anything else in this
//! example rendered anything, and R1653 measured what that costs on screen A:
//! three consecutive rounds shipped a painter that disagreed with its own hit
//! test while every model test passed, because every one of them asked the
//! geometry *helper* where a control was, and the helper was right each time.
//! Screens A and B gained this module then; screen C is the one that did not,
//! and it is also the one nobody had written a specification for.
//!
//! So this module runs the real pipeline — `view()` then
//! [`pinion_runtime::compute_layout`], the same two stages the window runs
//! before handing a scene to the rasteriser — and asks the resulting scene
//! where things ended up, through [`pinion_core::NodeVisit::absolute_rect`], at
//! every state in [`STATES`] and every size in [`SIZES`]:
//!
//! 1. **Forward** — every element the specification declares is painted.
//! 2. **Backward** — every painted tag belongs to a declared family, and the
//!    counts the specification fixes are the counts on the screen.
//! 3. **Reachable** — every painted control answers for *itself* when pressed
//!    at the centre of the rectangle it was painted in.
//! 4. **Contained** — every painted mark lies inside the pane its address puts
//!    it in, and no card's content leaves its card.
//! 5. **Disjoint** — no two rows of one card, and no two text runs of one
//!    widget, are painted on top of each other.
//! 6. **Reserved** — this round's own law, on the painted screen: every seat the
//!    specification reserves is *declared unavailable with its booking*, is
//!    therefore inert and faded and announced, and no reserved seat can be
//!    placed by any path the shell offers.
//!
//! ★ The population of every check comes from the scene or from
//! [`crate::spec`], never written out here — R1651.1 measured what a
//! hand-written population costs: "forty controls pass" read as coverage when
//! it was a sample, and three of the ones nobody had chosen were broken.

use std::collections::{BTreeMap, BTreeSet};

use pinion_core::availability::{Recourse, UnavailableKind};
use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::{Frame, Scene};

use super::{Hit, ShellOracle, ShellState, WIN_H, WIN_W, spec, use_shell_state};

/// One swept state: what to call it, and the edit that reaches it.
type SweptState = (&'static str, fn(&std::rc::Rc<ShellState>));

/// The states the screen is swept in.
///
/// They compose — state *n* is state *n-1* plus one edit — because that is what
/// a session with the tool looks like, and because a screen that only survives
/// being reset is not a screen anybody can use. R1652.1 is why this is a list at
/// all: the specification describes the screen *as it opens*, that was the only
/// state anybody had ever checked, and it is the state nobody works in.
const STATES: &[SweptState] = &[
    ("as it opens", |_| {}),
    ("with a card selected", |state| {
        let id = state.placed()[0].id().as_str().to_owned();
        state.selected.set(Some(id));
    }),
    ("in layout-edit mode", |state| {
        state.editing.set(true);
    }),
    ("with the preset menu open", |state| {
        state.editing.set(false);
        state.preset_open.set(true);
    }),
    ("with a second card of a placed kind", |state| {
        state.preset_open.set(false);
        ShellOracle::add(state, spec::BOARD[0].kind).expect("a placeable kind is placed");
    }),
    ("with a card maximised", |state| {
        let id = state.placed()[0].id().as_str().to_owned();
        ShellOracle::maximize(state, &id).expect("a placed card maximises");
    }),
    // ★ R1668 — every card shrunk to one cell. Found by a counterfactual: the
    // painters clamp their tables at the card's edge, and NO swept state was
    // ever small enough for the clamp to fire, so removing it changed nothing
    // and the gate stayed green with the mechanism gone. A person reaches this
    // state with the size steppers in two presses.
    ("with every card shrunk to one cell", |state| {
        let ids: Vec<String> = state
            .placed()
            .iter()
            .map(|c| c.id().as_str().to_owned())
            .collect();
        ShellOracle::restore(state).ok();
        for id in ids {
            for _ in 0..spec::GRID_COLS {
                ShellOracle::step(state, &id, "narrow").ok();
                ShellOracle::step(state, &id, "shorter").ok();
            }
        }
    }),
];

/// The window sizes the screen is swept at.
///
/// ★ R1656 — size was never an axis on screen A, and that is why five defects
/// got out. The opening size is first because the specification describes it;
/// the others are the two a person actually produces, a maximised window and
/// the window dragged down to the floor this screen declares.
/// ★ This screen declares `SizeStrategy::Fixed` at its opening size, so the
/// window's floor IS the size it opens in and there is no smaller state to
/// sweep. The third size is therefore an intermediate one rather than a floor:
/// a window somebody dragged part of the way out, which is the size a fixed
/// layout is least likely to have been checked at.
const SIZES: &[(&str, (u32, u32))] = &[
    ("at the size it opens in", (WIN_W, WIN_H)),
    ("maximised", (2494, 1531)),
    ("dragged part of the way out", (1760, 1080)),
];

/// Where every tag in the painted scene ended up, and every text run with it.
struct Painted {
    /// Tag -> the rectangle the layout pass gave it, window-absolute.
    tags: BTreeMap<String, Rect>,
    /// Every text run: its content, its rectangle, and the tag of its nearest
    /// tagged ancestor. Runs carry no tag of their own.
    runs: Vec<(String, Rect, Option<String>)>,
    /// R1668 — every node the disabled cascade resolved, with the reason that
    /// reached it. Read from the framework's own census rather than from this
    /// module's idea of which tags ought to be inert.
    inert: BTreeMap<String, (UnavailableKind, String, Recourse)>,
}

impl Painted {
    fn of(scene: &Scene) -> Self {
        let mut tags = BTreeMap::new();
        let mut runs = Vec::new();
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
        let inert = pinion_core::scene_disabled::disabled_census(scene)
            .into_iter()
            .map(|row| {
                (
                    row.tag,
                    (
                        row.reason.kind(),
                        row.reason.detail().to_owned(),
                        row.reason.recourse(),
                    ),
                )
            })
            .collect();
        Self { tags, runs, inert }
    }

    fn rect(&self, tag: &str) -> Option<Rect> {
        self.tags.get(tag).copied()
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
fn painted_at(size: (u32, u32)) -> (Painted, Scene) {
    // ★ R1654 — publish the size the way the shell does, so the sweep can ask
    // the screen to lay out at a size other than the one it was designed for.
    let owner = Owner::current().expect("the sweep runs inside a scope");
    pinion_core::reactive::VIEWPORT_SIZE
        .resolve(&owner)
        .set(size);
    let mut scene = super::view((), Frame::default());
    let mut cache = pinion_runtime::LayoutCache::new();
    pinion_runtime::compute_layout(&mut scene, &mut cache, size.0, size.1);
    // ★ The cascade is what the WINDOW runs after layout, through the settle
    // loop. A sweep that skipped it would ask "is this seat inert" of a scene
    // in which nothing had yet resolved, and would answer no for a reason that
    // has nothing to do with the screen.
    pinion_core::scene_disabled::resolve_disabled(&mut scene);
    (Painted::of(&scene), scene)
}

/// Which case a check is looking at.
///
/// The state's INDEX is handed over rather than inferred, because "the state
/// the specification describes" is a fact about the sweep and a check that
/// guesses at it from the shell's flags gets it wrong -- this one did, on its
/// first attempt, and reported a correctly-narrowed card as a defect.
struct Case<'a> {
    /// What to call it in a message.
    name: &'a str,
    /// Its index in [`STATES`]; `0` is the screen as it opens.
    state: usize,
    /// The window size it was laid out at.
    size: (u32, u32),
}

impl Case<'_> {
    /// Whether this is the screen exactly as the specification describes it:
    /// the state it opens in, at the size it opens at.
    fn as_specified(&self) -> bool {
        self.state == 0 && self.size == (WIN_W, WIN_H)
    }
}

impl std::fmt::Display for Case<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

/// The number of (state, size) cases one sweep covers. Pinned so a state or a
/// size deleted from a list takes its coverage with it loudly rather than
/// quietly (R1653: "a deleted state takes its own assertions with it").
const CASES: usize = 21;

/// Run `check` over every state at every size, naming the case in the message.
///
/// Returns how many cases ran, so a caller can pin it. ★ R1651.1 — a check
/// whose population derives to nothing passes, and reads afterwards as though
/// it had covered everything; the pin is what separates "nothing was wrong"
/// from "nothing was asked".
fn sweep(mut check: impl FnMut(&std::rc::Rc<ShellState>, &Painted, &Scene, Case<'_>)) {
    let mut ran = 0;
    for (size_name, size) in SIZES {
        for n in 0..STATES.len() {
            let owner = Owner::new();
            owner.run(|| {
                let state = use_shell_state();
                // Accumulate: state n is state n-1 plus one edit.
                for (_, edit) in &STATES[..=n] {
                    edit(&state);
                }
                let (shot, scene) = painted_at(*size);
                let name = format!("{} \u{2014} {size_name}", STATES[n].0);
                check(
                    &state,
                    &shot,
                    &scene,
                    Case {
                        name: &name,
                        state: n,
                        size: *size,
                    },
                );
            });
            ran += 1;
        }
    }
    assert_eq!(
        ran, CASES,
        "the sweep covered {ran} of {CASES} (state, size) cases",
    );
    assert_eq!(CASES, STATES.len() * SIZES.len());
}

/// The cards the board is showing: all of them, or the one a maximised board
/// leaves. Derived from the shell's own state rather than from the paint, so a
/// card that went missing is a failure and not a redefinition of the question.
fn shown_cards(state: &std::rc::Rc<ShellState>) -> Vec<String> {
    state.maximized.get().map_or_else(
        || {
            state
                .placed()
                .iter()
                .map(|c| c.id().as_str().to_owned())
                .collect()
        },
        |one| vec![one.id().as_str().to_owned()],
    )
}

// -- 1. Forward: everything the specification declares is painted -------------

/// R1668 — every element screen C declares is on the screen.
#[test]
fn r1668_every_declared_element_of_the_screen_is_painted() {
    sweep(|state, shot, _, case| {
        let mut wanted: Vec<String> = vec![
            "shell.appbar".into(),
            "shell.appbar.source".into(),
            "shell.appbar.capture".into(),
            "shell.appbar.search".into(),
            "shell.subbar".into(),
            "shell.subbar.preset".into(),
            "shell.subbar.edit".into(),
            "shell.subbar.add".into(),
            "shell.rail".into(),
            "shell.palette".into(),
        ];
        // The rail's seats and the palette's rows come from the specification's
        // own tables rather than from that list.
        for seat in spec::RAIL {
            wanted.push(format!("shell.rail.{}", seat.key));
        }
        for entry in spec::CATALOGUE {
            wanted.push(format!("shell.palette.{}", entry.kind));
        }
        // And the cards the board is showing. A maximised board shows exactly
        // one -- stated here rather than skipped, because "the other three are
        // legitimately absent" and "the other three went missing" are the same
        // observation until somebody writes down which is expected.
        let shown = shown_cards(state);
        if let Some(one) = state.maximized.get() {
            assert_eq!(
                shown,
                vec![one.id().as_str().to_owned()],
                "{case}: a maximised board shows the maximised card and nothing else",
            );
        } else {
            assert_eq!(
                shown.len(),
                state.placed().len(),
                "{case}: the board is not maximised and shows fewer cards than it holds",
            );
        }
        for id in &shown {
            wanted.push(format!("card.{id}"));
        }
        let missing: Vec<_> = wanted.iter().filter(|t| shot.rect(t).is_none()).collect();
        assert!(
            missing.is_empty(),
            "{case}: the screen does not paint {missing:?}",
        );
    });
}

/// R1668 — each placed card paints the body the specification gives its kind.
///
/// The check that a placeholder cannot pass, and the reason the four bodies
/// were written this round: a card drawn as a coloured swatch reproduces the
/// arrangement of screen C while reproducing none of screen C.
#[test]
fn r1668_each_placed_card_paints_the_body_its_kind_is_specified_to_have() {
    sweep(|state, shot, _, case| {
        for id in &shown_cards(state) {
            let id = id.as_str();
            let kind = super::kind_of(id);
            let rows = match kind {
                "packet" => spec::STREAM_ROWS.len(),
                "decode" => spec::DECODE_ROWS.len(),
                "keymap" => spec::MAP_ROWS.len(),
                "filter" => spec::FILTER_CHIPS.len(),
                other => panic!("{case}: {other} is placed and this check does not know it"),
            };
            let stem = match kind {
                "packet" => format!("card.{id}.row."),
                "decode" => format!("card.{id}.tree."),
                "keymap" => format!("card.{id}.map."),
                "filter" => format!("card.{id}.chip."),
                _ => unreachable!(),
            };
            let painted = shot.family(&stem).len();
            assert!(
                painted > 0,
                "{case}: card {id} paints none of its {rows} specified rows",
            );
            assert!(
                painted <= rows,
                "{case}: card {id} paints {painted} rows and the specification has {rows}",
            );
            // And what is IN each painted row is what the specification puts
            // there. A count alone passes on a card that paints the right
            // number of the WRONG rows, which is the failure mode a "reproduce
            // the reference" claim has.
            //
            // Two directions, and they are not the same claim. Every painted
            // word must be a specified one -- always, because a card that
            // invents a value is wrong at any size. Every specified word must
            // be painted only at the size and state the specification
            // describes: a card narrowed to one cell legitimately drops columns
            // from the right, and demanding them there would make the gate
            // report the screen's correct behaviour as a defect.
            //
            // ★ POSITIONALLY. The first draft asked only whether each painted
            // word was one of the row's specified values, and a counterfactual
            // that reversed a row's four columns passed it: the set was the
            // same, so "the timestamp is in the length column" was not a
            // failure. A value in the wrong column is exactly the defect a
            // reproduction claim has to exclude.
            let full = case.as_specified();
            for n in 0..painted {
                let words = run_words(shot, &format!("{stem}{n}"));
                let wanted = specified_row(kind, n);
                assert!(
                    words.len() <= wanted.len(),
                    "{case}: row {n} of card {id} paints {words:?} and the \
                     specification gives it {wanted:?}",
                );
                for (k, word) in words.iter().enumerate() {
                    assert_eq!(
                        word, &wanted[k],
                        "{case}: cell {k} of row {n} of card {id} paints {word:?} \
                         where the specification puts {:?}",
                        wanted[k],
                    );
                }
                if full {
                    assert_eq!(
                        words.len(),
                        wanted.len(),
                        "{case}: row {n} of card {id} paints {words:?} at the size \
                         the specification describes, and it specifies {wanted:?}",
                    );
                }
            }
        }
    });
}

/// The text runs the painted scene put inside one tag.
fn run_words(shot: &Painted, tag: &str) -> Vec<String> {
    shot.runs
        .iter()
        .filter(|(_, _, owner)| owner.as_deref() == Some(tag))
        .map(|(text, ..)| text.clone())
        .collect()
}

/// What the specification says row `n` of that kind's body holds.
///
/// The values only; where they sit is the painter's business and the contained
/// and disjoint checks are what hold that.
fn specified_row(kind: &str, n: usize) -> Vec<String> {
    match kind {
        "packet" => {
            let (time, ty, name, len) = spec::STREAM_ROWS[n];
            vec![time.into(), ty.into(), name.into(), len.into()]
        }
        "decode" => {
            let (_, key, value) = spec::DECODE_ROWS[n];
            if value.is_empty() {
                vec![key.into()]
            } else {
                vec![key.into(), value.into()]
            }
        }
        "keymap" => {
            let (id, resource, seen) = spec::MAP_ROWS[n];
            vec![id.into(), resource.into(), seen.into()]
        }
        "filter" => vec![spec::FILTER_CHIPS[n].0.into()],
        other => panic!("{other} has no specified body"),
    }
}

/// R1668 — the decode card lights exactly the bytes the specification says the
/// selected field occupies.
///
/// Screen B's law (R1663), held on screen C too. A card that lit a byte either
/// side would look right and be a different claim about where a value came
/// from, so the lit set is compared to the span rather than eyeballed.
#[test]
fn r1668_the_decode_card_lights_exactly_the_specified_bytes() {
    sweep(|state, shot, scene, case| {
        let Some(id) = shown_cards(state)
            .into_iter()
            .find(|id| super::kind_of(id) == "decode")
        else {
            return;
        };
        let (start, end) = spec::DECODE_SELECTED_SPAN;
        let painted = shot.family(&format!("card.{id}.byte."));
        // ★ Not a silent skip. A guard that returns when there is nothing to
        // check reads as coverage and runs no assertion (R1657.1), so the case
        // that must always have bytes says so out loud: at the size and the
        // span the specification gives the card, the pane is there.
        let bounds = shot.rect(&format!("card.{id}")).expect("shown");
        let pane_fits = 148.min(bounds.w / 2) >= 66;
        assert_eq!(
            !painted.is_empty(),
            pane_fits,
            "{case}: card {id} is {}px wide and paints {} byte cells",
            bounds.w,
            painted.len(),
        );
        if painted.is_empty() {
            return;
        }
        // Lit is a paint fact, so it is read off the paint: a byte cell whose
        // own fill is opaque is the lit one, and the specification's span is
        // what the set of them has to equal.
        let mut lit = BTreeSet::new();
        scene.for_each_node(&mut |visit| {
            let Some(tag) = visit.node.tag() else { return };
            let Some(index) = tag.strip_prefix(&format!("card.{id}.byte.")) else {
                return;
            };
            if let Scene::Container(node) = visit.node
                && node.style.fill.a == u8::MAX
                && let Ok(index) = index.parse::<usize>()
            {
                lit.insert(index);
            }
        });
        let wanted: BTreeSet<usize> = (start..end).collect();
        assert_eq!(
            lit, wanted,
            "{case}: card {id} lights {lit:?} and the specification's span is {wanted:?}",
        );
    });
}

// -- 2. Backward: nothing is on the screen that the specification does not own -

/// R1668 — every painted palette row and rail seat is one the specification
/// declares, and the counts on the screen are the specification's counts.
///
/// The direction that catches an invention. A screen that grew a row nobody
/// asked for passes every forward check ever written.
#[test]
fn r1668_the_screen_invents_no_seat_and_states_the_counts_it_specifies() {
    sweep(|state, shot, _, case| {
        let declared_kinds: BTreeSet<&str> = spec::CATALOGUE.iter().map(|w| w.kind).collect();
        for tag in shot.family("shell.palette.") {
            let kind = tag.trim_start_matches("shell.palette.");
            assert!(
                declared_kinds.contains(kind),
                "{case}: the palette paints a row {kind:?} the specification does not declare",
            );
        }
        assert_eq!(
            shot.family("shell.palette.").len(),
            spec::CATALOGUE.len(),
            "{case}: the palette paints a different number of rows than the catalogue has",
        );

        let declared_seats: BTreeSet<&str> = spec::RAIL.iter().map(|s| s.key).collect();
        for tag in shot.family("shell.rail.") {
            let key = tag.trim_start_matches("shell.rail.");
            // The account chip is chrome, not a destination.
            if key == "account" {
                continue;
            }
            assert!(
                declared_seats.contains(key),
                "{case}: the rail paints a seat {key:?} the specification does not declare",
            );
        }

        // The footer's two numbers, read off the painted runs rather than
        // recomputed: the screen's claim is the RELATION between them, so a
        // reader who is shown one of them is shown nothing.
        let footer: Vec<&str> = shot
            .runs
            .iter()
            .filter(|(_, _, owner)| owner.as_deref() == Some("shell.palette"))
            .map(|(text, ..)| text.as_str())
            .collect();
        let placed_line = format!(
            "{} placed of {}",
            state.placed().len(),
            spec::placeable_count()
        );
        let reserved_line = format!("{} reserved", spec::reserved_count());
        assert!(
            footer.contains(&placed_line.as_str()),
            "{case}: the palette's footer does not say {placed_line:?}; it says {footer:?}",
        );
        assert!(
            footer.contains(&reserved_line.as_str()),
            "{case}: the palette's footer does not say {reserved_line:?}; it says {footer:?}",
        );
    });
}

// -- 3. Reachable: a control answers for itself where it was painted ----------

/// R1668 — pressing the centre of a painted control resolves to that control.
///
/// The check R1653 was built around and the one a geometry helper cannot pass
/// on somebody else's behalf: the population is the painted scene, and the
/// question is put to the shell's own hit resolution.
#[test]
fn r1668_every_painted_control_answers_for_itself() {
    sweep(|state, shot, _, case| {
        // Only the size the screen is specified at: the hit test is written in
        // the specified coordinate space, and asking it about a window it was
        // not laid out for is a different (and separately reported) question.
        if case.size != (WIN_W, WIN_H) {
            return;
        }
        let mut probes: Vec<String> = Vec::new();
        for seat in spec::RAIL {
            probes.push(format!("shell.rail.{}", seat.key));
        }
        for entry in spec::CATALOGUE {
            probes.push(format!("shell.palette.{}", entry.kind));
        }
        for tag in [
            "shell.subbar.edit",
            "shell.subbar.add",
            "shell.subbar.preset",
        ] {
            probes.push(tag.to_owned());
        }
        for tag in probes {
            let Some(rect) = shot.rect(&tag) else {
                continue;
            };
            let (x, y) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
            let hit = Hit::at(state, x, y);
            let answered = super::hit_word(&hit);
            assert_eq!(
                answered, tag,
                "{case}: pressing the centre of {tag} at ({x}, {y}) resolves to {answered:?}",
            );
        }
    });
}

// -- 4. Contained: nothing is painted outside the box it belongs to -----------

/// R1668 — every painted mark lies inside the pane its address puts it in.
#[test]
fn r1668_every_mark_lies_inside_the_pane_its_address_names() {
    sweep(|_, shot, _, case| {
        for (stem, pane) in [
            ("shell.palette.", "shell.palette"),
            ("shell.rail.", "shell.rail"),
        ] {
            let Some(bounds) = shot.rect(pane) else {
                continue;
            };
            for tag in shot.family(stem) {
                let rect = shot.rect(tag).expect("just enumerated");
                assert!(
                    rect.x >= bounds.x
                        && rect.y >= bounds.y
                        && rect.x + rect.w <= bounds.x + bounds.w
                        && rect.y + rect.h <= bounds.y + bounds.h,
                    "{case}: {tag} at {rect:?} is painted outside {pane} at {bounds:?}",
                );
            }
        }
    });
}

/// R1668 — no card's content is painted outside its card.
///
/// The bodies this round wrote are drawn row by row from a table, and a table
/// longer than the card is the exact shape of the twenty-five-surface backlog
/// R1656 measured. The painter stops before the edge; this is what says so.
#[test]
fn r1668_no_card_paints_its_content_outside_itself() {
    sweep(|state, shot, _, case| {
        for id in &shown_cards(state) {
            let bounds = shot
                .rect(&format!("card.{id}"))
                .unwrap_or_else(|| panic!("{case}: card {id} is shown and not painted"));
            for tag in shot.family(&format!("card.{id}.")) {
                let rect = shot.rect(tag).expect("just enumerated");
                assert!(
                    rect.y + rect.h <= bounds.y + bounds.h,
                    "{case}: {tag} at {rect:?} runs off the bottom of its card at {bounds:?}",
                );
                assert!(
                    rect.x + rect.w <= bounds.x + bounds.w,
                    "{case}: {tag} at {rect:?} runs off the right of its card at {bounds:?}",
                );
            }
        }
    });
}

// -- 5. Disjoint: nothing is painted on top of anything ----------------------

/// R1668 — no two rows of one card overlap.
#[test]
fn r1668_no_two_rows_of_one_card_are_painted_over_each_other() {
    sweep(|state, shot, _, case| {
        for id in &shown_cards(state) {
            for stem in ["row.", "tree.", "map.", "chip.", "stat.", "bytes."] {
                let family = shot.family(&format!("card.{id}.{stem}"));
                let rects: Vec<(&str, Rect)> = family
                    .iter()
                    .map(|t| (*t, shot.rect(t).expect("just enumerated")))
                    .collect();
                for (n, (a_tag, a)) in rects.iter().enumerate() {
                    for (b_tag, b) in &rects[n + 1..] {
                        assert!(
                            a.x + a.w <= b.x
                                || b.x + b.w <= a.x
                                || a.y + a.h <= b.y
                                || b.y + b.h <= a.y,
                            "{case}: {a_tag} at {a:?} and {b_tag} at {b:?} overlap",
                        );
                    }
                }
            }
        }
    });
}

// -- 6. Reserved: the round's own law, on the painted screen ------------------

/// R1668 — every seat the specification reserves is painted **declared
/// unavailable, with its booking**, and no other seat is.
///
/// This is the check the screen exists for. A reserved seat drawn grey by hand
/// would pass a screenshot comparison and fail every part of this: the reason
/// is read out of the framework's own cascade census, so what the palette shows
/// and what `scene/disabled` reports are the same fact or this goes red.
#[test]
fn r1668_every_reserved_seat_is_declared_with_the_booking_it_states() {
    sweep(|_, shot, _, case| {
        for entry in spec::CATALOGUE {
            let tag = format!("shell.palette.{}", entry.kind);
            if shot.rect(&tag).is_none() {
                continue;
            }
            let inert = shot.inert.get(&tag);
            match entry.tier {
                spec::Tier::Reserved => {
                    let (kind, detail, recourse) = inert.unwrap_or_else(|| {
                        panic!("{case}: {tag} is reserved and the screen paints it live")
                    });
                    assert_eq!(
                        *kind,
                        UnavailableKind::Reserved,
                        "{case}: {tag} is inert as {kind:?} rather than as a reservation",
                    );
                    assert_eq!(
                        detail, entry.reserved_for,
                        "{case}: {tag} reports a booking the specification does not state",
                    );
                    assert_eq!(
                        *recourse,
                        Recourse::AwaitRelease,
                        "{case}: {tag} offers the wrong recourse for a reservation",
                    );
                }
                spec::Tier::Placeable => assert!(
                    inert.is_none(),
                    "{case}: {tag} is placeable and the screen paints it inert as {inert:?}",
                ),
            }
        }
        // The rail's two locked seats, the same way.
        for seat in spec::RAIL {
            let tag = format!("shell.rail.{}", seat.key);
            let inert = shot.inert.get(&tag);
            match seat.reserved_for {
                Some(why) => {
                    let (kind, detail, _) = inert.unwrap_or_else(|| {
                        panic!("{case}: the {} seat is reserved and painted live", seat.key)
                    });
                    assert_eq!(*kind, UnavailableKind::Reserved);
                    assert_eq!(
                        detail, why,
                        "{case}: the {} seat's booking drifted",
                        seat.key
                    );
                }
                None => assert!(
                    inert.is_none(),
                    "{case}: the {} seat is open and painted inert",
                    seat.key
                ),
            }
        }
    });
}

/// R1668 — the reserved seats are exactly the specification's nine, counted on
/// the painted screen rather than assumed.
///
/// The half the check above cannot give: it walks the specification and would
/// be silent about a *tenth* inert palette row that the specification never
/// mentioned.
#[test]
fn r1668_the_screen_paints_exactly_the_reserved_seats_it_specifies() {
    sweep(|_, shot, _, case| {
        let inert_rows: Vec<&String> = shot
            .inert
            .keys()
            .filter(|t| t.starts_with("shell.palette."))
            .collect();
        assert_eq!(
            inert_rows.len(),
            spec::reserved_count(),
            "{case}: the palette paints {} inert rows and reserves {}: {inert_rows:?}",
            inert_rows.len(),
            spec::reserved_count(),
        );
        let inert_seats = shot
            .inert
            .keys()
            .filter(|t| t.starts_with("shell.rail."))
            .count();
        assert_eq!(
            inert_seats,
            spec::RAIL
                .iter()
                .filter(|s| s.reserved_for.is_some())
                .count(),
            "{case}: the rail paints a different number of locked seats than it declares",
        );
    });
}

/// R1668 — a reserved seat cannot be placed by **any** path the shell offers.
///
/// Three paths reach the board: the palette press, the invoke verb, and the
/// rail. A declaration that made only the first inert would leave an agent able
/// to do what a person cannot, which is the asymmetry §2 #2 exists to forbid.
#[test]
fn r1668_no_path_places_a_reserved_seat() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let before = state.placed().len();
        for entry in spec::CATALOGUE
            .iter()
            .filter(|w| w.tier == spec::Tier::Reserved)
        {
            let refusal = ShellOracle::add(&state, entry.kind)
                .expect_err("a reserved kind is not placed by the invoke verb");
            let said = format!("{refusal:?}");
            assert!(
                said.contains(entry.reserved_for),
                "{}: the refusal is {said:?} and does not name the booking",
                entry.kind,
            );
        }
        assert_eq!(
            state.placed().len(),
            before,
            "a reserved kind reached the board anyway",
        );

        // And the reserved rail seats do not become the current section.
        let was = state.nav.get();
        for seat in spec::RAIL.iter().filter(|s| s.reserved_for.is_some()) {
            ShellOracle::act_on_hit(&state, Hit::Rail(seat.key));
            assert_eq!(
                state.nav.get(),
                was,
                "the {} seat is reserved and pressing it navigated there",
                seat.key,
            );
        }
    });
}
