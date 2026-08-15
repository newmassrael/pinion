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
    use_shell_state().surface.set(size);
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
                    let inside: BTreeSet<String> = shot
                        .tags
                        .keys()
                        .filter(|tag| {
                            tag.starts_with("shell.settings.") || tag.starts_with("card.")
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

        // ★ The two pages are DIFFERENT pages. Without this the check above is
        // satisfied by a region that paints the dashboard whatever the journey
        // says — which is the exact defect this round repairs, and it would
        // otherwise pass every assertion above.
        let pages: Vec<&BTreeSet<String>> = seen.values().collect();
        assert_eq!(pages.len(), 2, "two open destinations, measured");
        assert!(
            pages[0].is_disjoint(pages[1]),
            "two destinations painted overlapping content: {:?}",
            pages[0].intersection(pages[1]).collect::<Vec<_>>(),
        );
    });
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
        for destination in roster.open() {
            let key = destination.key.as_ref();
            if key != state.at() {
                state.go(key).expect("an open destination is reachable");
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
            let announced: BTreeSet<String> = super::AnalyzerShellView::access_node(&(), None)
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
            let announced: BTreeSet<String> = super::AnalyzerShellView::access_node(&(), None)
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
