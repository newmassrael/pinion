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
use pinion_core::external::{ExternalIntrospect, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::widgets::destination::Required;
use pinion_core::{Frame, Scene};
use pinion_screen::ScreenState;

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
    // ★ R1671 — publish the size through the channel the SHELL reads, which is
    // the state's own signal. It used to be `VIEWPORT_SIZE`, and that was a
    // second source: the shell's invoke path has no Owner scope, so it could
    // not read that one at all and answered about the design size while the
    // paint had moved. Driving the one channel is what makes this sweep able to
    // see a resize at all -- and `r1671_the_screen_fills_the_window_it_was_given`
    // is red the moment this line addresses anything else.
    let owner = Owner::current().expect("the sweep runs inside a scope");
    pinion_core::reactive::VIEWPORT_SIZE
        .resolve(&owner)
        .set(size);
    // ★★ R1700 — and the FRAMEWORK's record, which is the channel a caller off
    // a view scope reads. `announce_external_sizes` writes it after every real
    // paint from the surface's own rectangle; this sweep does not run that
    // pass, so it announces what the pass would have. R1671 published the size
    // through a signal on this screen's state instead — a private channel that
    // only this screen's sweep could drive, and one more thing the harness could
    // resolve that production could not.
    pinion_core::external::record_surface_size(super::VIEW_TAG, size.0, size.1);
    let mut scene = super::view(ScreenState::default(), Frame::default());
    let mut cache = pinion_runtime::LayoutCache::new();
    pinion_runtime::compute_layout(&mut scene, &mut cache, size.0, size.1);
    // ★★★★★ R1761 — and what this frame PAINTED, exactly as the window records
    // it on the pass that announces the sizes. This screen judges its own paint
    // now (`crate::judge`), so a sweep that skipped this line would be asking
    // the verdict of a store this frame never filled — and would get the
    // previous case's answer, which is worse than none.
    assert_eq!(
        pinion_runtime::record_painted_surfaces(&scene, &[super::VIEW_TAG]),
        1,
        "the shell is painted, or the verdict below is asked of a store this \
         frame never filled",
    );
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
            let (family, rows) = body_family(kind)
                .unwrap_or_else(|| panic!("{case}: {kind} is placed and unknown here"));
            let stem = format!("card.{id}.{family}.");
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

/// The tag family a kind's body rows are painted under, and how many rows the
/// specification gives it.
///
/// One table, read by the body check and by the clamp check next to it: two
/// copies of "which family is a stream row in" is two things that can disagree
/// about which clamp was exercised.
fn body_family(kind: &str) -> Option<(&'static str, usize)> {
    Some(match kind {
        "packet" => ("row", spec::STREAM_ROWS.len()),
        "decode" => ("tree", spec::DECODE_ROWS.len()),
        "keymap" => ("map", spec::MAP_ROWS.len()),
        "filter" => ("chip", spec::FILTER_CHIPS.len()),
        _ => return None,
    })
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
        // ★ R1671 — the lit set is exactly the span's bytes THAT ARE DRAWN.
        // A narrowed pane drops byte columns from the right, so demanding the
        // whole span would report the painter's correct clipping as a defect --
        // and demanding nothing would let a card light a byte the span does not
        // contain. Both directions are wanted, and the drawn set is read from
        // the paint rather than assumed.
        let drawn: BTreeSet<usize> = painted
            .iter()
            .filter_map(|t| t.rsplit('.').next().and_then(|n| n.parse::<usize>().ok()))
            .collect();
        let wanted: BTreeSet<usize> = (start..end).filter(|n| drawn.contains(n)).collect();
        assert!(
            !wanted.is_empty(),
            "{case}: card {id} draws none of the span's bytes, so this proves nothing",
        );
        assert_eq!(
            lit, wanted,
            "{case}: card {id} lights {lit:?}; the span's drawn bytes are {wanted:?}",
        );
    });
}

/// R1669 — the sweep reaches **both sides** of every clamp the painters carry.
///
/// ★ The debt this closes was found by a counterfactual that PASSED. R1668
/// deleted the stream body's "a row that would leave the card is not painted"
/// guard and every gate stayed green — not because the guard was wrong, but
/// because **no swept state was ever small enough for it to fire**. A guard
/// whose true branch nothing reaches is untested code that reads as covered,
/// and adding the state that reaches it produced five real defects in the next
/// ten minutes.
///
/// A test cannot ask whether a branch was taken. What it CAN ask is whether the
/// OUTCOME that branch exists to produce was observed — a body painting fewer
/// rows than the specification gives it — and whether the other outcome was
/// observed too, so "always truncated" cannot pass as coverage either. That is
/// what this asserts, over a population derived from `spec` rather than from a
/// list of clamps somebody maintains beside them.
///
/// The residue, stated: a clamp whose outcome is not "fewer rows than
/// specified" is not covered by this. The two in that class are named below and
/// asked about directly.
#[test]
fn r1669_the_sweep_reaches_both_sides_of_every_clamp() {
    /// What the sweep saw of one observable: its full form, and its clamped one.
    #[derive(Default, Clone, Copy)]
    struct Sides {
        full: bool,
        clamped: bool,
    }

    let mut seen: BTreeMap<String, Sides> = BTreeMap::new();
    let mut note = |what: String, clamped: bool| {
        let side = seen.entry(what).or_default();
        if clamped {
            side.clamped = true;
        } else {
            side.full = true;
        }
    };

    sweep(|state, shot, _, _| {
        for id in &shown_cards(state) {
            let kind = super::kind_of(id);
            if shot.rect(&format!("card.{id}")).is_none() {
                continue;
            }
            let Some((family, rows)) = body_family(kind) else {
                continue;
            };
            // The rows themselves.
            let painted = shot.family(&format!("card.{id}.{family}.")).len();
            note(format!("{kind}: rows"), painted < rows);
            // And the CELLS of each painted row, which is a second clamp with
            // the same shape: a column too narrow to say anything is dropped.
            //
            // ★ Only for a row that HAS a column to lose. A one-cell row cannot
            // be clamped and remain a row -- dropping its only cell is the row
            // going, which is the observable above. Derived rather than
            // excluded by name: this gate found the filter card's chips that
            // way and made the reason be stated instead of assumed.
            for n in 0..painted {
                let wanted = specified_row(kind, n).len();
                if wanted < 2 {
                    continue;
                }
                let cells = run_words(shot, &format!("card.{id}.{family}.{n}")).len();
                note(format!("{kind}: cells"), cells < wanted);
            }
            // The two all-or-nothing clamps, asked about by name because their
            // outcome is a presence rather than a count.
            if kind == "decode" {
                note(
                    "decode: byte pane".to_owned(),
                    shot.family(&format!("card.{id}.byte.")).is_empty(),
                );
            }
            if kind == "filter" {
                note(
                    "filter: stat tiles".to_owned(),
                    shot.family(&format!("card.{id}.stat.")).is_empty(),
                );
            }
        }
    });

    // The population is what the sweep produced, and it must not be empty --
    // a derivation that quietly yields nothing is the failure this whole
    // module's populations are written to avoid (R1651.1).
    assert!(
        seen.len() >= 9,
        "the sweep observed only {} clamp outcome(s): {:?}",
        seen.len(),
        seen.keys().collect::<Vec<_>>(),
    );
    let unreached: Vec<&String> = seen
        .iter()
        .filter(|(_, side)| !side.clamped)
        .map(|(what, _)| what)
        .collect();
    assert!(
        unreached.is_empty(),
        "no swept state reaches the clamped side of {unreached:?} — the guard \
         is there and nothing exercises it, so deleting it would change nothing \
         and no gate would say so",
    );
    let never_full: Vec<&String> = seen
        .iter()
        .filter(|(_, side)| !side.full)
        .map(|(what, _)| what)
        .collect();
    assert!(
        never_full.is_empty(),
        "the sweep never sees {never_full:?} unclamped, so 'always truncated' \
         would pass every check above it",
    );
}

/// R1671 — the screen FILLS the window it was given.
///
/// ★★ Reported by a person maximising the window: the content stayed exactly
/// where it was, painted at the size the screen opens in, with the rest of the
/// window empty. And this module's sweep already laid the screen out at a
/// maximised size and every check passed — because every check compares the
/// screen to ITSELF (is this mark inside its pane, do these rows overlap), and
/// all of those stay true when the whole screen ignores the window. R1654 wrote
/// that sentence down about screen A: "every check ran the layout at WIN_W x
/// WIN_H, so the assumption and the defect were the same number". Screen C had
/// the sizes in its sweep and no check that compared the paint to the WINDOW.
///
/// Screens A and B read `use_viewport_size`; this one did not, which is the
/// whole of the defect and the reason the check belongs here rather than in a
/// shared harness.
#[test]
fn r1671_the_screen_fills_the_window_it_was_given() {
    sweep(|_, shot, _, case| {
        let (w, h) = case.size;
        // The three top-level panes are what the window is divided into, so
        // their union IS the screen's extent. Derived from the paint rather
        // than from the shell's constants -- a constant would agree with the
        // defect.
        let mut right = 0;
        let mut bottom = 0;
        for pane in [
            "shell.appbar",
            "shell.rail",
            "shell.palette",
            "shell.subbar",
        ] {
            if let Some(r) = shot.rect(pane) {
                right = right.max(r.x + r.w);
                bottom = bottom.max(r.y + r.h);
            }
        }
        assert_eq!(
            right, w,
            "{case}: the screen paints {right}px wide in a {w}px window",
        );
        assert_eq!(
            bottom, h,
            "{case}: the screen paints {bottom}px tall in a {h}px window",
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
        // ★ R1694 — the panel now paints three more KINDS of addressable thing:
        // a heading per section, and the two counts. They are here rather than
        // excluded by name because the whole point of a backward check is that
        // it enumerates everything the screen paints, and an exclusion list is
        // where an invention hides.
        let declared: BTreeSet<String> = spec::CATALOGUE
            .iter()
            .map(|w| format!("shell.palette.{}", w.kind))
            .chain(
                spec::SECTIONS
                    .iter()
                    .map(|(key, _, _)| format!("shell.palette.section.{key}")),
            )
            .chain([
                "shell.palette.placed".to_owned(),
                "shell.palette.reserved".to_owned(),
            ])
            // ★ R1761 — and the panel's own heading, which this backward check
            // is what found: both lines were loose ink until the dashboard's
            // surfaces were written down, so neither could be addressed and
            // neither could be judged. The population is the dashboard
            // specification's own `palette_head` roster, for the reason the
            // rows below take theirs from `palette_row` — a part named only in
            // this file could not exist.
            .chain(
                spec::dashboard_document()
                    .canon("palette_head")
                    .expect("the dashboard specification declares the panel's heading")
                    .parts()
                    .iter()
                    .map(|part| format!("{}{}", super::PALETTE_HEAD, part.key))
                    .collect::<Vec<_>>(),
            )
            // ★ R1733 — and every PART of every row a widget can be picked up
            // from. The population is the board specification's own
            // `palette_row` roster crossed with the PLACEABLE catalogue, so a
            // part added to the paint and not to the specification fails here
            // as well as at the conformance check — and a part named only in
            // this file could not exist. Reserved rows have no parts, for the
            // two reasons `part_tag_of` states.
            .chain(
                spec::board_document()
                    .canon("palette_row")
                    .expect("the board specification declares a palette row")
                    .parts()
                    .iter()
                    .flat_map(|part| {
                        spec::CATALOGUE
                            .iter()
                            .filter(|w| w.tier == spec::Tier::Placeable)
                            .map(move |w| super::part_tag(part.key.as_ref(), w.kind))
                    }),
            )
            .collect();
        for tag in shot.family("shell.palette.") {
            assert!(
                declared.contains(tag),
                "{case}: the palette paints {tag:?}, which the specification does not declare",
            );
        }
        assert_eq!(
            shot.family("shell.palette.").len(),
            declared.len(),
            "{case}: the palette paints a different number of regions than the \
             catalogue, its sections and its two counts come to",
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
        // ★ R1671 — at EVERY size. This used to skip all but the opening one,
        // on the ground that "the hit test is written in the specified
        // coordinate space" — which was true, and was the defect: the paint
        // followed the window and the gesture did not, so a maximised screen
        // resolved 20 of 24 probes to the wrong control or to nothing. The skip
        // was the gate agreeing with the bug. Both halves read one fact now,
        // and this is what holds them there.
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

/// R1671 — nothing a card paints may cross the card's own FRAME.
///
/// ★★ Reported by a person looking at the window, twice: the stream's header
/// strip sat on the card's outline, leaving a gap in it. The first repair inset
/// the body and the outline was still eaten, because the culprit is an
/// UNTAGGED fill — and every check in this module addresses nodes by tag, so
/// none of them could see it. `scene/containment` cannot either: it compares a
/// mark against its owner's BOX, and a border is ink the box owns inside that
/// box, so painting over it is by that definition contained
/// ([[debt-a-child-may-paint-over-its-owners-border]] pins that in the
/// framework).
///
/// So this walks every painted node, tagged or not, and holds it to the card's
/// **content** rectangle — the box less its border.
/// ★★ R1672 — the preset menu is a popup: **anchored** to the chip that opens
/// it, **bounded** by the window rather than by the bar.
///
/// It was a child of the sub bar and hung 81 pixels below it. Moving it to the
/// window's own layer is what makes that an honest tree — but the move alone is
/// not checkable, and this is the trap this session kept walking into: the
/// paint and the hit test both read `preset_item_rect`, so a change to that
/// function moves BOTH and no assertion comparing them can notice. The shape
/// has to be pinned against something that is not itself, and the something is
/// the chip: the menu hangs under it, lines up with it, and stays inside the
/// window.
#[test]
fn r1672_the_preset_menu_hangs_under_the_chip_that_opens_it() {
    let mut open_cases = 0;
    sweep(|state, shot, _, case| {
        if !state.preset_open.get() {
            assert!(
                shot.rect("shell.preset.menu").is_none(),
                "{case}: the menu is closed and still painted",
            );
            return;
        }
        open_cases += 1;
        let chip = shot
            .rect("shell.subbar.preset")
            .unwrap_or_else(|| panic!("{case}: the chip that opens the menu is painted"));
        let menu = shot
            .rect("shell.preset.menu")
            .unwrap_or_else(|| panic!("{case}: the open menu is painted"));
        // The panel's own top overlaps the chip by design — the menu's heading
        // band sits over the bottom of the control that opened it, which is how
        // it reads as belonging to it. What has to be BELOW the chip is the
        // first thing a person can press, or the menu covers its own trigger.
        let first = shot
            .rect("shell.preset.item.0")
            .unwrap_or_else(|| panic!("{case}: the open menu paints its first row"));
        assert!(
            first.y >= chip.y + chip.h,
            "{case}: the first row at {first:?} is not below the chip at {chip:?}",
        );
        assert!(
            menu.x + 16 >= chip.x && menu.x <= chip.x + 16,
            "{case}: the menu at {menu:?} is not lined up with {chip:?}",
        );
        assert!(
            menu.x + menu.w <= case.size.0 && menu.y + menu.h <= case.size.1,
            "{case}: the menu at {menu:?} leaves the {:?} window",
            case.size,
        );
    });
    assert!(open_cases > 0, "no swept state opens the menu");
}

/// ★★ R1672 — a header too narrow for its own chrome gives way IN ORDER, and
/// the sweep reaches both sides of that.
///
/// The give-way is this round's mechanism and a mechanism nothing exercises on
/// both sides is a branch nobody has run ([[r1669-a-clamp-says-which-case-
/// reaches-it]]). The two floors below are what make this a coverage claim
/// rather than a sample: some case must paint a card's **whole** affordance
/// strip, and some case must paint **fewer** than a card offers — otherwise the
/// per-case assertion is true of a screen where the give-way never happens or
/// never stops happening.
///
/// The per-case assertion is that what is painted is a **suffix** of what the
/// card offers: dropping from the left keeps the last-declared control nearest
/// the edge a hand reaches for, and a hole in the middle would mean the strip's
/// positions and its contents had come apart.
#[test]
fn r1672_a_narrow_header_drops_affordances_from_the_left() {
    let mut whole = 0;
    let mut reduced = 0;
    sweep(|state, shot, _, case| {
        for id in shown_cards(state) {
            let Some(card) = state.card(&id) else {
                continue;
            };
            let offered: Vec<String> = card
                .chrome()
                .offered()
                .iter()
                .map(|a| a.wire().to_owned())
                .collect();
            if offered.is_empty() {
                continue;
            }
            let painted: Vec<String> = offered
                .iter()
                .filter(|wire| shot.rect(&format!("card.{id}.{wire}")).is_some())
                .cloned()
                .collect();
            assert_eq!(
                painted,
                offered[offered.len() - painted.len()..],
                "{case}: `{id}` painted {painted:?} of {offered:?} — the strip \
                 gives way from the LEFT, so what is left is a suffix",
            );
            if painted.len() == offered.len() {
                whole += 1;
            } else {
                reduced += 1;
            }
        }
    });
    assert!(whole > 0, "no case painted a card's whole strip");
    assert!(
        reduced > 0,
        "no case narrowed a card enough to drop one — the give-way branch is \
         never reached, so this sweep says nothing about it",
    );
}

/// ★★ R1672 — every painted mark is inside the box that OWNS it, ink and all.
///
/// The two checks around this one ask about *rectangles in the scene*: whether
/// a tag lands inside the pane its address names, and whether a card's parts
/// cross the card's frame. Neither can see **ink**, which is a measurement and
/// not something the scene holds — a label whose glyphs run three pixels past
/// the box the view gave it is inside its rectangle by every question above.
///
/// Screen A has asked this since R1656. This screen and screen B never did, and
/// R1672 measured what that cost: the check's metric had been copied into
/// screen B without the check, so a break that put its panes over their panels'
/// outlines was caught by nothing. The metric now comes from the crate
/// ([`pinion_core::test_fixtures::screen_ink`]) and all three screens ask.
#[test]
fn r1672_every_painted_mark_is_inside_the_box_that_owns_it() {
    use pinion_core::test_fixtures::screen_ink::assert_contained_ink;
    let mut below = 0;
    let mut weighed = 0;
    sweep(|_, shot, scene, case| {
        weighed += shot.runs.len();
        below += assert_contained_ink(case.name, scene, case.size);
    });
    // A floor on the SWEEP, for the reason `sweep` itself carries one (it pins
    // the case count): a check whose every mark took the off-window exemption
    // asserted nothing and reads afterwards as coverage. Bounded by what was
    // actually weighed rather than by a number picked here.
    assert!(weighed > 0, "the sweep found no painted runs to weigh");
    assert!(
        below < weighed,
        "{below} of {weighed} mark(s) took the off-window exemption — that is \
         the whole screen, so this gate weighed nothing",
    );
}

#[test]
fn r1671_nothing_a_card_paints_crosses_its_own_frame() {
    sweep(|state, shot, scene, case| {
        let cards: BTreeMap<String, Rect> = shown_cards(state)
            .into_iter()
            .filter_map(|id| shot.rect(&format!("card.{id}")).map(|r| (id, r)))
            .collect();
        if cards.is_empty() {
            return;
        }
        let mut crossing: Vec<(String, String, Rect)> = Vec::new();
        // ★ How many opaque marks the walk actually WEIGHED. Without it a gate
        // that stopped looking -- a predicate that never matches, a population
        // that derives to nothing -- passes and reads as coverage. Two rounds
        // running, a counterfactual found exactly that in a gate this session
        // wrote, so this one carries its own floor.
        let mut weighed = 0_usize;
        scene.for_each_node(&mut |visit| {
            let Some(rect) = visit.absolute_rect() else {
                return;
            };
            if rect.w == 0 || rect.h == 0 {
                return;
            }
            // Only nodes that PAINT: a grouping container with no fill draws
            // nothing over the frame, and holding one to the content box would
            // report the card's own body wrapper.
            let opaque = match visit.node {
                Scene::Box(n) => n.style.fill.a > 0,
                Scene::Container(n) => n.style.fill.a > 0,
                _ => false,
            };
            if !opaque {
                return;
            }
            for (id, card) in &cards {
                let tag = format!("card.{id}");
                // The card itself, and anything outside it, are not its
                // content: this is about what a card paints INSIDE itself.
                if visit.node.tag() == Some(tag.as_str()) {
                    continue;
                }
                let inside = rect.x >= card.x
                    && rect.y >= card.y
                    && rect.x + rect.w <= card.x + card.w
                    && rect.y + rect.h <= card.y + card.h;
                if !inside {
                    continue;
                }
                weighed += 1;
                let content = Rect::new(
                    card.x + super::CARD_FRAME,
                    card.y + super::CARD_FRAME,
                    card.w.saturating_sub(super::CARD_FRAME * 2),
                    card.h.saturating_sub(super::CARD_FRAME * 2),
                );
                if rect.x < content.x
                    || rect.y < content.y
                    || rect.x + rect.w > content.x + content.w
                    || rect.y + rect.h > content.y + content.h
                {
                    crossing.push((
                        tag,
                        visit.node.tag().unwrap_or("<untagged>").to_owned(),
                        rect,
                    ));
                }
            }
        });
        assert!(
            weighed >= cards.len(),
            "{case}: the walk weighed {weighed} opaque mark(s) inside {} card(s), \
             which is too few to have looked at anything",
            cards.len(),
        );
        assert!(
            crossing.is_empty(),
            "{case}: {} mark(s) paint over their card's frame: {:?}",
            crossing.len(),
            &crossing[..crossing.len().min(6)],
        );
    });
}

/// R1671 — the frame gate's own negative control.
///
/// ★★ Four counterfactuals against this round PASSED, and the reason was the
/// same each time: a gate that only ever runs against correct code cannot be
/// shown to work. Removing the untagged half of the walk above broke nothing,
/// because with the screen repaired there is nothing untagged crossing a frame
/// to miss. So the crossing is CONSTRUCTED here, untagged, and the walk has to
/// find it.
///
/// The scene is synthetic on purpose: the property is about the WALK, and
/// building it out of the real screen would make this a second copy of that
/// screen instead of a test of the instrument.
#[test]
fn r1671_the_frame_walk_finds_an_untagged_crossing() {
    use pinion_core::scene::{BoxNode, ContainerNode};
    use pinion_core::style::{BoxStyle, Color};

    /// Every opaque mark inside `card` that reaches past its frame, by the same
    /// rule the sweep applies.
    fn crossings(scene: &Scene, card: Rect) -> usize {
        let content = Rect::new(
            card.x + super::CARD_FRAME,
            card.y + super::CARD_FRAME,
            card.w.saturating_sub(super::CARD_FRAME * 2),
            card.h.saturating_sub(super::CARD_FRAME * 2),
        );
        let mut found = 0;
        scene.for_each_node(&mut |visit| {
            let Some(rect) = visit.absolute_rect() else {
                return;
            };
            let opaque = match visit.node {
                Scene::Box(n) => n.style.fill.a > 0,
                Scene::Container(n) => n.style.fill.a > 0,
                _ => false,
            };
            if !opaque || rect.w == 0 || rect.h == 0 || visit.node.tag() == Some("card") {
                return;
            }
            if rect.x < card.x || rect.x + rect.w > card.x + card.w {
                return; // outside the card is a different question
            }
            if rect.x < content.x || rect.x + rect.w > content.x + content.w {
                found += 1;
            }
        });
        found
    }

    let card = Rect::new(0, 0, 100, 40);
    let fill = BoxStyle::filled(Color::rgb(0x30, 0x30, 0x30));
    // An UNTAGGED strip at exactly the card's width: the shape the person saw.
    let mut over = ContainerNode::new(vec![Scene::Box(BoxNode::new(
        Rect::new(0, 10, 100, 12),
        fill.clone(),
    ))]);
    over.rect = card;
    over.tag = Some("card".to_owned().into());
    assert_eq!(
        crossings(&Scene::Container(over), card),
        1,
        "the walk must find an untagged mark at the card's full width",
    );

    // And the inset one is not a crossing, so the rule is not simply "anything".
    let mut inside = ContainerNode::new(vec![Scene::Box(BoxNode::new(
        Rect::new(super::CARD_FRAME, 10, 100 - super::CARD_FRAME * 2, 12),
        fill,
    ))]);
    inside.rect = card;
    inside.tag = Some("card".to_owned().into());
    assert_eq!(
        crossings(&Scene::Container(inside), card),
        0,
        "a band inset by the frame is not a crossing",
    );
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
        // ★ R1695 — the rail's locked seats, the same way, and now there are
        // five rather than two: three destinations this application cannot take
        // you to were painted live and refused nothing. The reason is read from
        // the ROSTER, so the seat's paint, its refusal and its accessibility
        // node cannot say three different things.
        let roster = spec::destinations();
        for seat in spec::RAIL {
            let tag = format!("shell.rail.{}", seat.key);
            let inert = shot.inert.get(&tag);
            match roster.get(seat.key).and_then(|d| d.standing.why()) {
                Some(why) => {
                    let (kind, detail, recourse) = inert.unwrap_or_else(|| {
                        panic!("{case}: the {} seat is closed and painted live", seat.key)
                    });
                    assert_eq!(*kind, why.kind(), "{case}: the {} seat's kind", seat.key);
                    assert_eq!(
                        detail,
                        why.detail(),
                        "{case}: the {} seat's booking drifted",
                        seat.key
                    );
                    assert_eq!(
                        *recourse,
                        why.recourse(),
                        "{case}: the {} seat offers the wrong recourse",
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
        // ★ R1733 — a ROW, by the shape of its name: `shell.palette.<kind>`,
        // with nothing further under it. Its parts are addressed
        // `shell.palette.part.<what>.<kind>` and the disabled cascade reaches
        // them too, so counting everything under the stem counted a reserved
        // row five times. A shape rather than a list of exclusions, so an
        // invented row is still caught — which is what this check is for.
        let inert_rows: Vec<&String> = shot
            .inert
            .keys()
            .filter(|t| {
                t.strip_prefix("shell.palette.")
                    .is_some_and(|rest| !rest.contains('.'))
            })
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
            spec::destinations().closed().count(),
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

        // And a rail seat this application cannot take you to does not become
        // the current section — five of them now, not two.
        let was = state.at();
        let roster = spec::destinations();
        for (destination, _) in roster.closed() {
            let key = spec::RAIL
                .iter()
                .find(|seat| seat.key == destination.key)
                .expect("the roster is built from the rail")
                .key;
            ShellOracle::act_on_hit(&state, Hit::Rail(key));
            assert_eq!(
                state.at(),
                was,
                "the {key} seat is closed and pressing it navigated there",
            );
        }
    });
}

/// ★★★ R1674 — the bands a card DECLARES are the bands its painter PLACES.
///
/// The round's whole rule, asserted at a consumer: `card_style` publishes the
/// chrome and `header_rect` / `edit_bar_rect` place the marks, and the two are
/// separate expressions of one fact. Nothing forced them to agree, and the
/// first draft did not — three edit bars sat one pixel above the footer band
/// the card had just declared for them, because the placement reserves the
/// WIDER of the two border widths (constant, so rows do not shift on selection)
/// while the declaration subtracted the one actually drawn.
///
/// A pixel, and it took the containment check to find it. That is the argument
/// for asserting the identity rather than the consequence: the consequence is
/// one overhang in one state, and the identity holds in all four.
///
/// The comparison is against the framework's own arithmetic
/// ([`pinion_core::containment::content_of`]) rather than against a re-derived
/// rectangle here, for the reason R1673 recorded when it hand-inverted
/// `line_box` and the sweep refuted it on the first run: a copy of a function
/// written beside it is free to disagree with it.
#[test]
fn r1674_the_declared_bands_are_the_placed_bands() {
    use pinion_core::containment::content_of;

    for selected in [false, true] {
        for editing in [false, true] {
            let palette = super::palette_of(&pinion_core::theme::Theme::dark(), true);
            let style = super::card_style(palette, selected, editing);
            let card = Rect::new(0, 0, 320, 240);
            let border = style.border.expect("a card is outlined in every state");

            // The header band, as the chrome says it is.
            let after_header = content_of(card, Some(&border), &style.chrome[..1]);
            let placed_header = super::header_rect(card);
            assert_eq!(
                after_header.y,
                placed_header.y + placed_header.h,
                "selected={selected} editing={editing}: the content starts where \
                 the placed header ends",
            );

            // And the footer band, in the mode that has one.
            let content = content_of(card, Some(&border), &style.chrome);
            if editing {
                let bar = super::edit_bar_rect(card);
                assert_eq!(
                    content.y + content.h,
                    bar.y,
                    "selected={selected}: the content ends where the placed \
                     edit bar begins",
                );
                assert_eq!(style.chrome.len(), 2, "header and footer");
            } else {
                assert_eq!(
                    style.chrome.len(),
                    1,
                    "no edit bar outside layout-edit mode, so no footer band",
                );
            }

            // ★ The negative control. Without it this file would pass with a
            // `card_style` that declared nothing at all: `content_of` with an
            // empty band list is the box less the border, and the assertions
            // above would then be comparing the header's placement with
            // itself in a different spelling.
            let undeclared = content_of(card, Some(&border), &[]);
            assert!(
                undeclared.y < after_header.y,
                "the declaration has to MOVE the content rectangle, or it is \
                 not being read",
            );
        }
    }
}

// --- R1695: arriving is not highlighting -------------------------------------

/// Paint the screen at `destination`, at the size it opens in.
///
/// A sweep of its own rather than a fifth axis on [`sweep`], because every check
/// in this file above is a question about the dashboard and would have to learn
/// to skip itself. What the destinations need is a different question.
fn painted_at_destination(destination: &str) -> Painted {
    let state = use_shell_state();
    if destination != state.at() {
        state
            .go(destination)
            .unwrap_or_else(|why| panic!("{destination} is open and refused: {why:?}"));
    }
    painted_at((WIN_W, WIN_H)).0
}

/// ★★★★★ R1721 — **a press at the centre of a painted saved-filter chip reaches
/// that chip.**
///
/// Measured before the arm this checks existed, by driving the running screen: the
/// five chips announced `checked`, a pointer press over every one of them changed
/// nothing, and the press landed on the card instead. The paint and the hit test
/// read ONE geometry (`filter_chip_rects`), which is the standing rule on this
/// screen — `debt-paint-and-gesture-read-two-facts` is open in this project
/// precisely because a control drawn where it cannot be pressed is what happens
/// when those are two functions.
#[test]
fn r1721_a_press_at_a_painted_chip_reaches_that_chip() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let shot = painted_at_destination("dashboard");
        let mut reached = 0;
        for n in 0..spec::FILTER_CHIPS.len() {
            let tag = format!("card.filter#3.chip.{n}");
            let rect = shot
                .rect(&tag)
                .unwrap_or_else(|| panic!("{tag} is announced and must be painted"));
            let hit = Hit::at(&state, rect.x + rect.w / 2, rect.y + rect.h / 2);
            assert_eq!(
                super::hit_word(&hit),
                tag,
                "a press at the centre of {tag} must reach {tag}, not {:?}",
                super::hit_word(&hit),
            );
            reached += 1;
        }
        assert_eq!(
            reached,
            spec::FILTER_CHIPS.len(),
            "every painted chip was pressed"
        );
    });
}

/// ★★★★★ R1695 — **every open destination is a place you arrive at, and every
/// closed one says why you did not.**
///
/// The check this screen did not have, and the reason it did not: the rail's
/// only consequence was a string it highlighted itself from, so "the press
/// worked" and "the window changed" were different facts and nothing compared
/// them. Driven through this screen's own hit path and measured before the
/// repair, four of the seven seats moved the string and left the painted scene
/// at **193 tagged regions before and 193 after**.
///
/// The floor cannot ask this at all. Measured by building a probe against the
/// reference toolkit at 6.11.1 and running it: its paged container is addressed
/// by ordinal, `setCurrentIndex` returns `void`, an out-of-range ordinal is a
/// silent no-op, and a **disabled page is arrived at anyway** — so there is no
/// refusal to assert and no reason to compare one against.
#[test]
fn r1695_every_open_destination_is_a_place_you_arrive_at() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = spec::destinations();
        let screens = super::screen_roster();
        let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for destination in roster.all() {
            let key = destination.key.as_ref();
            match &destination.standing {
                pinion_core::widgets::destination::Standing::Open => {
                    let shot = painted_at_destination(key);
                    assert_eq!(
                        state.at(),
                        key,
                        "arriving at {key} did not move the journey"
                    );
                    // ★★★★★ R1724 — what "its own" means depends on whose page
                    // it is. This screen's pages paint its settings rows or its
                    // cards; a mounted screen's page paints that screen, and
                    // the prefix is the screen's own tag rather than a fourth
                    // literal in this list.
                    let mounted = screens.is_mounted(key);
                    let inside: BTreeSet<String> = shot
                        .tags
                        .keys()
                        .filter(|tag| {
                            if mounted {
                                // Everything that is not this shell's chrome —
                                // which is the only thing this file can say
                                // about another screen's tags, and enough: the
                                // pages are compared pairwise below.
                                !tag.starts_with("shell.") && tag.as_str() != super::VIEW_TAG
                            } else {
                                tag.starts_with("shell.settings.") || tag.starts_with("card.")
                            }
                        })
                        .cloned()
                        .collect();
                    assert!(
                        !inside.is_empty(),
                        "the {key} page paints nothing of its own, so arriving \
                         at it is indistinguishable from not arriving",
                    );
                    seen.insert(key.to_owned(), inside);
                }
                pinion_core::widgets::destination::Standing::Closed(why) => {
                    let was = state.at();
                    let refusal = state
                        .go(key)
                        .expect_err("a closed destination refuses the journey");
                    assert_eq!(state.at(), was, "{key} refused and moved anyway");
                    let said = refusal.sentence(&roster);
                    assert!(
                        said.contains(why.detail()),
                        "the refusal for {key} is {said:?} and does not name \
                         the reason the seat is painted with",
                    );
                }
            }
        }

        // ★ The pages are DIFFERENT pages. Without this the check above is
        // satisfied by a region that paints the dashboard whatever the journey
        // says — which is the exact defect R1695 repaired, and it would
        // otherwise pass every assertion above.
        // ★★★★★ R1724 — three of them now, and PAIRWISE rather than the one
        // comparison two pages allowed. The third is the node graph lab,
        // mounted whole, so "the pages are distinct" became a claim a loop has
        // to make: with three destinations a single `pages[0]`/`pages[1]`
        // comparison would have left one page unchecked against either.
        let pages: Vec<(&String, &BTreeSet<String>)> = seen.iter().collect();
        assert_eq!(
            pages.len(),
            roster.open().count(),
            "every open destination was arrived at and measured",
        );
        // ★ R1729 — this used to pin the count at three, and the capture
        // viewer's mount made it four. The literal is gone: the population is
        // `roster.open().count()` on the line above, and all this needs to add
        // is that there is more than one page, because otherwise the pairwise
        // comparison below has nothing to compare and would pass on a screen
        // that paints the same thing everywhere. A count written twice is a
        // number that has to be edited by hand every time the tool grows a
        // section — this file has now paid that twice in two rounds.
        assert!(
            pages.len() > 1,
            "a single open destination makes the pairwise check below vacuous",
        );
        for (i, (key, page)) in pages.iter().enumerate() {
            for (other_key, other) in &pages[i + 1..] {
                assert!(
                    page.is_disjoint(other),
                    "{key} and {other_key} painted overlapping content: {:?}",
                    page.intersection(other).collect::<Vec<_>>(),
                );
            }
        }
    });
}

/// ★★★★★ R1729 — **every mounted screen actually paints itself, and only
/// where it belongs.**
///
/// The population is the roster's own `mounted_keys`, not a list here, so a
/// screen mounted by a later round is covered the day it is mounted rather than
/// the day somebody remembers to add it. R1724 wrote this check for the node
/// lab by name; the capture viewer's mount is the second consumer, and a second
/// hand-written copy is what this project lifts on sight.
///
/// Three claims per mounted screen, and the third is the one a picture cannot
/// show:
///
/// 1. arriving paints regions under **that screen's own root tag**;
/// 2. leaving takes them away, so the page is not painted everywhere at once;
/// 3. the host's chrome survives — a page is a page, not a takeover. Measured
///    at 6.11.1, a placed application window keeps its own menu bar, tool bar
///    and status bar on top of its host's, and the tree publishes two of each.
#[test]
fn r1729_every_mounted_screen_paints_itself_where_it_belongs() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let screens = super::screen_roster();
        let mounted: Vec<&str> = screens.mounted_keys().collect();
        assert!(
            mounted.len() > 1,
            "one mounted screen makes the away-comparison below vacuous",
        );

        for key in &mounted {
            state
                .go(key)
                .unwrap_or_else(|why| panic!("{key} is mounted and refused: {why:?}"));
            let shot = painted_at((WIN_W, WIN_H)).0;
            let root = screens
                .tag_of(key)
                .expect("a mounted destination names its screen's root tag");
            let here: Vec<&String> = shot
                .tags
                .keys()
                .filter(|tag| tag.as_str() == root || tag.starts_with(&format!("{root}.")))
                .collect();
            assert!(
                !here.is_empty(),
                "at {key}: the mounted screen's root is {root:?} and nothing \
                 under it is painted, so arriving is indistinguishable from not",
            );
            // The host is still the host.
            for chrome in ["shell.appbar", "shell.rail", &format!("shell.rail.{key}")] {
                assert!(
                    shot.rect(chrome).is_some(),
                    "at {key}: the host's {chrome} stopped being painted",
                );
            }
            // And every other mounted screen is away.
            for other in &mounted {
                if other == key {
                    continue;
                }
                let other_root = screens.tag_of(other).expect("a mounted key has a screen");
                assert!(
                    !shot.tags.keys().any(|tag| {
                        tag.as_str() == other_root || tag.starts_with(&format!("{other_root}."))
                    }),
                    "at {key}: {other}'s screen ({other_root}) is painted too",
                );
            }
        }
        state
            .go(spec::RAIL_ACTIVE)
            .expect("the opening seat is open");
    });
}

/// ★★★★★ R1728 — **the rail on the screen is the rail the reference draws**,
/// walked seat by seat through the paint and the press.
///
/// The integration half of the conformance check. `tests.rs` compares the
/// application's *roster value* with the specification, which is the model
/// question; this asks the questions only a painted screen can answer, and they
/// are the ones that were wrong:
///
/// 1. **Painted** — every seat the specification declares has a rectangle.
/// 2. **Ordered** — the rectangles run top to bottom in the specified order.
///    The reference draws one rail in one order on all three of its screens,
///    and nothing here had ever compared the order at all: a roster is a list
///    and a rail is a column, and the two agreeing is a separate fact.
/// 3. **No invention** — nothing tagged as a rail seat is a seat the
///    specification does not declare. This is the direction that had been
///    failing silently: three of the seven keys on this rail were this
///    application's own.
///
/// The press walk and the distinctness of the marks are the two siblings below,
/// split off so each failure names itself.
#[test]
fn r1728_the_painted_rail_is_the_rail_the_reference_draws() {
    let owner = Owner::new();
    owner.run(|| {
        let canon = spec::canon_spec();
        let shot = painted_at((WIN_W, WIN_H)).0;

        // 1 + 2. Painted, in the specified order, top to bottom.
        let mut previous: Option<(String, Rect)> = None;
        for seat in canon.seats() {
            let tag = format!("shell.rail.{}", seat.key);
            let rect = shot.rect(&tag).unwrap_or_else(|| {
                panic!(
                    "the specification declares seat {:?} and the screen paints no {tag}",
                    seat.key,
                )
            });
            if let Some((before_key, before)) = &previous {
                assert!(
                    before.y < rect.y,
                    "the specification puts {before_key} above {}, and the screen \
                     paints them at y={} and y={}",
                    seat.key,
                    before.y,
                    rect.y,
                );
            }
            previous = Some((seat.key.clone().into_owned(), rect));
        }

        // 3. And nothing else calls itself a seat.
        let painted: Vec<&String> = shot
            .tags
            .keys()
            .filter(|tag| tag.starts_with("shell.rail."))
            .collect();
        let specified: BTreeSet<String> = canon
            .seats()
            .iter()
            .map(|seat| format!("shell.rail.{}", seat.key))
            .collect();
        // ★★★ Not every tag under `shell.rail.` is a seat: the reference draws
        // an avatar at the foot of its rail, as `avatar` rather than as one of
        // its `ri` items, and this shell paints it inside the same tag
        // namespace. So the leftovers are checked rather than skipped — each
        // must be declared in the voice table under a population that is NOT
        // the rail's. Naming the exception here instead would make this gate
        // exactly as good as whoever remembered to update the list, which is
        // the failure mode the whole round is about.
        let mut chrome = 0_usize;
        for tag in &painted {
            if specified.contains(tag.as_str()) {
                continue;
            }
            let voice = spec::VOICES
                .iter()
                .find(|voice| voice.tag == tag.as_str())
                .unwrap_or_else(|| {
                    panic!(
                        "{tag} is painted on the rail, is not a specified seat, \
                         and is not declared as anything else either",
                    )
                });
            assert_ne!(
                voice.population,
                spec::Population::Rail,
                "{tag} claims to be a rail seat and the specification has no such seat",
            );
            chrome += 1;
        }
        assert_eq!(
            painted.len(),
            canon.len() + chrome,
            "the rail paints {} tags: {} specified seats and {chrome} declared as chrome",
            painted.len(),
            canon.len(),
        );
    });
}

/// ★★★★★ R1728 — **every specified seat answers a press the way the
/// specification says it does**, from the centre of the rectangle it was
/// actually painted in.
///
/// The half that makes the rail more than a picture. An open seat arrives; a
/// closed one refuses *and stays where it was*, which is the row the floor
/// fails outright — measured on 6.11.1, a disabled page is arrived at anyway.
/// And the press is resolved through this screen's own hit path rather than
/// through a geometry helper, because a helper agreeing with the painter is the
/// failure `debt-paint-and-gesture-read-two-facts` is open for.
#[test]
fn r1728_every_specified_seat_answers_a_press_as_specified() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let canon = spec::canon_spec();
        let shot = painted_at((WIN_W, WIN_H)).0;
        let roster = spec::destinations();
        let mut arrived = 0_usize;
        let mut refused = 0_usize;
        for seat in canon.seats() {
            let tag = format!("shell.rail.{}", seat.key);
            let rect = shot.rect(&tag).unwrap_or_else(|| {
                panic!("the specification declares {tag} and it is not painted")
            });
            let (x, y) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
            assert_eq!(
                super::hit_word(&Hit::at(&state, x, y)),
                tag,
                "pressing the centre of {tag} at ({x}, {y}) resolves elsewhere",
            );
            let key = seat.key.as_ref();
            let was = state.at();
            match state.go(key) {
                Ok(()) => {
                    assert_eq!(
                        state.at(),
                        key,
                        "arriving at {key} did not move the journey"
                    );
                    arrived += 1;
                }
                Err(why) => {
                    assert_eq!(
                        state.at(),
                        was,
                        "{key} refused the journey and moved anyway"
                    );
                    // ★ The refusal must be the KIND the specification requires,
                    // not merely a refusal: a seat closed as *reserved* when the
                    // specification says *unbuilt* sends a reader off to wait
                    // for a release the section is not booked into, and every
                    // check that only asked "did it refuse" would pass.
                    let said = why.sentence(&roster);
                    let standing = roster
                        .get(key)
                        .map(|d| Required::of(&d.standing))
                        .expect("the seat is on the roster it was walked from");
                    assert!(
                        !said.is_empty(),
                        "{key} refused and said nothing a reader could use",
                    );
                    // What the specification requires is what `tests.rs` holds
                    // against the declared remainder; what this asserts is that
                    // the standing the ROSTER carries is the standing the
                    // painted seat actually refuses with.
                    assert_ne!(standing, Required::Open, "{key} refused while open");
                    refused += 1;
                }
            }
            state
                .go(spec::RAIL_ACTIVE)
                .expect("the opening seat is open");
        }
        assert_eq!(
            arrived + refused,
            canon.len(),
            "every specified seat was pressed and answered",
        );
        assert_eq!(arrived, roster.open().count());
        assert_eq!(refused, roster.closed().count());
    });
}

/// ★★★★★ R1728 — **no two seats are drawn the same.**
///
/// Measured when this was first written, and it found two things at once. The
/// seat carrying the node graph lab — the one section of the tool this
/// application had actually finished — was drawn with the mark the reference
/// gives its **log** section. And two further seats had no arm in the painter
/// at all, fell through to its fallback, and were drawn **identically**: two
/// adjacent icons a reader could not tell apart, on a screen whose whole
/// subject is telling things apart.
///
/// Neither was visible to anything. A seat and its icon had never been one
/// fact: the rail table said what each seat *is* and the painter said what each
/// seat *looks like*, and no test had ever read both. Comparing the drawings
/// with each other is what makes the second failure impossible without needing
/// a copy of the reference's artwork in the repository, which there must not
/// be.
#[test]
fn r1728_no_two_seats_are_drawn_the_same() {
    let canon = spec::canon_spec();
    // Any colour: what is being compared is geometry, and every seat gets the
    // same one.
    let ink = pinion_core::style::Color::rgb(0xFF, 0xFF, 0xFF);
    let mut marks: BTreeMap<String, String> = BTreeMap::new();
    for seat in canon.seats() {
        let drawn = format!(
            "{:?}",
            super::rail_mark(seat.key.as_ref(), Rect::new(0, 0, 20, 20), ink)
        );
        assert!(
            drawn.len() > 32,
            "the {} seat draws no mark of its own",
            seat.key,
        );
        if let Some(other) = marks.insert(drawn, seat.key.clone().into_owned()) {
            panic!("the {} and {} seats are drawn identically", other, seat.key);
        }
    }
    assert_eq!(
        marks.len(),
        canon.len(),
        "every specified seat has its own mark"
    );
    // ★★★ R1728.1 — and none of them is the painter's FALLBACK.
    //
    // The closing audit found the hole the check above leaves: the defect it
    // was written for was two seats sharing the fallback, and it catches that
    // only because they collided. ONE seat falling through is unique, so it
    // passes — and the next rail key added without an arm gets the generic
    // mark silently, which is exactly the state `sessions` and `settings` were
    // already in when this round started. Comparing against the fallback
    // itself is what makes the arm mandatory rather than merely unshared.
    let fallback = format!(
        "{:?}",
        super::rail_mark("<no seat has this key>", Rect::new(0, 0, 20, 20), ink)
    );
    for seat in canon.seats() {
        let drawn = format!(
            "{:?}",
            super::rail_mark(seat.key.as_ref(), Rect::new(0, 0, 20, 20), ink)
        );
        assert_ne!(
            drawn, fallback,
            "the {} seat has no arm in the painter and is drawn with the generic mark",
            seat.key,
        );
    }
}

/// ★★★★★ R1695 — the specification says which destination each region belongs
/// to, and the screen paints exactly those.
///
/// Both directions, per destination. This is what closes the half of
/// `debt-the-voice-gate-judges-only-the-opening-screen` that lives here: the
/// census used to describe one screen because the application had one, and the
/// rail's roster is the enumeration that was missing.
#[test]
fn r1695_each_destination_paints_the_regions_the_specification_gives_it() {
    let owner = Owner::new();
    owner.run(|| {
        let roster = spec::destinations();
        for destination in roster.open() {
            let key = destination.key.as_ref();
            let shot = painted_at_destination(key);
            // Forward: a region this destination owns is painted here.
            for voice in spec::VOICES {
                if !voice.at.shows_at(key) {
                    continue;
                }
                for member in voice.population.members() {
                    let tag = voice.tag.replace("{}", &member);
                    assert!(
                        shot.rect(&tag).is_some(),
                        "at {key}: the specification gives this destination \
                         {tag:?} and the screen does not paint it",
                    );
                }
            }
            // Backward: a region another destination owns is NOT painted here.
            for voice in spec::VOICES {
                if voice.at.shows_at(key) {
                    continue;
                }
                for member in voice.population.members() {
                    let tag = voice.tag.replace("{}", &member);
                    assert!(
                        shot.rect(&tag).is_none(),
                        "at {key}: {tag:?} belongs to another destination and \
                         is painted here anyway — a page nobody navigated to \
                         is on screen",
                    );
                }
            }
        }
    });
}

/// ★★★★★ R1696 — **the screen has a keyboard, and the ring is the one the
/// specification declares.**
///
/// It had none. Measured on CI the round after this screen gained an
/// accessibility tree: it announces a navigation landmark of links, two
/// toolbars, a list of items, four tabs, a textbox and thirty-nine buttons, and
/// `focus/next` from cold answered nothing — announced as operable, unreachable
/// by keyboard. Two gates refused it, the same pair that refused the sibling
/// screen at R1693 for the same reason.
///
/// Three claims, because they fail differently:
///
/// * every stop the specification declares for a destination is focusable in
///   the scene painted at that destination, and nothing else is;
/// * the ring's ORDER is the paint order, which is what the §5.39 enumeration
///   walks — a table whose order was decorative would describe a Tab sequence
///   the screen does not have;
/// * every stop is a node in the accessibility tree, or a reader lands on
///   something the tree cannot name (`r1518`'s `missing bearer` arm).
#[test]
fn r1696_the_keyboard_ring_is_the_one_the_specification_declares() {
    use pinion_a11y::WidgetA11y;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = spec::destinations();
        let screens = super::screen_roster();
        for destination in roster.open() {
            let key = destination.key.as_ref();
            if key != state.at() {
                state.go(key).expect("an open destination is reachable");
            }
            // ★★★★★ R1724 — a destination whose page is a mounted screen has a
            // keyboard ring, and it is that screen's. `hello-node-lab` declares
            // its own stops and closes its own set against them; this table
            // could only ever hold a copy, and a copy of another screen's ring
            // is the two-lists-of-one-thing defect R1695 was a repair for. What
            // this census still owes at such a destination is that the section
            // is reachable by a keyboard at all.
            if screens.is_mounted(key) {
                let reachable = painted_at((WIN_W, WIN_H)).1.collect_focusable_tags();
                assert!(
                    !reachable.is_empty(),
                    "at {key}: a mounted screen is showing and nothing in the \
                     window is focusable",
                );
                continue;
            }
            let (_, scene) = painted_at((WIN_W, WIN_H));
            // The enumeration the framework walks, in the order it walks it.
            let walked = scene.collect_focusable_tags();
            let declared: Vec<&str> = spec::FOCUS_RING
                .iter()
                .filter(|stop| stop.at.shows_at(key))
                .map(|stop| stop.tag)
                .collect();
            assert!(
                !declared.is_empty(),
                "at {key}: the specification declares no keyboard stop, so this \
                 destination is announced as operable and unreachable",
            );
            // ★ The composites the table names come first and IN ORDER,
            // because the enumeration is depth-first over the paint scene and
            // this table's order is a claim about that. A control the
            // catalogue's own widgets declare (a switch, an appearance chip)
            // follows inside the page, which is why this is a prefix rather
            // than an equality.
            let composites: Vec<&str> = walked
                .iter()
                .map(String::as_str)
                .filter(|tag| spec::FOCUS_RING.iter().any(|stop| stop.tag == *tag))
                .collect();
            assert_eq!(
                composites, declared,
                "at {key}: the ring the scene enumerates is not the ring the \
                 specification declares (walked {walked:?})",
            );
            // ★★★ The half that is NOT self-comparison. The paint reads the
            // table, so "every declared stop is focusable" compares the table
            // with itself and R1669's rule says that is not a check. What the
            // table cannot produce is a stop it never named — a stray
            // `focusable` anywhere in the tree — so the set is closed here
            // against the two things allowed to be one: the table, and the
            // catalogue widgets the Settings page paints.
            let allowed: BTreeSet<String> = declared
                .iter()
                .map(|tag| (*tag).to_owned())
                .chain(
                    spec::OPTIONS
                        .iter()
                        .map(|o| format!("shell.settings.option.{}", o.key)),
                )
                // ★ R1762 — and the value rows' collapsed choosers, which are
                // the third kind of catalogue widget this page paints. From the
                // specification's own table, so a row added there arrives here
                // rather than being remembered.
                .chain(
                    spec::VALUE_ROWS
                        .iter()
                        .map(|row| format!("shell.settings.choose.{}", row.key)),
                )
                .chain((0..spec::THEMES.len()).map(|n| format!("shell.settings.theme.{n}")))
                .collect();
            for tag in &walked {
                assert!(
                    allowed.contains(tag),
                    "at {key}: {tag:?} is a keyboard stop the specification \
                     never declared — a ring nobody wrote down is a ring \
                     nobody can read",
                );
            }
            // Every stop is a node a reader can be told about.
            let announced: BTreeSet<String> =
                super::AnalyzerShellView::access_node(&ScreenState::default(), None)
                    .into_iter()
                    .map(|node| node.tag)
                    .collect();
            for tag in &walked {
                assert!(
                    announced.contains(tag),
                    "at {key}: {tag:?} is a keyboard stop and not a node in the \
                     accessibility tree — a reader lands on something the tree \
                     cannot name",
                );
            }
        }
    });
}

/// ★★★★★ R1695 — and the **accessibility tree** follows the rail too.
///
/// Found by a counterfactual that PASSED: emitting the settings page's nodes
/// while the dashboard is showing left every check above green, because they
/// all read the painted scene and a tree can announce what nothing paints. The
/// demo's boot census caught it as four `ghost` rows — which is the right
/// answer arriving in the wrong place, one build and forty seconds later, and
/// only for the destination the application happens to open at.
///
/// A reader offered a control that is not on screen is offered a control nobody
/// can reach, and that is the same defect as painting it — so it is the same
/// law, asked of the other surface.
#[test]
fn r1695_each_destination_announces_only_its_own_regions() {
    use pinion_a11y::WidgetA11y;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = spec::destinations();
        for destination in roster.open() {
            let key = destination.key.as_ref();
            if key != state.at() {
                state.go(key).expect("an open destination is reachable");
            }
            let announced: BTreeSet<String> =
                super::AnalyzerShellView::access_node(&ScreenState::default(), None)
                    .into_iter()
                    .map(|node| node.tag)
                    .collect();
            for voice in spec::VOICES {
                for member in voice.population.members() {
                    let tag = voice.tag.replace("{}", &member);
                    if voice.at.shows_at(key) {
                        assert!(
                            announced.contains(&tag),
                            "at {key}: {tag:?} is this destination's and the tree \
                             does not announce it",
                        );
                    } else {
                        assert!(
                            !announced.contains(&tag),
                            "at {key}: {tag:?} belongs to another destination and \
                             the tree announces it anyway — a reader is offered a \
                             control that is not on screen",
                        );
                    }
                }
            }
        }
    });
}

/// ★★ R1695 — the Settings page's two locked affordances are declared
/// unavailable with their booking, the same law the rail's seats keep.
///
/// A page the census could not see until this round is a page whose locked
/// seats nobody checked, which is how five of the sixteen came to be untested.
#[test]
fn r1695_the_settings_page_declares_its_locked_affordances() {
    let owner = Owner::new();
    owner.run(|| {
        let shot = painted_at_destination("settings");
        for row in spec::KEY_ROWS {
            let tag = format!("shell.settings.key.{}", row.key);
            let (kind, detail, recourse) = shot
                .inert
                .get(&tag)
                .unwrap_or_else(|| panic!("{tag} is booked for a later release and painted live"));
            assert_eq!(*kind, UnavailableKind::Reserved);
            assert_eq!(detail, row.reserved_for, "{tag}'s booking drifted");
            assert_eq!(*recourse, Recourse::AwaitRelease);
        }
        // And nothing else on the page is inert, so a switch that stopped
        // working would not hide inside the two that are meant to.
        let inert: Vec<&String> = shot
            .inert
            .keys()
            .filter(|t| t.starts_with("shell.settings."))
            .collect();
        assert_eq!(
            inert.len(),
            spec::KEY_ROWS.len(),
            "the settings page paints {} inert regions and books {}: {inert:?}",
            inert.len(),
            spec::KEY_ROWS.len(),
        );
    });
}

/// ★★★ R1695 — every control on the Settings page answers for **itself** when
/// pressed at the centre of the rectangle it was painted in.
///
/// The reachability half, which on this screen has caught the same class three
/// times: the painter and the hit test are two expressions of one geometry and
/// nothing but this makes them agree.
#[test]
fn r1695_every_settings_control_is_pressable_where_it_is_painted() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let shot = painted_at_destination("settings");
        let mut checked = 0;
        for (tag, rect) in &shot.tags {
            if !tag.starts_with("shell.settings.option.")
                && !tag.starts_with("shell.settings.key.")
                && !tag.starts_with("shell.settings.theme.")
            {
                continue;
            }
            let (px, py) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
            let hit = Hit::at(&state, px, py);
            assert_eq!(
                &super::hit_word(&hit),
                tag,
                "a press at the centre of {tag} ({px},{py}) answered {hit:?}",
            );
            checked += 1;
        }
        assert_eq!(
            checked,
            spec::OPTIONS.len() + spec::KEY_ROWS.len() + spec::THEMES.len(),
            "the sweep reached a different number of controls than the page has",
        );
    });
}

// ── R1697: every declared operation actually happens ─────────────────────────

/// The gesture that causes one operation, joined to [`spec::OPERATIONS`] by the
/// operation's name.
///
/// A function rather than a column in the table, for the reason the table's own
/// documentation gives: a gesture is a press at a place and a drag between two,
/// which is not a value a `const` can hold. The gate asserts the join is total
/// in both directions, so a declared gesture with no driver — and a driver for
/// an operation the table says has none — are both failures.
type OperationGesture = (&'static str, fn(&std::rc::Rc<ShellState>, &Painted));

const OPERATION_GESTURES: &[OperationGesture] = &[
    ("place a widget on the board", |state, shot| {
        press_tag(state, shot, "shell.palette.packet");
    }),
    ("move a card on the board", |state, shot| {
        // Grab the first card by its header grip and carry it right, far enough
        // that it lands in a cell it did not start in.
        drag_tag(state, shot, "card.packet#0.grip", (CELL_STEP, 0));
    }),
    ("resize a card", |state, shot| {
        // The size steppers exist in layout-edit mode only, and that is a
        // property of the SCREEN rather than a precondition of the table: the
        // affordance that opens the mode is the layout bar's own, so pressing
        // it here is how a person reaches the steppers at all.
        press_tag(state, shot, "shell.subbar.edit");
        let shot = painted();
        press_tag(state, &shot, "card.packet#0.widen");
    }),
    ("maximise a card", |state, shot| {
        press_tag(state, shot, "card.packet#0.maximize");
    }),
    ("restore a maximised card", |state, shot| {
        press_tag(state, shot, "card.packet#0.maximize");
    }),
    ("close a card", |state, shot| {
        press_tag(state, shot, "card.packet#0.close");
    }),
    ("detach a card", |state, shot| {
        press_tag(state, shot, "card.packet#0.tear_off");
    }),
    // ★★★★★ The three rows this round exists for, and the three that had no
    // driver because they had no operation.
    ("move a detached panel", |state, shot| {
        drag_tag(state, shot, "float.packet#0", (40, 25));
    }),
    ("size a detached panel", |state, shot| {
        drag_tag(state, shot, "float.packet#0.resize", (60, 40));
    }),
    ("bring a detached panel forward", |state, shot| {
        press_tag(state, shot, "float.packet#0");
    }),
    ("re-dock a detached panel", |state, shot| {
        press_tag(state, shot, "float.packet#0.redock");
    }),
    ("close a detached panel", |state, shot| {
        press_tag(state, shot, "float.packet#0.close");
    }),
];

/// Far enough along the board's row that a card lands in a cell it did not
/// start in — more than one column plus its gap at the opening window size.
const CELL_STEP: i32 = 160;

/// The centre of a painted rectangle.
const fn centre(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

/// Where a painted tag is, or a message naming the tag that is missing.
fn aim(shot: &Painted, tag: &str) -> (u32, u32) {
    centre(
        shot.rect(tag)
            .unwrap_or_else(|| panic!("{tag} is painted, so a person can aim at it")),
    )
}

/// A press and a release at one painted tag's centre.
fn press_tag(state: &std::rc::Rc<ShellState>, shot: &Painted, tag: &str) {
    let (x, y) = aim(shot, tag);
    ShellOracle::move_cursor(state, x, y);
    ShellOracle::press(state);
    ShellOracle::release(state);
}

/// A press at one painted tag's centre, a move by a signed delta, a release.
fn drag_tag(state: &std::rc::Rc<ShellState>, shot: &Painted, tag: &str, by: (i32, i32)) {
    let (x, y) = aim(shot, tag);
    ShellOracle::move_cursor(state, x, y);
    ShellOracle::press(state);
    let to = |v: u32, d: i32| u32::try_from(i64::from(v) + i64::from(d)).unwrap_or(0);
    ShellOracle::move_cursor(state, to(x, by.0), to(y, by.1));
    ShellOracle::release(state);
}

/// Read one introspection slot, through the surface an agent reads it through.
///
/// Not off the state's field: a witness read there would be true of a change no
/// client can observe, and *observable* is the whole claim the table makes.
fn witness(state: &std::rc::Rc<ShellState>, slot: &str) -> String {
    let mut oracle = ShellOracle::new();
    oracle.attach_state(std::rc::Rc::clone(state));
    match oracle.query(slot) {
        Ok(value) => format!("{value:?}"),
        Err(why) => panic!("the witness {slot:?} is not a slot this screen answers: {why:?}"),
    }
}

/// Bring the screen to the state an operation needs before it can be caused.
///
/// Reached the way a person reaches it — by causing the earlier operation the
/// table names, preferring its gesture — so a precondition can never be a state
/// no session produces. Panics rather than skipping: an unsatisfiable
/// precondition would silently stop exercising the operation that declared it,
/// which is a gate quietly covering less than it says.
fn reach_precondition(op: &spec::OperationSpec, state: &std::rc::Rc<ShellState>) {
    let Some(earlier) = op.needs else { return };
    let earlier = spec::OPERATIONS
        .iter()
        .find(|o| o.name == earlier)
        .unwrap_or_else(|| {
            panic!(
                "{:?} needs {earlier:?}, which this table does not hold",
                op.name
            )
        });
    if let Some((_, drive)) = OPERATION_GESTURES.iter().find(|(n, _)| *n == earlier.name) {
        let shot = painted();
        drive(state, &shot);
        return;
    }
    let (verb, arg) = earlier.verb.unwrap_or_else(|| {
        panic!(
            "{:?} needs {:?}, which has no way in at all",
            op.name, earlier.name
        )
    });
    let mut oracle = ShellOracle::new();
    oracle.attach_state(std::rc::Rc::clone(state));
    oracle
        .invoke(verb, IntrospectValue::Text(arg.to_owned()))
        .unwrap_or_else(|why| panic!("{:?}'s precondition refused: {why:?}", op.name));
}

/// The scene as it stands, indexed.
fn painted() -> Painted {
    painted_at((WIN_W, WIN_H)).0
}

/// Drive one operation's wire column, collecting what it failed to do.
fn drive_verb(op: &spec::OperationSpec, verb: &str, arg: &str, inert: &mut Vec<String>) {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        reach_precondition(op, &state);
        let before = witness(&state, op.witness);
        let mut oracle = ShellOracle::new();
        oracle.attach_state(std::rc::Rc::clone(&state));
        let answer = oracle.invoke(verb, IntrospectValue::Text(arg.to_owned()));
        let after = witness(&state, op.witness);
        if answer.is_err() {
            inert.push(format!(
                "{:?}: the wire refused `{verb} {arg}` ({answer:?})",
                op.name
            ));
        } else if before == after {
            inert.push(format!(
                "{:?}: `{verb} {arg}` was accepted and `{}` did not move",
                op.name, op.witness
            ));
        }
    });
}

/// Drive one operation's pointer column, collecting what it failed to do.
fn drive_gesture(
    op: &spec::OperationSpec,
    drive: fn(&std::rc::Rc<ShellState>, &Painted),
    inert: &mut Vec<String>,
) {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        reach_precondition(op, &state);
        let shot = painted();
        let before = witness(&state, op.witness);
        drive(&state, &shot);
        let after = witness(&state, op.witness);
        if before == after {
            inert.push(format!(
                "{:?}: the gesture ran and `{}` did not move — this is the \
                 column a wire-driven test cannot see",
                op.name, op.witness
            ));
        }
    });
}

/// Tear a card off and answer with the panel it became.
fn detached(state: &std::rc::Rc<ShellState>, card: &str) -> super::Float {
    let shot = painted();
    press_tag(state, &shot, &format!("card.{card}.tear_off"));
    state
        .float(card)
        .unwrap_or_else(|| panic!("{card} was torn off, so it is a panel"))
}

/// ★★★★★ R1697 — **a panel stops at its floor**, which "the slot moved" cannot
/// say.
///
/// The operations gate proves a resize changes something; it is satisfied by a
/// resize with no floor at all, and a panel dragged to nothing is a panel a
/// person cannot get back. The floor is the reference's own — `Math.max(320,
/// …)` and `Math.max(220, …)` in its source, read rather than guessed — and
/// this asserts the exact numbers, because a clamp that is merely "some floor"
/// is a clamp nobody can reproduce.
#[test]
fn r1697_a_panel_cannot_be_sized_below_its_floor() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let panel = detached(&state, "packet#0");
        assert_eq!(
            (panel.w, panel.h),
            (super::FLOAT_W, super::FLOAT_H),
            "a panel opens at the reference's size"
        );

        // Pull the corner far up and to the left — past the floor in both axes
        // at once, and past the panel's own origin, which is what a real drag
        // to the top-left corner of the window does.
        let shot = painted();
        drag_tag(&state, &shot, "float.packet#0.resize", (-2000, -2000));
        let squashed = state.float("packet#0").expect("the panel is still there");
        assert_eq!(
            (squashed.w, squashed.h),
            (super::FLOAT_MIN_W, super::FLOAT_MIN_H),
            "the corner clamps at the floor rather than collapsing the panel"
        );

        // And the floor is a floor, not a fixed size: it grows again.
        let shot = painted();
        drag_tag(&state, &shot, "float.packet#0.resize", (90, 60));
        let grown = state.float("packet#0").expect("the panel is still there");
        assert_eq!(
            (grown.w, grown.h),
            (super::FLOAT_MIN_W + 90, super::FLOAT_MIN_H + 60),
            "and the drag is exact — each event measured from where the grab opened"
        );
    });
}

/// ★★★★★ R1697 — **a press brings the panel under it to the front**, and the
/// hit test agrees on the next press.
///
/// The half the operations gate cannot see: `floats` moving proves a `z`
/// changed, not that the *screen* changed. Two overlapping panels are the state
/// where stacking is a fact rather than a field — the one on top is the one a
/// press at the shared point reaches, and the one drawn last.
///
/// It is also the case that made `z` necessary at all. The order was the
/// vector's, read backwards; that answers correctly only while nothing
/// reorders it, and a raise is exactly a reorder.
#[test]
fn r1697_a_press_brings_the_panel_under_it_to_the_front() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let first = detached(&state, "packet#0");
        let second = detached(&state, "decode#1");
        assert!(second.z > first.z, "the newest panel arrives in front");

        // Put them on top of one another, so a point exists that both cover.
        state.set_float(
            "decode#1",
            &super::Float {
                id: "decode#1".to_owned(),
                x: first.x,
                y: first.y,
                w: first.w,
                h: first.h,
                z: second.z,
            },
        );
        // ★ The point comes from the PAINTED rectangle, never from the state's
        // own numbers: a panel's coordinates are the canvas's and a hit test
        // takes the window's, and a test that did the conversion itself would
        // be asserting against its own arithmetic (R1684's lesson, and the
        // first draft of this test walked into it).
        let shot = painted();
        let shared = aim(&shot, "float.decode#1");

        assert_eq!(
            super::hit_word(&Hit::at(&state, shared.0, shared.1)),
            "float.decode#1",
            "the front panel is the one the shared point reaches"
        );
        // The scene agrees: the frontmost panel is painted last.
        let order = painted_order();
        assert_eq!(
            order,
            vec!["float.packet#0".to_owned(), "float.decode#1".to_owned()],
            "and it is painted last, so it is the one drawn on top"
        );

        // Raise the one underneath, the way a person does: slide the front one
        // partly off and press the strip of the covered panel that is showing.
        state.set_float(
            "decode#1",
            &super::Float {
                x: first.x + 200,
                ..state.float("decode#1").expect("present")
            },
        );
        let shot = painted();
        let back = shot
            .rect("float.packet#0")
            .expect("the covered panel is painted");
        let showing = (back.x + 8, back.y + back.h / 2);
        // ★ The aim is proved before it is used. Aiming at the covered panel's
        // CENTRE is what the first draft did, and the centre is still under the
        // other panel — so the press raised the wrong one and the assertion
        // below failed for a reason that was about the test.
        assert_eq!(
            super::hit_word(&Hit::at(&state, showing.0, showing.1)),
            "float.packet#0",
            "this point is on the covered panel and not on the one over it"
        );
        ShellOracle::move_cursor(&state, showing.0, showing.1);
        ShellOracle::press(&state);
        ShellOracle::release(&state);

        assert!(
            state.float("packet#0").expect("present").z
                > state.float("decode#1").expect("present").z,
            "the pressed panel came forward"
        );
        assert_eq!(
            painted_order(),
            vec!["float.decode#1".to_owned(), "float.packet#0".to_owned()],
            "and the paint order followed, because both read one function"
        );

        // ★★★★★ The assertion a counterfactual demanded. Everything above was
        // satisfied by the PRE-R1697 hit test — the roster read backwards —
        // because the raised panel was also the one that roster reached first.
        // Stacking is only a fact where two panels cover one point, and it is
        // only tested where they do so AFTER a raise has changed the order.
        state.set_float(
            "decode#1",
            &super::Float {
                x: state.float("packet#0").expect("present").x,
                ..state.float("decode#1").expect("present")
            },
        );
        let shot = painted();
        let shared = aim(&shot, "float.packet#0");
        assert_eq!(
            super::hit_word(&Hit::at(&state, shared.0, shared.1)),
            "float.packet#0",
            "★ a point both panels cover reaches the RAISED one — the roster's \
             own order would answer the other, and it is not the order any more"
        );
    });
}

/// ★★★★★ R1697 — **a press that moved nothing does not say it moved.**
///
/// Also demanded by a counterfactual: every check above is satisfied by a
/// release that announces "moved" for an ordinary click, because none of them
/// reads what the screen SAYS. That is the lie R1695 took out of the rail —
/// a message describing something that did not happen — and a panel is where
/// it comes back, since a click on one opens a grab that then carries nothing.
#[test]
fn r1697_a_click_on_a_panel_does_not_announce_a_move() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let panel = detached(&state, "packet#0");
        let opening = state.toast.get();

        let shot = painted();
        press_tag(&state, &shot, "float.packet#0");
        assert_eq!(
            state.toast.get(),
            opening,
            "a press and release that moved nothing said nothing new"
        );
        assert_eq!(
            (state.float("packet#0").expect("present").x, panel.y),
            (panel.x, panel.y),
            "and moved nothing, which is what makes the assertion above mean something"
        );

        // The other half: a drag that DOES move must say so, or the check
        // above is satisfied by a screen that never speaks at all.
        let shot = painted();
        drag_tag(&state, &shot, "float.packet#0", (30, 20));
        assert_ne!(
            state.toast.get(),
            opening,
            "a drag that moved the panel announced it"
        );
        assert!(
            state.toast.get().sentence().contains("moved"),
            "and said what happened: {:?}",
            state.toast.get()
        );
    });
}

/// How many of `spec::OPERATIONS` this screen cannot perform at all.
///
/// Zero, and it is said rather than assumed: every declared row is reachable by
/// at least one column, so this ratchet's job here is to fail the moment a row
/// is added that nothing can cause. That is the direction a table like this
/// rots in — a row written for an operation somebody meant to build. It lives
/// beside the gate rather than in the specification because it is a fact about
/// THIS BUILD rather than about the tool, which is where the sibling screen
/// keeps its own.
const ABSENT_OPERATIONS: usize = 0;

/// The detached panels in the order the scene paints them, back to front.
fn painted_order() -> Vec<String> {
    let (_, scene) = painted_at((WIN_W, WIN_H));
    let mut order = Vec::new();
    scene.for_each_node(&mut |visit| {
        if let Some(tag) = visit.node.tag()
            && tag.starts_with("float.")
            && !tag[6..].contains('.')
        {
            order.push(tag.to_owned());
        }
    });
    order
}

/// ★★★★★ R1697 — **for every way this screen says an operation can be caused,
/// causing it that way changes something observable.**
///
/// The gate the dashboard did not have, and the defect it would have caught the
/// day it was written: a detached panel could be torn off, closed and re-docked
/// and **could not be moved**. Everything else here was green, each check
/// correctly — the panel is painted, hit-testable, contained, named and
/// announced. None of them asks whether grabbing it moves it. A person had to
/// open the window and pull.
///
/// Both columns are driven, never one: the column a test naturally drives is
/// the wire, and the column that breaks is the pointer. And the witness is read
/// through the introspection surface rather than off a field, so *changed*
/// means changed where a client can see it.
#[test]
fn r1697_every_declared_way_of_causing_an_operation_causes_it() {
    // The half a reader of the table alone can check, from the framework.
    let faults = pinion_core::operation::faults(spec::OPERATIONS);
    assert!(faults.is_empty(), "the table is inconsistent: {faults:?}");
    let order = pinion_core::operation::in_order(spec::OPERATIONS)
        .unwrap_or_else(|stuck| panic!("the preconditions form a cycle: {stuck:?}"));
    assert_eq!(order.len(), spec::OPERATIONS.len());

    let declared: BTreeSet<&str> = spec::OPERATIONS
        .iter()
        .filter(|op| op.gesture)
        .map(|op| op.name)
        .collect();
    let driven: BTreeSet<&str> = OPERATION_GESTURES.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        declared, driven,
        "★ the specification and the drivers name different operations — a \
         gesture declared with nothing behind it is exactly what nailed a \
         detached panel where it landed"
    );

    let mut inert = Vec::new();
    let mut exercised = 0;
    for op in spec::OPERATIONS {
        if let Some((verb, arg)) = op.verb {
            drive_verb(op, verb, arg, &mut inert);
            exercised += 1;
        }
        if let Some((_, drive)) = OPERATION_GESTURES.iter().find(|(n, _)| *n == op.name) {
            drive_gesture(op, *drive, &mut inert);
            exercised += 1;
        }
    }

    assert!(
        inert.is_empty(),
        "{} of {exercised} declared way(s) of causing an operation caused \
         nothing:\n  {}",
        inert.len(),
        inert.join("\n  ")
    );

    let absent = pinion_core::operation::absent(spec::OPERATIONS);
    assert_eq!(
        absent.len(),
        ABSENT_OPERATIONS,
        "★ this screen can cause {} of the {} operations it declares and the \
         ratchet says {}. Growing is a regression; shrinking means the table \
         moved and this number has to move with it:\n  {}",
        spec::OPERATIONS.len() - absent.len(),
        spec::OPERATIONS.len(),
        spec::OPERATIONS.len() - ABSENT_OPERATIONS,
        absent.join("\n  ")
    );
}

// ── R1733: the palette hands the board a footprint ─────────────────────────
//
// ★★★★★ The integration test the round is judged by: the gesture is DRIVEN —
// a real press on a palette row, a real cursor move onto the board, a real
// release — and what it puts on screen is read back out of the PAINT and held
// against `docs/analyzer-board-spec.json`, in both directions.
//
// A table of intentions would pass while the painter drew something else; that
// is the whole reason R1730 built this mechanism, and it has caught something
// on every screen it has been pointed at since.

/// What the board specification calls one part of one surface.
///
/// Read out of the document rather than written here, so a title this module
/// invented could not make a difference report as agreement.
fn board_title(surface: &'static str) -> impl Fn(&str) -> Option<String> {
    move |key: &str| {
        spec::board_document()
            .canon(surface)?
            .parts()
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.title.as_ref().to_owned())
    }
}

/// The kind a carry is picked up from in these checks.
///
/// The FIRST placeable kind, taken from the catalogue rather than named here —
/// a reserved row refuses to be picked up (by design, and asserted below), and
/// a hand-written kind is one the catalogue can rename out from under.
fn first_placeable() -> &'static str {
    spec::CATALOGUE
        .iter()
        .find(|w| w.tier == spec::Tier::Placeable)
        .map(|w| w.kind)
        .expect("the catalogue places at least one kind")
}

/// ★★★★★ R1734 — the board's DECLARED accept set, as the parts a
/// specification is judged against.
///
/// Sourced from [`ShellOracle`]'s
/// [`ExternalIntrospect::drop_contract`](pinion_core::external::ExternalIntrospect::drop_contract)
/// impl — the same method `scene/query .../$drop` and `scene/drop_targets`
/// read — rather than from the constant behind it, so a build whose published
/// answer stopped matching its constant would fail here.
///
/// The title is DERIVED from the clause (its actions, then its region) rather
/// than written beside it, which is what makes the pin a check: a clause that
/// gained `link`, or narrowed to a part list, would arrive with a different
/// sentence and the specification would refuse it.
fn declared_drop_parts() -> Vec<pinion_core::conformance::Part> {
    let contract = ShellOracle::new().drop_contract();
    contract
        .clauses
        .iter()
        .map(|clause| {
            let actions = clause
                .actions
                .iter()
                .map(pinion_core::drop_target::DropAction::as_wire_name)
                .collect::<Vec<_>>()
                .join(" or ");
            let parts = clause.named_parts();
            let region = if parts.is_empty() {
                "anywhere on the surface".to_owned()
            } else {
                format!("on {}", parts.join(", "))
            };
            pinion_core::conformance::Part::new(clause.kind, format!("{actions}, {region}"))
        })
        .collect()
}

/// Pick a palette row up and carry it to the middle of the canvas, without
/// letting go. Answers the cell the carry says it would land in.
///
/// Driven through the same two entry points a hand reaches — the press path and
/// the cursor path — so a green here is a claim about the screen rather than
/// about a test-only door.
fn carry_to_middle(state: &std::rc::Rc<ShellState>, shot: &Painted, kind: &str) -> (u32, u32) {
    let (px, py) = aim(shot, &format!("shell.palette.{kind}"));
    ShellOracle::move_cursor(state, px, py);
    ShellOracle::press(state);
    let (mx, my) = board_middle();
    ShellOracle::move_cursor(state, mx, my);
    state
        .drag
        .get()
        .and_then(|d| d.landing())
        .expect("a carry over the middle of the board has a landing")
}

/// ★★★★★ R1735 — a **real router drag session** against the running screen.
///
/// Not a helper called directly: an `InputRouter` is built over the paint this
/// sweep just produced and a state scene holding this screen's own `External`,
/// and the gesture goes in as cursor moves, a press and a release. So every
/// link is exercised — the root's `drop_target` opt-in that makes the drop point
/// resolve to this surface at all, the declaration that gates dispatch, this
/// screen's `drop_offered`, the standing the router forwards back to the source,
/// and the commit that takes the acceptance as its witness.
///
/// Before this round none of that could run against this screen: nothing routed
/// a drop to it, because no node in its paint had opted in as a drop region.
/// The screen-driven `press` / `move_cursor` / `release` path is still exercised
/// wherever the claim is about the PAINT — but a claim about what a release
/// DOES belongs here, because the router is what performs one.
struct RouterDrag {
    router: pinion_runtime::InputRouter,
    model: Scene,
}

impl RouterDrag {
    /// Open a session over `scene`, with this screen's own state behind it.
    fn over(state: &std::rc::Rc<ShellState>, scene: Scene) -> Self {
        use pinion_core::scene::ExternalNode;
        let mut oracle = ShellOracle::new();
        oracle.attach_state(std::rc::Rc::clone(state));
        let mut model = Scene::External(
            ExternalNode::new(Box::new(oracle)).with_tag(super::VIEW_TAG.to_string()),
        );
        let mut router = pinion_runtime::InputRouter::new();
        router.update_paint_scene(scene, &mut model);
        Self { router, model }
    }

    fn cursor(&mut self, at: (u32, u32)) {
        self.router.cursor_moved(
            pinion_runtime::PointerId::MOUSE,
            f64::from(at.0),
            f64::from(at.1),
            &mut self.model,
        );
    }

    fn press(&mut self) {
        self.router
            .pointer_down(pinion_runtime::PointerId::MOUSE, &mut self.model);
    }

    fn release(&mut self) {
        self.router
            .pointer_up(pinion_runtime::PointerId::MOUSE, &mut self.model);
    }
}

/// The middle of the board, in window coordinates.
fn board_middle() -> (u32, u32) {
    let canvas = super::canvas_rect();
    (canvas.x + canvas.w / 2, canvas.y + canvas.h / 2)
}

/// R1735 — the standings the source was handed over the board and back over the
/// palette row it came from.
fn router_standings(
    state: &std::rc::Rc<ShellState>,
    scene: Scene,
    shot: &Painted,
    kind: &str,
) -> (
    pinion_core::drop_target::DropStanding,
    pinion_core::drop_target::DropStanding,
) {
    let row = aim(shot, &format!("shell.palette.{kind}"));
    let mut drag = RouterDrag::over(state, scene);
    drag.cursor(row);
    drag.press();
    drag.cursor(board_middle());
    let accepted = state.standing.borrow().clone();
    drag.cursor(row);
    let refused = state.standing.borrow().clone();
    // Let go where nothing takes it, so the sweep's next case starts with an
    // empty hand rather than a session this one left open.
    drag.release();
    (accepted, refused)
}

/// R1735 — the two standings as the parts a specification is judged against.
///
/// Each title is DERIVED from the framework's own value — the action for an
/// acceptance, the refusal's wire tag for a refusal — so a landing that stopped
/// naming a cell, or a refusal that changed which of the four kinds it is,
/// arrives with a different sentence and the pin refuses it.
fn standing_parts(
    accepted: &pinion_core::drop_target::DropStanding,
    refused: &pinion_core::drop_target::DropStanding,
) -> Vec<pinion_core::conformance::Part> {
    use pinion_core::conformance::Part;
    use pinion_core::drop_target::DropStanding;
    let mut parts = Vec::new();
    if let DropStanding::Accepted { accept, .. } = accepted {
        let names_a_cell = matches!(
            &accept.landing,
            IntrospectValue::Json(v) if v.get("col").is_some() && v.get("row").is_some()
        );
        parts.push(Part::new(
            accepted.as_wire_name(),
            format!(
                "{}, {}",
                accept.action.as_wire_name(),
                if names_a_cell {
                    "naming the cell the commit will use"
                } else {
                    "naming nothing"
                }
            ),
        ));
    }
    if let DropStanding::Refused { refusal, .. } = refused {
        parts.push(Part::new(
            refused.as_wire_name(),
            format!("{}, carrying the reason", refusal.as_wire_name()),
        ));
    }
    parts
}

/// ★★★★★ R1733 — **every surface the board specification declares is the one
/// the paint draws, while a widget is actually being carried.**
///
/// Swept across every state and every size, because a gesture that only
/// conforms on the screen as it opens has not been checked in the states a
/// person reaches: a maximised card, a board of one-cell cards, an open menu.
#[test]
fn r1733_every_specified_board_surface_is_the_one_the_paint_draws() {
    let doc = spec::board_document();
    let kind = first_placeable();
    let mut carried = 0;
    sweep(|state, shot, _, case| {
        carry_to_middle(state, shot, kind);
        // Repaint: the carry is what puts these surfaces on screen, so the shot
        // the sweep took before it was armed cannot hold them.
        let (_, scene) = painted_at(case.size);
        carried += 1;

        for surface in doc.surfaces() {
            let built = match surface {
                // Layers, so the specified order is the z-order.
                "carry" => pinion_core::test_fixtures::surface::painted_stack(
                    &scene,
                    "shell.carry.",
                    &board_title("carry"),
                ),
                "slot" => pinion_core::test_fixtures::surface::painted_surface(
                    &scene,
                    "shell.carry.slot.",
                    &board_title("slot"),
                ),
                "palette_row" => pinion_core::test_fixtures::surface::painted_surface_of(
                    &scene,
                    super::PALETTE_PART,
                    kind,
                    &board_title("palette_row"),
                ),
                // ★★★★★ R1734 — the one surface here that is not painted. It is
                // what the screen SAYS it accepts, read back through the same
                // `ExternalIntrospect` method the wire's `$drop` answers with,
                // so what an agent is told and what this pin judges are one
                // call. Swept with the rest because a declaration is a claim
                // about every state: a board that stops accepting widgets once
                // a card is maximised would be caught here and nowhere else.
                "drop_contract" => declared_drop_parts(),
                // ★★★★★ R1735 — the LIVE answer, driven through a real router
                // session in this very case. `drop_contract` above is what the
                // board says before anything is picked up; this is what it says
                // about the carry actually in hand, and it is the value the
                // router forwarded to the source rather than one this test
                // re-derived. Swept with the rest for the same reason: whether
                // a release lands is a claim about every state, and a board
                // that stopped accepting once a card is maximised would be
                // caught here.
                "drop_standing" => {
                    let (accepted, refused) =
                        router_standings(state, painted_at(case.size).1, shot, kind);
                    standing_parts(&accepted, &refused)
                }
                other => panic!("no surface named {other}"),
            };
            assert!(
                !built.is_empty(),
                "{case}: the {surface} surface painted no parts at all, so a \
                 difference against it would come out empty and read as success",
            );
            let unreconciled: Vec<String> = doc
                .unreconciled(surface, &built)
                .iter()
                .map(pinion_core::conformance::Unreconciled::sentence)
                .collect();
            assert!(
                unreconciled.is_empty(),
                "{case}: the painted {surface} surface is not what \
                 docs/analyzer-board-spec.json declares:\n  {}\n  \
                 (painted: {:?})",
                unreconciled.join("\n  "),
                built.iter().map(|p| p.key.as_ref()).collect::<Vec<_>>(),
            );
        }
    });
    assert_eq!(carried, CASES, "every swept case carried a widget");
}

/// ★★ The specification is the reference's, rather than whatever this build
/// happens to hold.
#[test]
fn r1733_the_board_specification_is_the_references_own_gesture() {
    let doc = spec::board_document();
    assert_eq!(
        doc.canon("palette_row")
            .expect("the pin fixes the row")
            .parts()
            .iter()
            .map(|p| p.key.as_ref())
            .collect::<Vec<_>>(),
        ["swatch", "name", "gist", "verb"],
        "the reference's row is a code tile, a name, a line and an add seat",
    );
    assert_eq!(
        doc.canon("carry")
            .expect("the pin fixes the carry")
            .parts()
            .iter()
            .map(|p| p.key.as_ref())
            .collect::<Vec<_>>(),
        ["grid", "slot", "banner"],
        "and while a footprint is carried it raises the grid, marks the cell \
         and says what letting go does",
    );
    assert_eq!(
        doc.canon("slot").expect("the pin fixes the mark").len(),
        2,
        "the reference's mark carries a grip glyph and the cell in words",
    );
}

/// ★★★★★ R1733 — **the cell the preview drew is the cell the release placed**,
/// driven through the real gesture at every size.
///
/// The property R1668 established for a card already on the board, on the drag
/// that had no answer at all until this round. Asserted on the SCREEN rather
/// than on the framework type, because the framework's own test cannot see a
/// shell that decides to clamp for itself — which is exactly what R1668 found.
#[test]
fn r1733_a_carry_lands_where_its_preview_said_it_would() {
    let kind = first_placeable();
    let mut checked = 0;
    sweep(|state, shot, _, case| {
        let before = state.board.get().tiles().len();
        // ★★★★★ R1735 — through the ROUTER, because the router is what performs
        // a release now. Driven screen-first this asserted a path real input no
        // longer takes, and it would have stayed green while the gesture a
        // person makes did nothing.
        let row = aim(shot, &format!("shell.palette.{kind}"));
        let mut drag = RouterDrag::over(state, painted_at(case.size).1);
        drag.cursor(row);
        drag.press();
        drag.cursor(board_middle());
        let previewed = state
            .drag
            .get()
            .and_then(|d| d.landing())
            .unwrap_or_else(|| {
                panic!("{case}: a carry over the middle of the board has a landing")
            });
        let carrying = witness(state, "carrying");
        assert!(
            carrying.contains("fresh:"),
            "{case}: the wire says what is being carried, and it is not on the \
             board yet: {carrying}",
        );
        assert_eq!(
            witness(state, "drag"),
            format!(
                "Text(\"{kind}#{},{},{}\")",
                state.next_id.borrow(),
                previewed.0,
                previewed.1
            ),
            "{case}: the wire's landing is the carry's",
        );
        // ★★★★★ R1735 — and the STANDING the router handed the source names
        // that same cell, before anything is released. That is the half the
        // floor has no room for: measured at 6.11.1, a source is told an object
        // and an action and never a position.
        assert_eq!(
            state
                .standing
                .borrow()
                .accepted()
                .map(|a| a.landing.clone()),
            Some(IntrospectValue::Json(
                serde_json::json!({"col": previewed.0, "row": previewed.1})
            )),
            "{case}: the standing names the cell the preview drew",
        );

        // The id the carry is holding, taken from the wire before the release:
        // the board may already hold cards of this kind, so "a card of that
        // kind at that row" is not a witness — the first draft used it and
        // found an OLDER card at the same row, reporting a defect that was its
        // own choice of needle.
        let id = pinion_core::widgets::tile_grid::TileId::new(format!(
            "{kind}#{}",
            state.next_id.borrow()
        ));
        drag.release();
        let board = state.board.get();
        assert_eq!(
            board.tiles().len(),
            before + 1,
            "{case}: a release over the board places one card",
        );
        let placed = board
            .tile(&id)
            .unwrap_or_else(|| panic!("{case}: the carried card {id} is on the board"));
        assert_eq!(
            (placed.col, placed.row),
            previewed,
            "{case}: the preview promised {previewed:?} and the release took \
             {:?}",
            (placed.col, placed.row),
        );
        assert_eq!(
            witness(state, "carrying"),
            "Text(\"\")",
            "{case}: nothing is carried once it has been put down",
        );
        checked += 1;
    });
    assert_eq!(checked, CASES);
}

/// ★★★★★ R1733 — **the action survives the gesture.**
///
/// A press and a release **on the same spot** still adds at the bottom of the
/// board. This is the assertion that keeps a pointer-only reference from
/// costing a reader the only path they have: the reference has zero keyboard
/// bindings, so reproducing its drag INSTEAD of the click would be a regression
/// wearing a reproduction's clothes.
///
/// ★ R1735 — it survives because the router synthesises the trailing release
/// only when the press did NOT become a drag, so the latch is still there for
/// it. A press that travelled is a drag and nothing else, which is what
/// `r1733_a_carry_let_go_off_the_board_is_not_a_placement` now asserts.
#[test]
fn r1733_a_click_on_a_palette_row_still_adds_at_the_bottom() {
    let kind = first_placeable();
    let mut checked = 0;
    sweep(|state, shot, _, case| {
        let board = state.board.get();
        let bottom = board.rows();
        let before = board.tiles().len();
        press_tag(state, shot, &format!("shell.palette.{kind}"));
        let board = state.board.get();
        assert_eq!(
            board.tiles().len(),
            before + 1,
            "{case}: a click on a palette row adds a card",
        );
        let placed = board
            .tiles()
            .iter()
            .find(|t| t.row == bottom && t.col == 0)
            .unwrap_or_else(|| panic!("{case}: the click placed at the bottom of the board"));
        assert_eq!(super::kind_of(placed.id.as_str()), kind);
        assert!(
            state.drag.get().is_none(),
            "{case}: and nothing is left being carried",
        );
        checked += 1;
    });
    assert_eq!(checked, CASES);
}

/// ★★★★★ R1735 — **a fresh carry is not this screen's to commit.**
///
/// The claim `commit_drag`'s first arm makes in a comment, as a check. A
/// palette press opens a router drag session, so the release is committed by
/// `drop_commit` and the hand is emptied by `drag_release_at` — by the time
/// this screen's own `release` runs there is nothing left for it to place, and
/// running it a second time places nothing either.
///
/// Worth its own test because the alternative is a comment: an arm that is
/// unreachable through real input, left total so a case nobody has seen cannot
/// crash, reads exactly like an arm somebody forgot to delete.
#[test]
fn r1735_a_fresh_carry_is_not_the_shells_to_commit() {
    let kind = first_placeable();
    let mut checked = 0;
    sweep(|state, shot, _, case| {
        let before = state.board.get().tiles().len();
        let row = aim(shot, &format!("shell.palette.{kind}"));
        let mut drag = RouterDrag::over(state, painted_at(case.size).1);
        drag.cursor(row);
        drag.press();
        drag.cursor(board_middle());
        assert!(
            state.drag.get().is_some_and(|d| !d.carried().is_placed()),
            "{case}: a fresh carry is in hand mid-gesture",
        );
        drag.release();
        assert!(
            state.drag.get().is_none(),
            "{case}: the router emptied the hand, so this screen's own release \
             has no carry to find",
        );
        assert_eq!(
            state.board.get().tiles().len(),
            before + 1,
            "{case}: and the drop committed exactly once",
        );
        // A second release with nothing in hand and no latch left: the arm the
        // comment calls unreachable, driven, placing nothing.
        ShellOracle::release(state);
        assert_eq!(
            state.board.get().tiles().len(),
            before + 1,
            "{case}: a release after the router's own places nothing more",
        );
        checked += 1;
    });
    assert_eq!(checked, CASES);
}

/// ★★★★★ R1733 — **a carry let go off the board places nothing where the
/// cursor is**, and ★★★★★ R1735 — it places nothing anywhere else either.
///
/// The half a card drag never needed and the reference does not have: its board
/// drag listens on the whole document, so a release over its palette commits.
/// Here the carry has no landing off the board and the release is refused, with
/// a reason.
///
/// ★ R1735 changed the second half of this. R1733 let an abandoned carry fall
/// through to the latched control, so a drag that wandered off and came back
/// still added a card at the bottom — this screen re-deriving click-vs-drag and
/// deciding the opposite of the framework's own rule, under which a press that
/// became a real drag suppresses the trailing click (R794). The floor agrees
/// with the framework: measured at 6.11.1, a source that ran a drag receives
/// ZERO mouse releases for that gesture. The click path is untouched and is
/// checked by `r1733_a_click_on_a_palette_row_still_adds_at_the_bottom`, which
/// is a press and release that never moved.
#[test]
fn r1733_a_carry_let_go_off_the_board_is_not_a_placement() {
    let kind = first_placeable();
    let mut checked = 0;
    sweep(|state, shot, _, case| {
        let bottom = state.board.get().rows();
        let before: Vec<String> = state
            .board
            .get()
            .tiles()
            .iter()
            .map(|t| t.id.as_str().to_owned())
            .collect();
        let row = aim(shot, &format!("shell.palette.{kind}"));
        let mut drag = RouterDrag::over(state, painted_at(case.size).1);
        drag.cursor(row);
        drag.press();
        // Onto the board, so a landing exists...
        drag.cursor(board_middle());
        let over = state
            .drag
            .get()
            .and_then(|d| d.landing())
            .unwrap_or_else(|| panic!("{case}: a carry over the board has a landing"));
        assert_ne!(
            over,
            (0, bottom),
            "{case}: the middle of the board is not where a click would put it, \
             so the two outcomes are distinguishable",
        );
        // ...and back onto the palette, where it is not.
        drag.cursor(row);
        assert_eq!(
            state.drag.get().and_then(|d| d.landing()),
            None,
            "{case}: carried off the board, there is no cell a release would use",
        );
        // ★★★★★ R1735 — and the source is told WHY, which is a different answer
        // from "there is nothing here". On the floor those two are one answer.
        let refusal = state.standing.borrow().sentence();
        assert!(
            refusal.contains("board"),
            "{case}: the refusal says what would have worked: {refusal:?}",
        );
        drag.release();

        let after: Vec<String> = state
            .board
            .get()
            .tiles()
            .iter()
            .map(|t| t.id.as_str().to_owned())
            .collect();
        assert_eq!(
            after, before,
            "{case}: ★ a completed drag let go off the board places nothing — \
             not at {over:?} where it had been over, and not at the bottom \
             either, because a real drag is not also a click",
        );
        checked += 1;
    });
    assert_eq!(checked, CASES);
}

/// ★★★★★ R1733 — **the board grows while something is carried**, by the three
/// rows the reference grows by.
///
/// Without it there is nowhere VISIBLE to drop a card below everything already
/// placed: the guide stops at the last occupied row, so the last row of the
/// board looks like the last row a drop can reach.
///
/// Counted out of the paint — the grid's own marks — rather than from its
/// rectangle. ★ The first draft measured the rectangle and read four rows for a
/// board of four: the grid lives inside the scrolling viewport, so its
/// resolved rectangle is the viewport's height whatever it asked for, and a
/// height that says "four" while seven rows of marks are drawn inside it would
/// have failed a true screen. The marks are the fact; the box around them is
/// the window they are seen through.
#[test]
fn r1733_the_board_grows_three_rows_while_something_is_carried() {
    let kind = first_placeable();
    let mut checked = 0;
    sweep(|state, shot, _, case| {
        let rows = state.board.get().rows();
        carry_to_middle(state, shot, kind);
        let (_, scene) = painted_at(case.size);
        let mut marks = None;
        scene.for_each_node(&mut |visit| {
            if visit.node.tag() == Some("shell.carry.grid")
                && let Scene::Container(container) = visit.node
            {
                marks = Some(container.children.len());
            }
        });
        let marks =
            marks.unwrap_or_else(|| panic!("{case}: the grid is a named part while carrying"));
        // One mark per grid intersection, so `(rows + 1) * (columns + 1)`.
        let per_row = usize::try_from(spec::GRID_COLS + 1).expect("a column count");
        assert_eq!(
            marks % per_row,
            0,
            "{case}: {marks} marks is not a whole number of rows of {per_row}",
        );
        let drawn = u32::try_from(marks / per_row).expect("a row count") - 1;
        assert_eq!(
            drawn,
            rows + 3,
            "{case}: a board of {rows} row(s) draws {drawn} while carrying, and \
             the reference draws three more than it holds",
        );
        checked += 1;
    });
    assert_eq!(checked, CASES);
}

/// ★★★★★ R1733 — and the cells those extra rows show are **reachable**: a
/// carry aimed below everything already placed lands below it.
///
/// The behaviour the three rows exist for, asserted separately from the marks
/// that advertise it — because a guide drawn over cells a drop cannot reach
/// would be worse than no guide.
#[test]
fn r1733_a_carry_reaches_the_rows_below_what_is_placed() {
    let kind = first_placeable();
    let mut checked = 0;
    let mut cases = 0;
    sweep(|state, shot, _, case| {
        cases += 1;
        let rows = state.board.get().rows();
        let (px, py) = aim(shot, &format!("shell.palette.{kind}"));
        ShellOracle::move_cursor(state, px, py);
        ShellOracle::press(state);
        let canvas = super::canvas_rect();
        let mut reached = 0;
        for step in 1..=3 {
            let want = rows + step;
            let y = canvas.y + super::GAP + want * super::ROW_H + super::ROW_H / 2;
            if y >= canvas.y + canvas.h {
                // Below the viewport at this size. A person scrolls to it and a
                // cursor cannot be put outside the window, so this is a limit
                // of the harness rather than of the board — and the case where
                // NO row is reachable is asserted below rather than skipped,
                // because "nothing was asked" must not read as "nothing was
                // wrong" (R1651.1).
                continue;
            }
            ShellOracle::move_cursor(state, canvas.x + canvas.w / 2, y);
            assert_eq!(
                state.drag.get().and_then(|d| d.landing()).map(|(_, r)| r),
                Some(want),
                "{case}: a carry aimed {step} row(s) below the board lands there",
            );
            reached += 1;
            checked += 1;
        }
        if reached == 0 {
            let first = canvas.y + super::GAP + (rows + 1) * super::ROW_H + super::ROW_H / 2;
            assert!(
                first >= canvas.y + canvas.h,
                "{case}: no row below the board was reached and the first one is \
                 inside the viewport, so the reason is not the window",
            );
        }
        ShellOracle::release(state);
    });
    // ★ The pin is the CASES, not the rows: a board taller than the viewport
    // legitimately reaches none of them from a cursor that cannot leave the
    // window, and that case proves its own reason above. Pinning the rows
    // instead would make a state that grew a card look like a regression.
    assert_eq!(cases, CASES);
    assert!(
        checked > 0,
        "no row below any board was reachable, so this check asked nothing",
    );
}

/// R1733 — a reserved row cannot be picked up, and says the same thing the
/// click says.
///
/// One refusal with one wording, not two: the pick-up and the action both go
/// through the catalogue's own tier, so a row that is booked for a later
/// release is un-draggable for the reason it is un-clickable.
#[test]
fn r1733_a_reserved_row_cannot_be_picked_up() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let reserved = spec::CATALOGUE
            .iter()
            .find(|w| w.tier == spec::Tier::Reserved)
            .expect("this release reserves at least one kind");
        let picked = ShellOracle::pick_up(&state, reserved.kind);
        let added = ShellOracle::add(&state, reserved.kind);
        assert!(picked.is_err(), "a reserved row is not a drag source");
        assert_eq!(
            format!("{:?}", picked.unwrap_err()),
            format!("{:?}", added.unwrap_err()),
            "and the pick-up refuses in the same words the click does",
        );
    });
}

/// ★★★★★ R1733 — **an agent reaches the cell a person's drag reaches.**
///
/// §2 #2: the headless path is the primary one, not a subset of it. A gesture
/// only a hand can perform would be a capability this framework's own premise
/// says must not exist — so `add` takes the cell, refuses a half-named one, and
/// clamps what it is given by the board's rule rather than trusting it.
#[test]
fn r1733_the_wire_places_at_a_cell_and_refuses_half_a_cell() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let kind = first_placeable();
        let before = state.board.get().rows();

        ShellOracle::add(&state, &format!("{kind},3,{}", before + 1))
            .expect("a kind and a cell place a card there");
        let placed = state
            .board
            .get()
            .tiles()
            .iter()
            .find(|t| t.row == before + 1)
            .map(|t| (t.col, t.row))
            .expect("the card is at the named cell");
        assert_eq!(placed, (3, before + 1));

        assert!(
            ShellOracle::add(&state, &format!("{kind},3")).is_err(),
            "a placement names both a column and a row, or neither",
        );
        assert!(
            ShellOracle::add(&state, &format!("{kind},a,b")).is_err(),
            "and a cell that is not a number is refused rather than rounded to one",
        );

        // Past the right edge: clamped by the board, exactly as the pointer is.
        ShellOracle::add(&state, &format!("{kind},11,{}", before + 4))
            .expect("a column past the edge is a gesture, not an error");
        let clamped = state
            .board
            .get()
            .tiles()
            .iter()
            .find(|t| t.row == before + 4)
            .map(|t| (t.col, t.col + t.w))
            .expect("the card is on the board");
        assert!(
            clamped.1 <= spec::GRID_COLS,
            "a placement past the right edge stops on the board: {clamped:?}",
        );
    });
}

// ── R1761: the dashboard against the pin, through the function the window uses ─

/// ★★★★★ R1761 — **what this section paints, against
/// `docs/analyzer-dashboard-spec.json`, in every state and at every size the
/// sweep drives.**
///
/// Everything above this line compares the screen with `crate::spec` — the
/// screen's own table, written in the same edit as the painter it feeds. This
/// one compares it with a pin extracted from the behaviour reference, and the
/// difference is why both exist: the first says this build is self-consistent,
/// and only the second can say it is the reference.
///
/// Judged through [`crate::judge::built`] — the SAME function the running
/// window and the assembled application answer from, so a copy of these
/// readings kept here would be the second account whose disagreement nobody
/// notices, because the one running in a window is not the one anybody runs.
///
/// # The rule is in three parts, and the third is the one with teeth
///
/// 1. In the state the specification describes, at the size it describes, every
///    surface **reconciles**: the difference this build has is exactly the
///    difference somebody wrote down.
/// 2. At every OTHER state the sweep drives, `board` is exempt — those states
///    add a card, maximise one, shrink them all. What the pin fixes is the
///    board a reader arrives at, and the exemption is a state the sweep NAMES
///    (`Case::state`) rather than a condition under which this check fails.
/// 3. At every state and size, an undeclared difference on the other four
///    surfaces may only be an **absence**, or a reordering with an absence
///    beside it to explain it: shrinking a window may take a part off the
///    frame, and it must never rename one or grow one the reference has not.
///
/// ★★★★★ R1770 — whether this frame is the one a specification describes.
///
/// Read off the PIN and compared with what the frame recorded, rather than
/// against this module's own window constants. Two constants in two files
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

/// ★ R1770 — a sweep over several sizes visited the one its pin was written at.
///
/// Counted rather than asserted per case, because such a sweep visits several
/// sizes on purpose: a verdict read anywhere else is a different claim and the
/// report says so, but the strict reading has to happen somewhere or the pin's
/// `$at` is a number nothing checks.
#[cfg(test)]
fn assert_swept_the_declared_extent(
    at_declared: usize,
    doc: &pinion_core::conformance::SpecDocument,
) {
    assert!(
        at_declared > 0,
        "★★★★★ R1770 — this sweep never painted at the extent its pin says its \
         canon was written against ({:?}), so every case judged a size the \
         specification does not describe.",
        doc.written_at(),
    );
}

#[test]
fn r1761_the_dashboard_reproduces_its_specification_or_says_why_not() {
    use pinion_core::conformance::{Built, PartDivergence, Unreconciled};
    use pinion_screen::Showing;

    let mut judged = 0usize;
    let mut reconciled = 0usize;
    let mut at_declared = 0usize;
    sweep(|_, _, _, case| {
        let regions =
            pinion_core::painted::painted_regions(super::VIEW_TAG).expect("the sweep just painted");
        let doc = spec::dashboard_document();
        at_declared += usize::from(at_the_declared_extent(&regions, &doc));
        for surface in doc.surfaces() {
            let Built::Standing(parts) = super::judge::built(&regions, surface, Showing::OnScreen)
            else {
                panic!(
                    "{case}: `{surface}` is away while this page is the one painted -- the only \
                     away this section has is the reader being somewhere else"
                );
            };
            judged += 1;
            let said: Vec<String> = doc
                .unreconciled(surface, &parts)
                .iter()
                .map(Unreconciled::sentence)
                .collect();
            if case.as_specified() {
                assert!(
                    said.is_empty(),
                    "{case}: `{surface}` is not what \
                     docs/analyzer-dashboard-spec.json declares:\n  {}",
                    said.join("\n  "),
                );
                reconciled += 1;
                continue;
            }
            if surface == "board" {
                // Part 2 — this sweep's later states are edits to the board.
                continue;
            }
            let canon = doc.canon(surface).expect("the document fixes it");
            let undeclared: BTreeSet<String> = doc
                .unreconciled(surface, &parts)
                .into_iter()
                .filter_map(|entry| match entry {
                    Unreconciled::Undeclared { sentence, .. } => Some(sentence),
                    // ★ R1770 — `Unsized` joins the arms this filter drops: it
                    // is a refusal to judge an entry, not a difference the
                    // build has, and this set is the differences.
                    Unreconciled::Paid { .. }
                    | Unreconciled::Reworded { .. }
                    | Unreconciled::Unsized { .. } => None,
                })
                .collect();
            let divergences: Vec<PartDivergence> = canon.diff(&parts);
            let absent: BTreeSet<&str> = divergences
                .iter()
                .filter_map(|d| match d {
                    PartDivergence::Absent { key, .. } => Some(key.as_str()),
                    _ => None,
                })
                .collect();
            for difference in &divergences {
                let sentence = difference.sentence();
                if !undeclared.contains(&sentence) {
                    continue;
                }
                let allowed = match difference {
                    PartDivergence::Absent { .. } => true,
                    PartDivergence::OutOfOrder { .. } => !absent.is_empty(),
                    PartDivergence::Unspecified { .. } | PartDivergence::Retitled { .. } => false,
                };
                assert!(
                    allowed,
                    "{case}: `{surface}` differs from the pin in a way nobody \
                     declared and a smaller window cannot explain: {sentence}",
                );
            }
        }
    });
    assert_eq!(judged, CASES * 5, "every surface was judged in every case");
    assert_eq!(
        reconciled, 5,
        "and all five reconcile in the one case the specification describes",
    );
    assert_swept_the_declared_extent(at_declared, &spec::dashboard_document());

    // ★★ And the away this section DOES have, which no size or state can
    // produce: the reader standing somewhere else. Asserted here rather than
    // left to the demo, because it is the one answer the judge cannot derive
    // from its own marks and the one a refactor could quietly drop.
    // (The preferences page's own version of both halves is the test below.)
    let owner = Owner::new();
    owner.run(|| {
        let _ = painted_at((WIN_W, WIN_H));
        let regions =
            pinion_core::painted::painted_regions(super::VIEW_TAG).expect("the sweep just painted");
        let doc = spec::dashboard_document();
        for surface in doc.surfaces() {
            match super::judge::built(&regions, surface, Showing::Elsewhere) {
                Built::Away(why) => assert!(
                    why.contains("another section"),
                    "the away names where the reader is, not what the judge failed to find: {why}"
                ),
                Built::Standing(_) => panic!(
                    "`{surface}` reports standing from a page the reader is not on, which is the \
                     defect a host's own store makes possible"
                ),
            }
        }
    });
}

/// ★★★★★ R1762 — **an open roster hangs off the control it belongs to**, and
/// the two are compared against the PAINT rather than against each other.
///
/// Found by a counterfactual that PASSED: pointing the roster at the other
/// value row's control changed nothing any gate could see. The demo presses the
/// roster's options by reading their rectangles back out of the frame, so a
/// roster laid over the wrong row is still pressed correctly — it is simply in
/// the wrong place, which is exactly the kind of defect a driver cannot feel and
/// a reader cannot miss.
///
/// The comparison is the roster's own box against the CHOOSER'S PAINTED
/// RECTANGLE, which are two derivations: one is what `chooser::lay_roster`
/// computed, the other is where the frame actually drew the control.
#[test]
fn r1762_an_open_roster_hangs_off_the_control_it_belongs_to() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let mut shot = painted_at_destination("settings");
        for row in spec::VALUE_ROWS {
            let tag = format!("shell.settings.choose.{}", row.key);
            press_tag(&state, &shot, &tag);
            shot = painted_at((WIN_W, WIN_H)).0;
            let control = shot
                .rect(&tag)
                .unwrap_or_else(|| panic!("the frame drew {}'s control", row.key));
            let roster = shot
                .rect(&format!("shell.settings.roster.{}", row.key))
                .unwrap_or_else(|| panic!("pressing {}'s control opens its roster", row.key));
            assert_eq!(
                (roster.x, roster.w),
                (control.x, control.w),
                "{}: the roster is the control's own width, in the control's own \
                 column",
                row.key,
            );
            assert!(
                roster.y >= control.y + control.h || roster.y + roster.h <= control.y,
                "{}: the roster hangs clear of the control rather than over it: \
                 {roster:?} vs {control:?}",
                row.key,
            );
            // Close it again, so the next row starts from the same state this
            // one did.
            press_tag(&state, &shot, &tag);
            shot = painted_at((WIN_W, WIN_H)).0;
        }
    });
}

/// ★★★★★ R1762 — **what the PREFERENCES page paints, against
/// `docs/analyzer-settings-spec.json`, through the function the window uses.**
///
/// The sibling of the dashboard's test above, and it exists because the round
/// that wrote it measured its absence: eight counterfactuals were run against
/// `cargo test -p hello-analyzer-shell` and **six passed** — every break in the
/// preferences judge, its stems, its titles and its tables went unnoticed,
/// because nothing in this crate's own suite drove that judge at all. A demo
/// caught them, and a demo is not a gate a `cargo test` runs.
///
/// The rule is the dashboard's, with the same two parts: at the size and scroll
/// position the specification describes, every surface reconciles; and the away
/// this section has is the reader standing somewhere else, never *I found none
/// of my parts*.
#[test]
fn r1762_the_preferences_page_reproduces_its_specification_or_says_why_not() {
    use pinion_core::conformance::{Built, Unreconciled};
    use pinion_screen::Showing;

    let owner = Owner::new();
    owner.run(|| {
        let _ = painted_at_destination("settings");
        let regions =
            pinion_core::painted::painted_regions(super::VIEW_TAG).expect("the sweep just painted");
        let doc = spec::settings_document();
        let mut judged = 0usize;
        for surface in doc.surfaces() {
            let Built::Standing(parts) =
                super::judge::settings_built(&regions, surface, Showing::OnScreen)
            else {
                panic!(
                    "{surface}: away while this page is the one painted — the only away this \
                     section has is the reader being somewhere else"
                );
            };
            judged += 1;
            // ★★★★★ R1770 — judged AT the extent this frame was painted into,
            // because one entry of this page's ledger is a fold and a fold is a
            // function of how tall the surface is. A gate that passed no extent
            // would be refused by that entry rather than excused by it, which
            // is the point: this page's verdict is a claim about a size.
            let said: Vec<String> = doc
                .unreconciled_at(surface, regions.extent(), &parts)
                .iter()
                .map(Unreconciled::sentence)
                .collect();
            assert!(
                said.is_empty(),
                "`{surface}` is not what docs/analyzer-settings-spec.json declares \
                 at {:?}:\n  {}",
                regions.extent(),
                said.join("\n  "),
            );
        }
        assert_eq!(
            regions.extent(),
            doc.written_at(),
            "★★★★★ R1770 — this gate paints at the extent \
             docs/analyzer-settings-spec.json declares its canon was written at. \
             If these part company, every judgement above is about a size the \
             specification does not describe.",
        );
        assert_eq!(
            judged,
            doc.surfaces().count(),
            "every surface the document names was judged",
        );
        assert!(
            judged >= 7,
            "the document names the page's surfaces: {judged}"
        );

        // ★★ And the away, which no size can produce.
        for surface in doc.surfaces() {
            match super::judge::settings_built(&regions, surface, Showing::Elsewhere) {
                Built::Away(why) => assert!(
                    why.contains("another section"),
                    "the away names where the reader is, not what the judge failed to find: {why}"
                ),
                Built::Standing(_) => {
                    panic!("`{surface}` reports standing from a page the reader is not on")
                }
            }
        }
    });
}
