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
        // ★ R1797 — a card is CLOSED to make room before one is added. The
        // board holds five since the latency card was promoted, and this shell
        // does not scroll its canvas
        // (`debt-the-analyzer-canvas-does-not-scroll`), so an unconditional add
        // put the new card below the viewport: the model said it was shown and
        // the frame had never drawn it, which every containment gate here then
        // reported as the screen failing to paint a card. Swapping rather than
        // appending keeps what this state is FOR — two cards of one kind, so an
        // ordinal is exercised — without also asserting a scroll this screen
        // does not have.
        // ★★★★★ The second card is PLACED AT A NAMED CELL, and nothing is
        // closed to make room. Three attempts, and the two that failed are
        // worth the lines because both failure modes are structural:
        //
        // * An unconditional `add` places at `(0, board.rows())` — below every
        //   existing row. The board has held five cards since the latency card
        //   was promoted, so the sixth landed past the viewport and this shell
        //   does not scroll its canvas
        //   (`debt-the-analyzer-canvas-does-not-scroll`): the model said the
        //   card was shown and no frame had ever drawn it.
        // * Closing a card to make room removes THAT card's clamps for the
        //   rest of the sweep, because these states are cumulative. Whichever
        //   card is chosen, the clamp census reports its guards as unexercised
        //   — correctly. There is no card that can be spared.
        //
        // The board's last placement is four columns wide in a twelve-column
        // grid, so the cell beside it holds the widest kind the palette has.
        // Derived from the specification rather than written down, so a board
        // that changes shape fails the `expect` instead of quietly placing a
        // card somewhere nobody looks.
        //
        // ★★★★★ R1851 — the kind added is the SHORTEST placed one, and that is a
        // measurement rather than a tidy-up. The board is exactly full (twelve
        // columns by four rows is 48 cells and the seven placements fill them),
        // so any add pushes the bottom card down. A TWO-row add pushes it two
        // rows, which puts it entirely outside the canvas's clip: `cell_rect`
        // puts row 5 at y=886 in an 802-tall canvas, so nothing of it intersects
        // the viewport, the model still says the card is shown, and every
        // containment gate here reports the screen failing to paint a card. A
        // one-row add pushes one row, and row 4 still intersects the clip.
        //
        // Derived from the board rather than named, so a placement that changes
        // height moves this with it — the same discipline as the cell below.
        let tail = spec::BOARD.last().expect("the board is not empty");
        let shortest = spec::BOARD
            .iter()
            .min_by_key(|placed| placed.rows)
            .expect("the board is not empty");
        ShellOracle::add(
            state,
            &format!("{},{},{}", shortest.kind, tail.col + tail.cols, tail.row),
        )
        .expect("a placeable kind fits beside the board's last placement");
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

/// How many runs on this screen sit in a box too short for their own face.
///
/// ★ R1800 — a ratchet PIN, measured rather than wished at zero. The population
/// across this tree was measured at 289 of 290 runs on one screen: not a
/// backlog of slips but a convention that never consulted the face. Lowering
/// this is the repair, and `containment::line_rect` is how.
/// ⚠ R1843 moved it 242 -> 248, and the SHAPE of that move is the finding.
///
/// The screen gained a sixth card, so the population grew — and the round's
/// first attempt read the pin as a ceiling and set it to the peak, 263. It is
/// not a ceiling: this gate says *"the budget is a PIN, not a ceiling: N run(s)
/// are short, so lower it"*, and that refusal is what produced the number here.
///
/// ★★★★★ Between those two figures the round derived the tile's row heights
/// from [`line_box`](pinion_core::containment::line_box) instead of authoring
/// them at the font size, and **fifteen short runs stopped being short** —
/// 263 -> 248 while the card grew. A 12px label in a 12px box overflows by
/// construction, so the new card had been contributing to this backlog on
/// every row it drew. The pin therefore rises by six for the population and
/// falls by fifteen for the repair, which is the direction this ratchet exists
/// to record.
///
/// The repair for the remainder is unchanged: `containment::line_rect` at the
/// sites that still reserve a line at the face's own size.
///
/// ★★★★★ R1851 — **248 -> 211 while the screen grew a SEVENTH card**, and the
/// thirty-seven were found by asking the question of one card instead of the
/// screen.
///
/// The alarm card's own gate (`r1851_no_alarm_run_sits_in_a_box_too_short_for_
/// its_own_face`) asks for ZERO, and its first run reported nothing about the
/// alarm feed at all: it reported the card's size-stepper strip. Every run in
/// that band — four stepper glyphs, two axis letters and the size reading — had
/// been authored into 14px boxes for faces needing 18 and 20, on every card, in
/// every editing state, at every size. Seven per card, invisible for as long as
/// this number was a screen-wide ratchet: seven runs of one band sit under the
/// noise of a population of 248, and a seventh card would have pushed the total
/// over the line for a reason that had nothing to do with the card.
///
/// ⇒ **A ratchet is the right shape for a backlog and the wrong shape for a
/// surface being written now.** The per-card zero gate is what a new card should
/// carry, and this pin is what the backlog is measured by.
///
/// ★ R1864 — 211 -> 208, and all three left for one reason: a box whose height
/// comes from `pinion_core::containment::line_box` cannot be short of its own
/// face. The gesture sentence moved into the status band, and the palette's two
/// counts moved into the panel's footer band; every one of them had been
/// authored `16` or `14` tall for an 11-pixel face needing 18.
///
/// ★ R1865 — 208 -> 207: the toast's own sentence, for the same reason one more
/// time. It was a 16-pixel box set in a 12-pixel face; in the band it is the
/// slot's height, taken from `line_box(STATUS_FACE)`.
const SHORT_BOX_BUDGET: usize = 207;

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

    /// How many ROWS of a family are painted — distinct `{n}`, not every tag
    /// beneath it.
    ///
    /// ★★★★★ R1843 — [`Self::family`] counts descendants, and every body in
    /// this screen used to have none: a row was one container with untagged
    /// text inside it, so "tags under the stem" and "rows" were the same
    /// number by accident of how the bodies happened to be built. The health
    /// card is the first whose rows come from a CRATE, and
    /// `pinion_widget_paint::stat_tile` tags each word's own box on purpose —
    /// that is what stops a word being filed under whatever encloses it. So
    /// the same five rows answered as fifty, and the gate read a richer body
    /// as a wrong one.
    ///
    /// Counting the segment after the stem is what "rows" always meant. It is
    /// unchanged for every body that has no tagged descendants, which is why
    /// this is a repair rather than an exemption for one card.
    fn rows(&self, stem: &str) -> usize {
        self.tags
            .keys()
            .filter_map(|t| t.strip_prefix(stem))
            .map(|rest| rest.split('.').next().unwrap_or(rest))
            .collect::<std::collections::BTreeSet<_>>()
            .len()
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
            let painted = shot.rows(&stem);
            // ★ R1797 — a family whose rows are all-or-nothing may legitimately
            // paint none of them: the latency card's three tiles go together
            // when the card is too narrow for one of them to say anything, the
            // way the filter card's do. For a family whose cells drop one at a
            // time, painting nothing IS the defect this line was written for —
            // a placeholder passing as a body — so the two are distinguished by
            // the same table the rest of this reads, not by a name here.
            assert!(
                painted > 0 || body_cells(kind) == Cells::Whole,
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

/// Whether a body's rows give up their columns one at a time.
///
/// ★ R1797 — stated rather than assumed. The clamp census below asks every
/// multi-column row whether it has ever been seen dropping a cell, and reports
/// a clamp nothing exercises as a guard that could be deleted with no gate
/// noticing. That question is only meaningful for a row that CAN drop a cell:
/// the latency card's tiles are laid out at a fixed width and drawn whole or
/// not at all, so asking it produces a clamp that can never be reached and a
/// permanently red gate. Naming the exception in the census would put the
/// knowledge where the census is rather than where the painter is; this puts it
/// in the one table both read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cells {
    /// A column too narrow to say anything is dropped from the right.
    Droppable,
    /// The row is drawn whole or not at all; its clamp is the family's own.
    Whole,
}

/// The tag family a kind's body rows are painted under, how many rows the
/// specification gives it, and whether its cells drop one at a time.
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
        // ★ R1797 — the latency card's rows are its three STAT TILES, not its
        // bars. The bars carry no words of their own: a bar chart puts the
        // category label on its axis, so `run_words` against a bar tag is
        // empty by construction and a positional word check there would assert
        // nothing. The tiles are where this card's reproduction claim actually
        // lives — they are the three figures the reference publishes, and the
        // card DERIVES them from its samples, so comparing painted words with
        // the specification's numbers is a check that can fail.
        "latency" => ("stat", spec::LATENCY_STAT_KEYS.len()),
        // ★ R1843 — the health card's rows are its five KPI tiles, for the same
        // reason the latency card's are its three: the sparkline beside each
        // one carries no words, so a word check against a spark tag would
        // assert nothing. The tiles are where this card's reproduction claim
        // lives.
        "health" => ("stat", spec::HEALTH_TILES.len()),
        // ★★★★★ R1851 — the alarm feed's rows, and the count is the WINDOW rather
        // than the table. Every other family here counts a specification table;
        // this one cannot, because the feed is virtualised: it constructs the
        // rows that fit its body plus an overscan row at each end, and eighteen
        // alarms are never eighteen rows. `ALARM_ROWS_SHOWN` is that window at
        // the opening size, and `r1851_the_feed_builds_only_the_window_it_shows`
        // is what keeps the pin honest.
        "alarms" => ("feed.row", spec::ALARM_ROWS_SHOWN),
        _ => return None,
    })
}

/// Whether `kind`'s body rows drop columns one at a time.
fn body_cells(kind: &str) -> Cells {
    match kind {
        // ★ R1843 added `health` beside `latency`, and the two are one claim:
        // a stat tile's words go together. The label, the reading and the
        // change are a single statement, and a tile showing two of the three
        // says something a reader would misread — so both strips drop whole
        // TILES as they narrow and never a tile's cells. The clamp gate is
        // what asked for the declaration: it refuses a guard nothing
        // exercises, and nothing will ever exercise a cell clamp that cannot
        // happen.
        // ★ R1851 added `alarms` to the same claim. An alarm row says one
        // thing — how bad, when, and what — and a row showing two of the three
        // is a sentence a reader completes wrongly: a time and a message with
        // no severity reads as routine. So the row's words go together, and the
        // clamp that happens as the card narrows is a WORD giving way inside
        // its own column box (which `TextOverflow::Ellipsis` shows) rather than
        // a column being dropped. A cell clamp declared here would be a guard
        // nothing exercises, which the census below refuses by name.
        "latency" | "health" | "alarms" => Cells::Whole,
        _ => Cells::Droppable,
    }
}

/// The text runs the painted scene put inside one tag.
fn run_words(shot: &Painted, tag: &str) -> Vec<String> {
    // ★★★★★ R1843 — a row's words, wherever UNDER it they are filed.
    //
    // This asked for runs owned by the tag EXACTLY, which found every word
    // while a body row was one container with untagged text inside it: the
    // nearest tagged ancestor of each word was the row itself. A row built by
    // `pinion_widget_paint::stat_tile` tags each word's own box — which is the
    // thing that stops a word being filed under whatever encloses it — so its
    // words answer to `…stat.0.label` and this returned nothing at all. The
    // gate read a row painting three words as a row painting none.
    //
    // The third site of one repair, with `Painted::rows` and the disjointness
    // check: each assumed a flat body, and each had been exactly right until a
    // body stopped being flat. A descendant's word is still the row's word.
    let nested = format!("{tag}.");
    shot.runs
        .iter()
        .filter(|(_, _, owner)| {
            owner
                .as_deref()
                .is_some_and(|o| o == tag || o.starts_with(&nested))
        })
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
        // ★ R1797 — the reference's own published figures, rendered from the
        // specification's constants rather than from the painter's derivation.
        // Two accounts of one number ON PURPOSE, which is unusual here and is
        // the point: the card computes these from a hundred samples and this
        // states what the reference says they are, so the assertion between
        // them can fail. Reading the painter's own helper instead would be the
        // comparison that cannot.
        "latency" => {
            let (key, value) = spec::latency_tile(n);
            vec![key.into(), value]
        }
        // ★ R1843 — the health tile's three words, in the order the tile paints
        // them: what is counted, the reading, and the change. The reading is
        // rebuilt here the way the body builds it (value, then unit when there
        // is one) rather than read back from the painter, for the reason the
        // latency arm above states — a check against the painter's own helper
        // is one that cannot fail.
        "health" => {
            let tile = spec::HEALTH_TILES[n];
            let heading = if tile.unit.is_empty() {
                tile.label.to_owned()
            } else {
                format!("{} {}", tile.label, tile.unit)
            };
            vec![heading, tile.value.into(), tile.delta.into()]
        }
        // ★★★★★ R1851 — a SECOND account of the feed's order, on purpose, which
        // is the `latency` arm's rule applied to an ordering instead of to a
        // number. The card asks `compute_order` through the severity scale; this
        // sorts the specification's own table right here. Two implementations of
        // one rule, so the assertion between them can fail — where reading the
        // painter's own `alarm_order` would be a comparison that cannot.
        //
        // ⚠ Valid because the sweep never moves the feed's order: the opening
        // sort is `ALARM_OPENING_SORT` (time, descending) with no floor, and
        // nothing in a swept case sorts or filters. The moment a case does, this
        // arm is wrong and says so by failing.
        "alarms" => {
            assert_eq!(
                spec::ALARM_OPENING_SORT,
                (1, false),
                "this oracle sorts by time descending; the feed opens elsewhere"
            );
            let mut newest: Vec<&spec::AlarmSpec> = spec::ALARMS.iter().collect();
            newest.sort_by_key(|a| std::cmp::Reverse(a.seconds()));
            let alarm = newest[n];
            vec![
                alarm.severity.to_uppercase(),
                alarm.clock(),
                alarm.message.to_owned(),
            ]
        }
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
    // ★ R1774 — the record and the three assertions are the framework's now
    // (`pinion_core::test_fixtures::clamp`), because the two sibling screens
    // ask the same question and a verbatim third copy is how three screens come
    // to disagree about what an unexercised guard is. What stays here is the
    // only part that is this screen's: WHICH observables it has and how to read
    // one off a painted frame.
    let mut census = pinion_core::test_fixtures::clamp::ClampCensus::new();
    let mut note = |what: String, clamped: bool| census.note(what, clamped);

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
            //
            // ★ R1843 — `rows`, not `family`. The latter counts every tag under
            // the stem, which equalled the row count only while bodies had no
            // tagged descendants; with a body whose rows tag their own words it
            // over-counts, and `0..painted` below then indexes a specification
            // table past its end. That is what it did: `the len is 5 but the
            // index is 5`.
            let painted = shot.rows(&format!("card.{id}.{family}."));
            note(format!("{kind}: rows"), painted < rows);
            // And the CELLS of each painted row, which is a second clamp with
            // the same shape: a column too narrow to say anything is dropped.
            //
            // ★ Only for a row that HAS a column to lose. A one-cell row cannot
            // be clamped and remain a row -- dropping its only cell is the row
            // going, which is the observable above. Derived rather than
            // excluded by name: this gate found the filter card's chips that
            // way and made the reason be stated instead of assumed.
            if body_cells(kind) == Cells::Droppable {
                for n in 0..painted {
                    let wanted = specified_row(kind, n).len();
                    if wanted < 2 {
                        continue;
                    }
                    let cells = run_words(shot, &format!("card.{id}.{family}.{n}")).len();
                    note(format!("{kind}: cells"), cells < wanted);
                }
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
            // ★ R1797 — the latency card's own two, both all-or-nothing: the
            // tiles go when a tile would be narrower than it can say anything
            // in, and the distribution goes when the plot would be narrower
            // than one pixel per bucket. The second is the clamp this round
            // added and it is registered here so a sweep that never reaches a
            // small card reports the guard as unexercised rather than passing.
            if kind == "latency" {
                note(
                    "latency: stat tiles".to_owned(),
                    shot.family(&format!("card.{id}.stat.")).is_empty(),
                );
                note(
                    "latency: distribution".to_owned(),
                    shot.rect(&format!("card.{id}.bins")).is_none(),
                );
            }
        }
    });

    // The population floor is what it always was: a derivation that quietly
    // yields nothing is the failure this whole module's populations are written
    // to avoid (R1651.1), and the shared check requires the floor rather than
    // defaulting it for exactly that reason.
    census.assert_both_sides_reached("dashboard", 9);
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
        // ★ R1864 — `shell.status` joined them, and it is the pane that reaches
        // the window's bottom edge now: the rail and the palette stop at the
        // band. A list that had not learned the new pane would have read as the
        // screen failing to fill its window by exactly the band's height, which
        // is what its first run said.
        for pane in [
            "shell.appbar",
            "shell.rail",
            "shell.palette",
            "shell.subbar",
            super::STATUS_BAND,
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
                    .map(|(key, _)| format!("shell.palette.section.{key}")),
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
/// ★ R1800 — and was each run's OWN box authored tall enough for its face?
///
/// The check below asks whether a mark left its PARENT and answers *no* on this
/// screen. That is true and it is not the whole question: a pane roomy enough
/// for its rows says nothing about a row three pixels short of the line it
/// holds, which is what a reader sees as a cut descender. Pure scene
/// arithmetic — no font, so no CI disagreement.
#[test]
fn r1800_no_run_sits_in_a_box_too_short_for_its_own_face() {
    use pinion_core::test_fixtures::screen_ink::assert_boxes_hold_their_text;
    let mut worst = 0;
    sweep(|_, _, scene, case| {
        worst = worst.max(assert_boxes_hold_their_text(
            case.name,
            scene,
            SHORT_BOX_BUDGET,
        ));
    });
    assert_eq!(
        worst, SHORT_BOX_BUDGET,
        "the budget is a PIN, not a ceiling: {worst} run(s) are short, so lower it"
    );
}

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

/// ★★★★★ R1797 — a card is painted **once** per frame.
///
/// Written as a hypothesis, which is why it is worth keeping whichever way it
/// answers. R1671's frame gate reported the latency card's bars 54 pixels below
/// where the card's own rectangle put them, and every arithmetic route from the
/// card rect to the bars said that was impossible. The remaining explanation is
/// that two nodes carry the tag, so a mark from one is being judged against the
/// other's rectangle — which would make the frame gate's comparison wrong for
/// any card whose body reaches the bottom, and text rows never do.
///
/// If a card really is drawn twice under one address then that is the defect
/// and the frame gate is an innocent reporter of it; every tag-addressed read
/// in this screen — the wire, the hit test, the accessibility tree — resolves
/// one of the two arbitrarily.
#[test]
fn r1797_a_card_is_painted_once_per_frame() {
    let mut doubled: Vec<(String, String, Vec<Rect>)> = Vec::new();
    sweep(|state, shot, scene, case| {
        for card in state.placed() {
            let tag = format!("card.{}", card.id().as_str());
            let mut seen = Vec::new();
            scene.for_each_node(&mut |visit| {
                if visit.node.tag() == Some(tag.as_str())
                    && let Some(at) = visit.absolute_rect()
                {
                    seen.push(at);
                }
            });
            if seen.len() > 1 {
                doubled.push((case.name.to_owned(), tag, seen));
            }
        }
        // The shot is read so this walks the same frame the gates beside it do,
        // rather than a scene nobody rendered.
        let _ = shot.runs.len();
    });
    assert!(
        doubled.is_empty(),
        "{} card(s) were painted more than once under one tag: {:?}",
        doubled.len(),
        &doubled[..doubled.len().min(4)]
    );
}

#[test]
fn r1671_nothing_a_card_paints_crosses_its_own_frame() {
    sweep(|state, _, scene, case| {
        // ★★★★★ R1797 — the card rectangles come from the SAME walk, in the
        // SAME frame, as the marks compared against them. They used to come
        // from the painted regions, which are CLIPPED, while the marks were
        // walked with `absolute_rect`, which is also clipped — and clipping
        // does not preserve the relation this gate is about. A card dragged
        // part of the way off the canvas has its rectangle cut at the clip
        // edge; a mark inside it that reaches the bottom is cut at the same
        // edge; and the content box is the CUT card inset by the frame, which
        // now sits `CARD_FRAME` pixels ABOVE where both were cut. So every
        // mark reaching the boundary was reported as crossing the frame, by
        // exactly two pixels, always.
        //
        // Nothing had ever reached it: the bodies here are text rows that stop
        // short of the bottom, and the first body whose content is a chart
        // filling its box found it immediately. Unclipped on both sides —
        // `offset` plus the node's own rect, which is what the walk documents
        // as *where it would be* — asks the question the gate is named for.
        let unclipped = |visit: &pinion_core::scene::NodeVisit<'_, '_>| -> Option<Rect> {
            let r = visit.node.rect();
            Some(Rect::new(
                u32::try_from(visit.offset.0 + i64::from(r.x)).ok()?,
                u32::try_from(visit.offset.1 + i64::from(r.y)).ok()?,
                r.w,
                r.h,
            ))
        };
        let shown: BTreeSet<String> = shown_cards(state).into_iter().collect();
        let mut cards: BTreeMap<String, Rect> = BTreeMap::new();
        scene.for_each_node(&mut |visit| {
            let Some(tag) = visit.node.tag() else { return };
            let Some(id) = tag.strip_prefix("card.") else {
                return;
            };
            if shown.contains(id)
                && let Some(at) = unclipped(&visit)
            {
                cards.insert(id.to_owned(), at);
            }
        });
        if cards.is_empty() {
            return;
        }
        let mut crossing: Vec<(String, String, Rect, Rect)> = Vec::new();
        // ★ How many opaque marks the walk actually WEIGHED. Without it a gate
        // that stopped looking -- a predicate that never matches, a population
        // that derives to nothing -- passes and reads as coverage. Two rounds
        // running, a counterfactual found exactly that in a gate this session
        // wrote, so this one carries its own floor.
        let mut weighed = 0_usize;
        scene.for_each_node(&mut |visit| {
            let Some(rect) = unclipped(&visit) else {
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
                    // ★ R1797 — the CONTENT box travels with the finding. The
                    // message used to name the card and the mark and leave the
                    // box it was judged against to be re-derived by whoever
                    // read it, which cost this round three rounds of arithmetic
                    // against the wrong assumption about where the card was. A
                    // gate that says what it compared can be diagnosed from its
                    // own output.
                    crossing.push((
                        tag,
                        visit.node.tag().unwrap_or("<untagged>").to_owned(),
                        rect,
                        content,
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

/// Kinds allowed to announce a row they do not paint.
///
/// ★★★★★ R1843 — a RATCHET, not an exemption, on R1807's `UNASSEMBLED` model.
/// The gate below refuses a card that tells a reader about a row nobody drew;
/// this names the ones that already did when the rule landed, so a NEW ghost
/// cannot join them silently and the backlog is a list a reader can shorten
/// rather than a number in a summary line.
///
/// ⚠⚠ **Every card this round did not build is on it**, and each name was
/// MEASURED — the gate was run, the card it named was added, and it was run
/// again, five times over. Shrunk to one cell, each announces a row it does not
/// paint: `packet` row 5 of its stream, `decode` row 6 of its tree, `keymap`
/// row 5 of its map, `filter` row 3 of its chips, `latency` row 0 of its stat
/// strip. Not one was guessed from the pattern, because a name here is a claim
/// that a card HAS this defect and an unproven one would be a slander that
/// silences a real gate.
///
/// ★★★★★ The card that produced this rule — `health` — is deliberately NOT on
/// the list. It was REPAIRED rather than admitted, by giving the paint and the
/// accessibility tree one shared count (`health_tile_count`). Admitting the
/// defect that motivated the gate would have made the gate ornamental.
///
/// So this list is the shape of the finding: the ghost is not a new card's
/// mistake, it is what every body on this screen does, and it was invisible
/// because nothing asked. Shortening it is the repair — a body's node builder
/// must consult whatever its painter consults.
///
/// ⇒ `debt-a-card-announces-a-row-it-does-not-paint`.
/// ★★★★★ R1851 added `alarms`, and the reason is a MODEL limit rather than a
/// card that gets a pass.
///
/// This gate reads *painted* off the finished frame, which is after the canvas's
/// clip. Every other card is fully inside that clip in every swept state, so
/// "painted" and "constructed" are the same set for them. The alarm card is the
/// first that a swept state pushes partly OUTSIDE it: the board is exactly full,
/// so adding a second card of a placed kind pushes the bottom row down one, and a
/// row that exists and is scrolled away is a row the frame does not record. The
/// gate then reads that as a row nobody drew, which is a true statement about the
/// paint and a false one about the tree.
///
/// The claim is not lost — it is asserted where it can be exact, against the
/// window the assembly actually built rather than against the clip:
/// `r1851_the_feed_builds_only_the_window_it_shows`, and on the wire in
/// `tools/demos/r1851_an_alarm_feed_is_graded_ordered_and_windowed.py` section F.
const GHOSTS: &[&str] = &["packet", "decode", "keymap", "filter", "latency", "alarms"];

/// ★★★★★ R1843 — **a card announces the rows it PAINTS, and no others.**
///
/// This gate exists because a counterfactual found nothing holding it. The
/// health strip narrows by dropping whole tiles, and its accessibility tree
/// announced the whole table regardless — so at the opening size the card
/// painted three tiles and announced five, and a reader was told about two
/// tiles nobody drew. The demo measured it (`3 tile(s) painted, 5 announced`);
/// the Rust suite stayed green, and a demo is not run by `cargo test`.
///
/// ⚠ Written for EVERY card rather than for the one that had the defect. A
/// ghost row is not a health-strip problem — it is what happens whenever a
/// body's row count depends on something the node builder does not consult,
/// and this screen now has two bodies whose row count depends on width.
///
/// The reverse direction is deliberately NOT asserted here: a painted row that
/// is not announced is the voice census's question, and it answers it with a
/// vocabulary this check does not have (`silent`, `unvoiced`, `ghost`).
///
/// ★★★★★ And its first run found a card this round did not touch. See
/// [`GHOSTS`].
#[test]
fn r1843_a_card_announces_only_the_rows_it_paints() {
    sweep(|state, shot, _, case| {
        for id in &shown_cards(state) {
            let id = id.as_str();
            let kind = super::kind_of(id);
            if GHOSTS.contains(&kind) {
                continue;
            }
            let Some(card) = state.card(id) else { continue };
            let Some((family, _)) = body_family(kind) else {
                continue;
            };
            let stem = format!("card.{id}.{family}.");
            let painted: std::collections::BTreeSet<String> = shot
                .family(&stem)
                .iter()
                .map(|t| t[stem.len()..].split('.').next().unwrap_or("").to_owned())
                .collect();
            for node in super::card_nodes(state, &card) {
                let Some(rest) = node.tag.strip_prefix(&stem) else {
                    continue;
                };
                if rest.contains('.') {
                    continue;
                }
                assert!(
                    painted.contains(rest),
                    "{case}: {id} announces row {rest} of its {family} and paints \
                     none — a reader is told about a row nobody drew",
                );
            }
        }
    });
}

/// ★★★★★ R1851 — **not one run the alarm feed authors sits in a box too short
/// for its own face**, at any size, in any state.
///
/// ZERO, not a budget. The screen-wide gate
/// (`r1800_no_run_sits_in_a_box_too_short_for_its_own_face`) is a ratchet over a
/// population measured before it existed, and a ratchet is the right shape for a
/// backlog and the wrong shape for a surface being written now: a new card can
/// add short runs and the ratchet only notices when the total crosses a line
/// somebody else's work also moves. This asks the question of one card, where the
/// answer can be zero.
///
/// It PRINTS what it finds, because "which run and by how much" is what a person
/// fixing one needs and the screen-wide gate shows only the first six.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1851_no_alarm_run -- --nocapture
/// ```
#[test]
fn r1851_no_alarm_run_sits_in_a_box_too_short_for_its_own_face() {
    let mut worst: Vec<String> = Vec::new();
    sweep(|state, _, scene, case| {
        let Some(id) = shown_cards(state)
            .into_iter()
            .find(|id| super::kind_of(id) == "alarms")
        else {
            return;
        };
        let stem = format!("card.{id}.");
        for short in pinion_core::containment::short_boxes(scene) {
            let inside = short.tag.as_deref().is_some_and(|t| t.starts_with(&stem))
                || short.path.iter().any(|t| t.starts_with(&stem));
            if inside {
                worst.push(format!(
                    "{case}: {:?} at {}px in a {}px box needs {} (short by {}) — {:?}",
                    short.content, short.px, short.rect.h, short.needs, short.short_by, short.tag,
                ));
            }
        }
    });
    for line in &worst {
        println!("{line}");
    }
    assert!(
        worst.is_empty(),
        "{} alarm run(s) are in a box too short for their own face",
        worst.len()
    );
}

// -- 5. Disjoint: nothing is painted on top of anything ----------------------

/// R1668 — no two rows of one card overlap.
#[test]
fn r1668_no_two_rows_of_one_card_are_painted_over_each_other() {
    sweep(|state, shot, _, case| {
        for id in &shown_cards(state) {
            // ★ R1851 added `feed.row.` — the alarm feed's rows are absolutely
            // positioned inside a virtualised sizer, which is precisely the
            // arithmetic that can put two of them on the same pixels.
            for stem in [
                "row.",
                "tree.",
                "map.",
                "chip.",
                "stat.",
                "bytes.",
                "feed.row.",
            ] {
                let family = shot.family(&format!("card.{id}.{stem}"));
                let rects: Vec<(&str, Rect)> = family
                    .iter()
                    .map(|t| (*t, shot.rect(t).expect("just enumerated")))
                    .collect();
                for (n, (a_tag, a)) in rects.iter().enumerate() {
                    for (b_tag, b) in &rects[n + 1..] {
                        // ★★★★★ R1843 — a row and its own DESCENDANT are not
                        // two rows, and a containment is not an overlap.
                        //
                        // This walks every tag under the stem and compares them
                        // pairwise, which was exact while every body row was one
                        // container with untagged text inside it — there were no
                        // descendants to meet. `pinion_widget_paint::stat_tile`
                        // tags each word's own box on purpose (that is what
                        // stops a word being filed under whatever encloses it),
                        // so `stat.0` now meets `stat.0.delta`, and a child
                        // sitting inside its parent read as two rows painted on
                        // top of each other.
                        //
                        // Ancestry is exactly what the tag says, so it is what
                        // the skip tests. The rule the gate is FOR — no two
                        // SIBLING rows overlapping — is untouched, and the same
                        // repair as `Painted::rows` one screen over: the model
                        // assumed flat bodies and a body stopped being flat.
                        if b_tag.starts_with(&format!("{a_tag}.")) {
                            continue;
                        }
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

/// ★★★★★ R1864 — paint `destination` in **each frame its section declares**,
/// in order.
///
/// A section is not always one frame. A page taller than the region it is given
/// shows part of itself at a time, and what the section *has* is what its
/// frames have together — which is why `ScreenRoster::poses_of` exists and why
/// R1864 taught it to answer for a page the host paints itself as well as for a
/// mounted screen. The population is the roster's; a list written here would be
/// this file's opinion about another module's fact.
///
/// ⚠ **The order is load-bearing.** A pose that scrolls asks the pane for its
/// range, and the range is derived by the layout pass — so pose 0 has to have
/// been painted before pose 1 can ask. Painting them in declaration order is
/// what makes that true rather than a coincidence.
///
/// Leaves the section in pose 0, the state a reader arrives in, so a later
/// check in the same scope is not silently asked about a pose this one chose.
fn poses_at_destination(destination: &str) -> Vec<Painted> {
    let state = use_shell_state();
    if destination != state.at() {
        state
            .go(destination)
            .unwrap_or_else(|why| panic!("{destination} is open and refused: {why:?}"));
    }
    let poses = state.screens.poses_of(destination);
    assert!(
        poses >= 1,
        "the roster says {destination} needs {poses} frame(s), and a section \
         with no frames is one nothing below can ask about",
    );
    let mut out = Vec::with_capacity(poses);
    for nth in 0..poses {
        state.screens.pose(destination, nth);
        out.push(painted_at((WIN_W, WIN_H)).0);
    }
    state.screens.pose(destination, 0);
    out
}

/// ★★★★★ R1867 — **the host's status slot has two occupants, and a census of
/// what a destination shows has to see both.**
///
/// Navigating says a sentence, so every frame these gates take is a frame with
/// a toast in the band — which is why `shell.toast` has always satisfied a
/// `Where::Chrome` row and why the gesture sentence beside it could not. The
/// slot is one place with two things in it, exactly as a section is one place
/// with several poses, and this is that second axis made explicit rather than
/// left to whichever state the test happened to be in.
///
/// Runs `f` with a toast up (the state navigation leaves behind) and again with
/// the toast's whole life spent. ⚠ Seconds, not milliseconds (R1783), and taken
/// from [`Saying::life`](pinion_core::utterance::Saying::life) rather than
/// written here — a test that pins that number pins a fact the type owns.
fn over_slot_occupancies<T>(
    state: &std::rc::Rc<super::ShellState>,
    mut f: impl FnMut() -> T,
) -> Vec<T> {
    let owner = Owner::current().expect("a pose is taken inside an Owner scope");
    let mut out = Vec::with_capacity(2);
    assert!(
        state.toast.showing().is_some(),
        "navigation says a sentence, so a frame taken right after it must have \
         one — if this fails the two occupancies below are one",
    );
    out.push(f());
    owner.tick_animations(state.toast.life() + 1.0);
    assert!(
        state.toast.showing().is_none(),
        "the toast outlived its own declared life",
    );
    out.push(f());
    out
}

/// The same frames, folded into one index: what the section shows across all of
/// them.
///
/// ⚠ Only for questions about what a section HAS. A question about where
/// something is — a press at a painted rectangle, say — is about one frame, and
/// folding two would compare a rectangle from one with a hit test run in the
/// other.
///
/// ★ R1867 — folded over the status slot's occupancies too, for the reason
/// [`over_slot_occupancies`] gives.
fn painted_over_poses(destination: &str) -> Painted {
    let state = use_shell_state();
    if destination != state.at() {
        state
            .go(destination)
            .unwrap_or_else(|why| panic!("{destination} is open and refused: {why:?}"));
    }
    let mut frames: Vec<Painted> =
        over_slot_occupancies(&state, || poses_at_destination(destination))
            .into_iter()
            .flatten()
            .collect();
    let mut folded = frames.remove(0);
    for frame in frames {
        for (tag, rect) in frame.tags {
            folded.tags.entry(tag).or_insert(rect);
        }
        folded.runs.extend(frame.runs);
        for (tag, row) in frame.inert {
            folded.inert.entry(tag).or_insert(row);
        }
    }
    folded
}

/// ★★★★★ R1846 — **the health strip draws exactly what the census declares.**
///
/// The gate that makes [`spec::HEALTH_TILES_SHOWN`] safe to be a written
/// number. That constant is what `Population::HealthTiles` expands to, and it
/// is the first family on this screen whose size is a function of the card's
/// WIDTH rather than of a table — so it could not be derived from a `const`,
/// and a pin with no gate is the defect this project keeps repairing.
///
/// ⚠ Asserted at the OPENING size only, on purpose: at a wider window the strip
/// draws more tiles and the census's rows are about the board a reader opens
/// with. That the voice table describes one window size and not all of them is
/// a real limit, and it is the sibling of the one
/// `debt-the-voice-gate-judges-only-the-opening-screen` already records.
#[test]
fn r1846_the_strip_draws_what_the_census_declares() {
    let owner = Owner::new();
    owner.run(|| {
        let shot = painted_at((WIN_W, WIN_H)).0;
        let id = spec::card_of("health").expect("the opening board places the health card");
        let drawn = shot.rows(&format!("card.{id}.stat."));
        assert_eq!(
            drawn,
            spec::HEALTH_TILES_SHOWN,
            "the strip draws {drawn} tile(s) and the census declares {}, so \
             `r1694`'s voice comparison is about to disagree with the paint",
            spec::HEALTH_TILES_SHOWN
        );
        assert!(
            drawn < spec::HEALTH_TILES.len(),
            "this pin only earns its keep while the strip narrows — with all \
             {} tiles drawn the family should expand from the table instead",
            spec::HEALTH_TILES.len()
        );
    });
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
///
/// ★★★★★ R1864 — over the section's **frames**, not over one of them. The
/// preferences page is taller than the region it is given, so its last group is
/// below the fold and a one-frame reading called it unpainted — of a page a
/// reader scrolls to in one gesture. The backward direction is unaffected by
/// the fold and gains from it: a region another destination owns must be absent
/// from *every* frame this one has, which is a stronger sentence than the one
/// this gate used to make.
#[test]
fn r1695_each_destination_paints_the_regions_the_specification_gives_it() {
    let owner = Owner::new();
    owner.run(|| {
        let roster = spec::destinations();
        for destination in roster.open() {
            let key = destination.key.as_ref();
            let shot = painted_over_poses(key);
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
            // ★ R1867 — over the status slot's two occupancies, for the reason
            // `over_slot_occupancies` gives: the tree carries whichever of the
            // slot's occupants is painted, so a reading taken in one state
            // reports the other's region as unannounced.
            let announced: BTreeSet<String> = over_slot_occupancies(&state, || {
                super::AnalyzerShellView::access_node(&ScreenState::default(), None)
                    .into_iter()
                    .map(|node| node.tag)
                    .collect::<BTreeSet<String>>()
            })
            .into_iter()
            .flatten()
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
        // ★★★★★ R1864 — **frame by frame, and folded only at the end.** A press
        // is about ONE frame: the rectangle a control was painted in and the
        // scroll the hit test runs at have to be the same frame's, or this
        // would be comparing a position from one with an answer from another.
        // What folds across frames is only which controls were REACHED, which
        // is the population question and not the geometry one.
        if state.at() != "settings" {
            state
                .go("settings")
                .expect("`settings` is an open destination");
        }
        let poses = state.screens.poses_of("settings");
        let mut reached: BTreeSet<String> = BTreeSet::new();
        for nth in 0..poses {
            state.screens.pose("settings", nth);
            let shot = painted_at((WIN_W, WIN_H)).0;
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
                    "in frame {nth} of {poses}, a press at the centre of {tag} \
                     ({px},{py}) answered {hit:?}",
                );
                reached.insert(tag.clone());
            }
        }
        state.screens.pose("settings", 0);
        assert_eq!(
            reached.len(),
            spec::OPTIONS.len() + spec::KEY_ROWS.len() + spec::THEMES.len(),
            "the sweep reached a different number of controls than the page \
             has, over {poses} frame(s): {reached:?}",
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
                // R1826 — this case is about two panels overlapping on the
                // canvas, which the window level has nothing to do with.
                on_top: false,
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
        let opening = state.toast.showing();

        let shot = painted();
        press_tag(&state, &shot, "float.packet#0");
        assert_eq!(
            state.toast.showing(),
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
            state.toast.showing(),
            opening,
            "a drag that moved the panel announced it"
        );
        assert!(
            state.toast.sentence().contains("moved"),
            "and said what happened: {:?}",
            state.toast.showing()
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
/// ★★★★★ R1819 — **every gesture this screen ADVERTISES does something**, and
/// this screen advertises some at last.
///
/// The last of the tool's three screens to get this gate, and the reason the
/// debt stayed open: a screen with no advertised list runs this check over the
/// EMPTY SET, which passes and is indistinguishable from a screen keeping every
/// promise. Screen A printed `wheel -> zoom` for its whole life with the wheel
/// dead, and the operation gate next door could not see it in principle because
/// the advertised list is a **different population**.
///
/// ⚠ The shape is NOT written here. It is
/// [`pinion_core::test_fixtures::advertised`], because this gate already
/// existed twice — on screens A and B — and the two copies had drifted, so
/// writing it a third time would have been three versions of one rule. What
/// this screen supplies is the driving and the witness, which is all that
/// genuinely differs.
///
/// The witness is the WHOLE published layout rather than a slot chosen per
/// gesture: an advertised effect is prose ("moves it", "resizes the panel"),
/// and picking the slot to watch would be this test deciding what the prose
/// meant. Pinning each effect to its own witness is the operation gate below.
#[test]
fn r1819_every_gesture_this_screen_advertises_does_something() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        pinion_core::test_fixtures::advertised::assert_every_advertised_gesture_acts(
            "the analysis shell",
            spec::GESTURES,
            || {
                // Everything a client can observe about the board, so a gesture
                // that moved anything at all counts as having acted.
                format!(
                    "{}|{}|{}",
                    witness(&state, "layout"),
                    witness(&state, "floats"),
                    witness(&state, "cards")
                )
            },
            |gesture| {
                let shot = painted();
                match gesture {
                    "drag a card by its grip" => {
                        drag_tag(&state, &shot, "card.packet#0.grip", (CELL_STEP, 0));
                    }
                    // The panel has to BE detached before it can be dragged,
                    // and it is detached the way a person detaches it rather
                    // than by assignment — the same rule `reach_precondition`
                    // follows for the operation table.
                    "drag a detached panel" => {
                        press_tag(&state, &shot, "card.packet#0.tear_off");
                        let shot = painted();
                        drag_tag(&state, &shot, "float.packet#0", (40, 25));
                    }
                    "drag a detached panel's corner" => {
                        drag_tag(&state, &shot, "float.packet#0.resize", (60, 40));
                    }
                    "drag a palette entry to the board" => {
                        press_tag(&state, &shot, "shell.palette.packet");
                    }
                    _ => return false,
                }
                true
            },
        );
    });
}

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
        // ★★★★★ R1864 — **over the page's frames.** The page scrolls and its
        // content is taller than the region it is given, so no single frame
        // holds all of it: the last group is below the fold at the top and the
        // page's own heading is above it at the end. A surface reconciles when
        // SOME frame of the page reproduces it, which is the claim the
        // specification makes — the parts are the page's, and a reader reaches
        // them all in one gesture.
        //
        // ⚠ Not a weakening of the per-surface rule: every surface still has to
        // reconcile ENTIRELY in one frame. What is folded is which frame that
        // is, not which parts were found.
        let state = use_shell_state();
        if state.at() != "settings" {
            state
                .go("settings")
                .expect("`settings` is an open destination");
        }
        let poses = state.screens.poses_of("settings");
        let doc = spec::settings_document();
        let mut judged = 0usize;
        let mut extent = None;
        for surface in doc.surfaces() {
            let mut worst: Option<Vec<String>> = None;
            let mut reconciled = false;
            for nth in 0..poses {
                state.screens.pose("settings", nth);
                let _ = painted_at((WIN_W, WIN_H));
                let regions = pinion_core::painted::painted_regions(super::VIEW_TAG)
                    .expect("the sweep just painted");
                extent = Some(regions.extent());
                let Built::Standing(parts) =
                    super::judge::settings_built(&regions, surface, Showing::OnScreen)
                else {
                    panic!(
                        "{surface}: away while this page is the one painted — the only away this \
                         section has is the reader being somewhere else"
                    );
                };
                // ★★★★★ R1770 — judged AT the extent this frame was painted
                // into, because one entry of this page's ledger is a fold and a
                // fold is a function of how tall the surface is. A gate that
                // passed no extent would be refused by that entry rather than
                // excused by it, which is the point: this page's verdict is a
                // claim about a size.
                let said: Vec<String> = doc
                    .unreconciled_at(surface, regions.extent(), &parts)
                    .iter()
                    .map(Unreconciled::sentence)
                    .collect();
                if said.is_empty() {
                    reconciled = true;
                    break;
                }
                // EVERY frame's sentences, not the first frame's. A page whose
                // frames fail differently is the case this loop exists for, and
                // a report naming one of them would send a reader to look at
                // the wrong one.
                worst
                    .get_or_insert_with(Vec::new)
                    .extend(said.into_iter().map(|s| format!("frame {nth}: {s}")));
            }
            judged += 1;
            assert!(
                reconciled,
                "`{surface}` is not what docs/analyzer-settings-spec.json declares \
                 in any of the page's {poses} frame(s):\n  {}",
                worst.unwrap_or_default().join("\n  "),
            );
        }
        state.screens.pose("settings", 0);
        let _ = painted_at((WIN_W, WIN_H));
        let regions =
            pinion_core::painted::painted_regions(super::VIEW_TAG).expect("the sweep just painted");
        assert_eq!(
            Some(regions.extent()),
            extent,
            "the frames this gate judged were painted at more than one extent, \
             so the size clause below is about only the last of them",
        );
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

/// ★★★★★ R1775 §5.32 — **this host does not paint on top of the screen it is
/// showing.**
///
/// # Reported by a person, and by nothing here
///
/// The assembled tool was run on a real desktop and the reader said the node
/// lab section was covering another rectangle. It was: this shell's status
/// toast, sitting on the mounted screen's own palette. Every gate in this file
/// was green, and could only be — containment asks whether a mark is inside the
/// box that owns it, and the toast is its own top-level box; the overlap gates
/// compare marks of ONE screen. The host/guest seam had no question at all,
/// which is [`pinion_screen::layering`]'s reason to exist.
///
/// # The population is derived, and both halves of it
///
/// `mounted_keys` gives the destinations with a screen behind them and
/// `tag_of` gives each one's paint root, so neither the list nor the tags are
/// written here. A hand-written pair would go stale the round a screen is
/// mounted — which is the round this check most needs to run.
#[test]
fn r1775_the_host_does_not_paint_on_the_screen_it_is_showing() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = super::screen_roster();
        let mounted: Vec<(String, &'static str)> = roster
            .mounted_keys()
            .map(str::to_owned)
            .filter_map(|key| roster.tag_of(&key).map(|tag| (key, tag)))
            .collect();
        assert!(
            mounted.len() >= 4,
            "the roster reports {} mounted screen(s); a population this small \
             cannot be the assembled tool's, and an empty one would pass every \
             clause below without testing anything",
            mounted.len(),
        );
        let mut over: BTreeSet<String> = BTreeSet::new();
        for (key, tag) in &mounted {
            state
                .go(key)
                .unwrap_or_else(|why| panic!("`{key}` is mounted and refused: {why:?}"));
            let scene = painted_at((WIN_W, WIN_H)).1;
            assert!(
                pinion_screen::layering::region_of(&scene, tag).is_some(),
                "`{key}` is mounted and painted no node tagged `{tag}`, so this \
                 check would be reporting on a screen that is not showing",
            );
            for found in pinion_screen::layering::host_marks_over_guest(&scene, tag) {
                over.insert(found.host);
            }
        }
        // ★★★★★ R1865 — **ZERO, and the shape changed with the number.**
        //
        // This was a RATCHET pinning the set at exactly `{shell.toast}`, and
        // the comment under it explained why the obvious rule was wrong: the
        // behaviour reference's own toast is `position: fixed; bottom: 22px;
        // left: 50%` and floats over whatever is beneath it, so *nothing
        // overlaps* looked like a demand the reference itself does not meet.
        //
        // That reasoning had a hole, and a reader found it twice: the reference
        // is an EXAMPLE and not a ceiling (the standing correction of
        // 2026-08-27), and what the reference does with a floating box is not
        // the only way to say a sentence. R1865 put the toast in the host's own
        // status band — outside every `page_rect`, one rectangle at every
        // destination — so the set is empty by construction rather than by
        // discipline, and the rule the comment called wrong is simply true now.
        //
        // ⚠ An equality pin on an EMPTY set is the wrong shape (R1858, R1860):
        // it reads as a backlog of size zero when what is meant is a property.
        // This is an emptiness assertion, and the sibling gate below is what
        // stops it being satisfied by a shell that paints no toast at all.
        assert!(
            over.is_empty(),
            "a host mark reaches a mounted screen's region. Everything this \
             shell says about itself belongs in `shell.status`, which is \
             outside every page rectangle — so this is new chrome drawn into a \
             region the host gave away: {over:#?}",
        );
    });
}

/// ★★★★★ R1781 — **the window this tool ships in is narrower than what its own
/// screens declare they need, and that conflict is two constants nobody
/// compared.**
///
/// # What this is NOT, because the round that wrote it nearly built the wrong
/// thing twice
///
/// A reader ran the assembled tool and found the inspector cut off, and a debt
/// was opened saying the shipping size was "outside the judged population".
/// **Both halves of that were wrong**, and re-measuring is what said so: R1767
/// measured this exact size, and `r1770_a_verdict_says_what_size_it_was_read_at`
/// section D DRIVES THE BINARY AT IT and asserts the whole story — that the tool
/// does not claim to conform there, that the lab's surfaces decline to be
/// judged rather than failing, that the reason names both the width the screen
/// declares and the width it was given, and that the second is smaller than the
/// first. Nothing about the state a reader met is unmeasured or ungated.
///
/// A first draft of THIS test tried to re-ask that in process and got
/// `reproduced = 26` against the demo's 118 — twice, once before walking the
/// destinations and once after. The cause is not a regression: this harness
/// records ONE surface (`record_painted_surfaces(.., &[VIEW_TAG])`) where the
/// running binary records every mounted screen's, so a walk-level verdict
/// cannot be assembled here at all. That is
/// `debt-the-in-process-sweep-cannot-mount-a-screens-extra-externals`, met from
/// the inside.
///
/// # What is genuinely missing, and is what this asserts
///
/// The demo says the tool declines at this size. Nothing says WHY it had to:
/// that `WIN_W` and a mounted screen's declared comfortable width were chosen
/// in different rounds and cannot both be satisfied. Those are two constants,
/// available here without any surface, and comparing them is a fact about the
/// application's own arrangement rather than about any frame.
///
/// It is a ratchet on the SET of screens that cannot fit, not a demand that the
/// set be empty: closing it means choosing between a window bigger than most
/// laptops, a toolbar overflow affordance that does not exist yet, and a screen
/// decision three rounds paid for. What may not happen is the set changing
/// while nobody notices.
#[test]
fn r1781_the_shipping_window_cannot_give_every_screen_what_it_declares() {
    let owner = Owner::new();
    owner.run(|| {
        let roster = super::screen_roster();
        // What a mounted screen actually receives: the window less the rail the
        // host keeps for itself. Derived rather than written down, so moving
        // either constant moves this.
        let granted = WIN_W.saturating_sub(spec::RAIL_W);

        // ★★★★★ R1784 — THE POPULATION IS EVERY SECTION A READER CAN OPEN, not
        // the mounted ones. This walked `mounted_keys` and asserted it had asked
        // at least four; the tool opens SIX, and the two the host paints itself
        // were not failing the check, they were never in it. R1738's finding one
        // property over — there a section that published no verdict was absent
        // rather than short, here a section that declared no size was.
        //
        // `unsized_keys` is the assertion that closes it, because it names what
        // the question did not reach instead of counting what answered.
        let unanswered: Vec<&str> = roster.unsized_keys().collect();
        assert!(
            unanswered.is_empty(),
            "★ {unanswered:?} cannot say what they lay out in, so this window \
             was never checked against them. A page this host paints itself \
             declares through `ScreenRoster::laying_out`; a mounted screen \
             declares through `Screen::shrink_policy` and no host can answer \
             for it.",
        );

        // ★★★★★ R1830 — the peer census, and it is asserted for the reason the
        // one above is: an open destination this host never granted a width has
        // not been checked, and a gate that skipped it would go green over a
        // question nobody put. This is what makes `granted_of` safe to unwrap
        // in the loop below.
        let ungranted: Vec<&str> = roster.ungranted_keys().collect();
        assert!(
            ungranted.is_empty(),
            "★ {ungranted:?} were never granted a width, so this window was \
             never checked against what they RECEIVE. A host declares that \
             through `ScreenRoster::granting`.",
        );

        // The open destinations, which is what "a section a reader can arrive
        // at" means and what `unsized_keys` above filtered on. A closed seat
        // lays nothing out, so a size there would be a number about a page
        // nobody reaches.
        let open: Vec<String> = roster
            .destinations()
            .keys()
            .filter(|key| {
                roster
                    .destinations()
                    .get(key)
                    .is_some_and(|d| d.standing.is_open())
            })
            .map(str::to_owned)
            .collect();

        // Every row, not only the short ones. ★★★★★ R1784's counterfactual is
        // why: breaking the per-destination grant — reading one
        // window-less-rail figure for all six — left this gate GREEN, because
        // both host-painted pages fit under either reading. A mechanism nothing
        // can be wrong about is a mechanism nobody is holding, so the rows are
        // kept and the dashboard's is asserted against `page_rect` below.
        let mut rows: Vec<(String, u32, u32)> = Vec::new();
        let mut short: Vec<(String, u32, u32)> = Vec::new();
        let mut asked = 0usize;
        for key in &open {
            let Some(policy) = roster.shrink_policy_of(key) else {
                continue;
            };
            asked += 1;
            // ★★★★★ AND WHAT A SECTION IS GRANTED IS PER-DESTINATION. The old
            // reading — the window less the rail — is right for a mounted
            // screen and wrong for a page the host paints itself, whose
            // section includes chrome the host draws BESIDE the page region
            // (the dashboard's palette is 292 of it).
            //
            // ★★★★★ R1830 — asked of the ROSTER, not of `super::page_rect`.
            // That function is this host's; another host has its own, and while
            // the grant lived only there the roster could not check that a
            // section's want and its grant were about the same section — the
            // only thing holding the pair together was this assertion. The host
            // now DECLARES the grant (`ScreenRoster::granting`, in
            // `screen_roster`), so the pair is checkable from the roster alone
            // and `r1830_a_section_that_wants_more_than_its_grant_is_named_by_
            // the_roster` holds it with no host in sight.
            let region = roster
                .granted_of(key, super::win_w())
                .expect("the ungranted check above is empty, so every open key has one");
            let wants = policy.comfortable().0;
            rows.push((key.clone(), wants, region));
            if wants > region {
                short.push((key.clone(), wants, region));
            }
        }

        // ★★★★★ THE ROW THAT PINS THE PER-DESTINATION GRANT. The dashboard's
        // region is the one that differs from every mounted screen's, so it is
        // the row a wrong reading changes.
        //
        // ★★★★★ R1830 CHANGED WHAT THIS ASSERTION IS FOR, and it is worth
        // saying because the line did not move. It used to check that the gate
        // read the host's own function. The gate now reads the ROSTER, so this
        // is the CROSS-CHECK that keeps the declaration honest: the inset the
        // host declared through `ScreenRoster::granting` must derive the same
        // width the host actually paints through `page_rect`. Without it a host
        // could declare a comfortable inset and paint a different one, and
        // moving the grant into the roster would have bought portability at the
        // cost of truthfulness.
        //
        // ⚠ Not vacuous, measured: replacing this round's per-destination inset
        // with one figure for every section — the reading R1784's CF-5 showed a
        // gate passing — fails HERE, `left: 1388, right: 1096`.
        let board = rows
            .iter()
            .find(|(key, ..)| key == "dashboard")
            .expect("the dashboard is an open destination and declares a size");
        assert_eq!(
            board.2,
            super::page_rect("dashboard").w,
            "the dashboard was compared against {} where its page region is \
             {} — a section the host paints itself is handed less than a \
             mounted screen, because the host's chrome is inside the section",
            board.2,
            super::page_rect("dashboard").w,
        );

        assert_eq!(
            asked,
            open.len(),
            "every open destination answered, or the loop skipped one the \
             emptiness check just said could not exist",
        );
        assert!(
            asked >= 6,
            "only {asked} section(s) — a population this small cannot be this \
             tool's, and the clauses below would be vacuous over it",
        );
        // The mounted screens are handed the window less the rail; a page the
        // host paints itself is handed less again, because the host's own
        // chrome sits inside the section. Kept as an assertion rather than a
        // comment so the two readings cannot silently converge.
        assert!(
            super::page_rect("dashboard").w < granted,
            "the dashboard's page region ({}) must be narrower than a mounted \
             screen's ({granted}) — its palette is inside the section",
            super::page_rect("dashboard").w,
        );

        // ★★★★★ NONE — and it was two, then one, and this is the round it
        // reaches zero.
        //
        // R1781 wrote this as a RATCHET on the set, with `["packets", "lab"]`,
        // and said what a change in either direction would mean: *"growing the
        // set means a screen was mounted that does not fit; shrinking it means
        // somebody chose one of the three repairs and this ratchet is what they
        // update"*. R1791 chose one of the three for the node lab. R1860 chose
        // one for the capture viewer — its two side panes had no floor of their
        // own, so the width the specification draws them at was standing in for
        // one, and deriving the real floors took its declared minimum from 1425
        // to 1352.
        //
        // ★★★★★ AND THE SHAPE CHANGES WITH THE NUMBER, which is R1858's rule:
        // **a ratchet is the shape of a backlog and the wrong shape for a
        // property that now holds.** An equality pin on `["packets"]` said "one
        // screen may be short"; there is nothing left to be permissive about, so
        // this is an emptiness assertion and a screen that stops fitting fails
        // here rather than being compared against a list somebody remembers to
        // update.
        //
        // ⚠ It is not a claim that no window can ever be too small — it is about
        // THE WINDOW THIS TOOL SHIPS IN, which R1791 also made the narrowest one
        // it will open at, so there is no smaller case to reason about.
        let names: Vec<&str> = short.iter().map(|(k, ..)| k.as_str()).collect();
        assert!(
            names.is_empty(),
            "★ {names:?} declare they lay out wider than the window this tool \
             ships in gives them, so what they paint past that edge is CUT. \
             Measured: {short:?} — each is (screen, the width it declares it \
             lays out at, the width {WIN_W} less the rail leaves it). A screen \
             appears here by declaring a minimum bigger than its grant; the \
             repair is the one R1791 and R1860 each made, which is to find the \
             term of that minimum that is a design width standing in for a \
             floor. See \
             `debt-the-shipped-window-is-below-a-mounted-screens-minimum`",
        );
    });
}

/// ★★★★★ R1784 — **the dashboard's floor is derived from the layout it opens
/// with**, and this is what makes the derivation's direction checkable.
///
/// Without it the `const fn` could read the WIDEST span instead of the
/// narrowest and no gate would move: the floor would come out smaller and the
/// shipping window would still satisfy it. A derivation nothing can be wrong
/// about is a number written down with extra steps.
#[test]
fn r1784_the_boards_floor_is_derived_from_the_layout_it_opens_with() {
    // The span the floor rests on, computed the other way round — over the
    // specification's rows at runtime rather than in a `const fn` — so the two
    // derivations have to agree.
    let narrowest = spec::BOARD
        .iter()
        .map(|placed| placed.cols)
        .min()
        .expect("the opening layout places something");
    assert_eq!(
        narrowest,
        super::narrowest_span(),
        "the floor is derived from the NARROWEST card the board opens with, \
         because that is the one a shrinking canvas takes below legibility \
         first",
    );
    assert!(
        narrowest < spec::GRID_COLS,
        "the premise: some card spans less than the whole board, or \
         'the narrowest' is not a discriminating fact about this layout",
    );

    // ★★ And the floor does what it was derived to do: at that width the
    // narrowest card is no smaller than the same card torn off, which is the
    // one number this derivation borrows from outside the board.
    let canvas = super::board_canvas_floor();
    let pitch = (canvas - super::GAP) / spec::GRID_COLS;
    let card = narrowest * pitch - super::GAP;
    assert!(
        card >= super::FLOAT_MIN_W,
        "at the derived canvas floor {canvas} a {narrowest}-column card is \
         {card} wide, under the {} a torn-off panel clamps to — so a card \
         would be legible detached and not in place, which is this shell \
         disagreeing with itself about one thing",
        super::FLOAT_MIN_W,
    );
}

/// ★★★★★ R1776 — **what the host paints over a guest LEAVES.**
///
/// The rule the sibling ratchet above cannot state. The obvious gate — a host
/// paints nothing inside the region it handed over — is wrong, and the
/// reference is what says so: its own toast is `position: fixed; bottom: 22px;
/// left: 50%` and floats over whatever is beneath it. What makes that
/// acceptable there is `setTimeout(.., 2600)`.
///
/// So the honest question is not *does it overlap* but *does it go*, and that
/// takes two frames and a clock between them. Both clauses are asserted: it has
/// to be there first, or a screen that painted no toast at all would pass this
/// while reproducing nothing.
///
/// The clock is the framework's — [`Owner::tick_animations`], which backends
/// call once per paint with the frame's `dt`. Driving it here is what makes
/// this a test of the shipped mechanism rather than of a number in a field.
///
/// ★★★★★ R1865 — **and the first clause got STRONGER by ceasing to be about
/// overlap at all.** It used to require the live toast to be over the guest,
/// because that was the only way to know a toast had been painted. The toast
/// lives in the host's status band now, which is outside every `page_rect`, so
/// the honest pair is *it is on the frame* and *it is not over the guest* —
/// asserted together, because either one alone is satisfied by a shell that
/// paints no toast.
#[test]
fn r1776_the_hosts_toast_leaves_the_screen_it_is_showing() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = super::screen_roster();
        let mounted: Vec<(String, &'static str)> = roster
            .mounted_keys()
            .map(str::to_owned)
            .filter_map(|key| roster.tag_of(&key).map(|tag| (key, tag)))
            .collect();
        assert!(!mounted.is_empty(), "no screen is mounted");
        let (key, tag) = &mounted[0];
        state
            .go(key)
            .unwrap_or_else(|why| panic!("`{key}` is mounted and refused: {why:?}"));
        // Say something, so the toast is alive for a reason a person can cause
        // rather than because the application happens to open having spoken.
        state.say(super::Utterance::done("a thing happened"));

        let before = painted_at((WIN_W, WIN_H)).1;
        assert!(
            pinion_runtime::rect_for_tag(&before, "shell.toast").is_some(),
            "the toast must be ON THE FRAME before the clock runs, or this \
             gate would pass on a shell that never paints one",
        );
        let over_before = pinion_screen::layering::host_marks_over_guest(&before, tag);
        assert!(
            over_before.is_empty(),
            "★ R1865 — a LIVE toast reaches the guest's region: it is supposed \
             to be in the host's status band, which is outside every page \
             rectangle, so this is the covering defect back rather than the \
             lifetime one: {over_before:#?}",
        );

        // Past its life, in the steps a paint loop actually takes rather than
        // in one jump: a lifetime that only expires when handed its whole
        // duration at once is not one a running application would reach.
        for _ in 0..200 {
            owner.tick_animations(1.0 / 60.0);
        }

        let after = painted_at((WIN_W, WIN_H)).1;
        let over_after = pinion_screen::layering::host_marks_over_guest(&after, tag);
        assert!(
            over_after.is_empty(),
            "the host still paints over the guest after its toast's life ran \
             out — {over_after:#?}",
        );
    });
}

/// ★★★★★ R1811 — **how much of each one-run box its run does not use.**
///
/// The complaint that opened `debt-the-host-paints-a-toast-onto-the-guests-content`
/// was not that anything was lost — it was that a status box reading a short
/// sentence was *strangely wide*. This tree's paint gates ask whether ink stays
/// inside its box and whether a box is tall enough for its face; neither asks
/// whether a box is far LARGER than the one thing it holds, which is why a
/// person had to.
#[test]
fn r1811_a_one_run_box_is_not_far_larger_than_its_run() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        state.say(super::Utterance::done("a thing happened"));
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_text::LayoutCache::new();
        pinion_runtime::layout::compute_layout(&mut scene, &mut cache, WIN_W, WIN_H);

        // The SAME cache the layout just shaped with — a stand-in measure would
        // be answering about a font nobody painted.
        let found = pinion_core::containment::slack(&scene, &mut |text| {
            let max_width = (text.rect.w > 0).then_some(text.rect.w);
            cache.ink_size(&text.content, &text.style, &text.runs, max_width)
        });
        assert!(
            !found.is_empty(),
            "no box on this screen holds anything, so this gate would pass by \
             not looking"
        );
        // ★ THE CALLER CHOOSES, because intent is not in the geometry — see
        // `slack`'s own documentation for the three derivations that were tried
        // and measured wrong. The toast is chosen because its width is a
        // constant that says nothing about its sentence, and a reader looking at
        // the running window said so.
        let toast = found
            .iter()
            .find(|s| s.tag.as_deref() == Some("shell.toast"))
            .expect("the toast was said, so it is on this frame");
        assert!(
            toast.spare_w <= TOAST_SLACK,
            "the toast box is {}px wider than what it holds ({:?} in a box {}px \
             wide) — a status box's width is a claim about its sentence",
            toast.spare_w,
            toast.content,
            toast.box_rect.w,
        );
    });
}

/// ★★★★★ R1811 — **the toast's width tracks its sentence**, which is what
/// "the box is a claim about its content" means and what a bound alone cannot
/// show.
///
/// A constant width satisfies any slack ratchet you set loosely enough. This
/// asserts the RELATION: a longer sentence gets a wider box, and a short one
/// does not get the long one's width. It is immune to the defect it replaced
/// by construction — 560 for everything fails here and passes any single-frame
/// bound.
#[test]
fn r1811_a_longer_toast_gets_a_wider_box() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let width_of = |sentence: &str| {
            state.say(super::Utterance::done(sentence));
            let scene = painted_at((WIN_W, WIN_H)).1;
            pinion_runtime::rect_for_tag(&scene, "shell.toast")
                .expect("a toast was just said")
                .w
        };
        let short = width_of("saved");
        let long =
            width_of("the capture was reassembled and every declared field resolved cleanly");
        assert!(
            long > short,
            "a longer sentence must get a wider box — short {short}px, long {long}px",
        );
        assert!(
            long <= super::TOAST_W,
            "and a very long one is bounded rather than crossing the window: \
             {long}px against a {}px maximum",
            super::TOAST_W,
        );
    });
}

/// ★ R1811 — the width the toast may hold beyond its content, as a RATCHET.
///
/// Not zero, and the residue is a DECIDED one rather than an unmeasured one:
/// a short sentence hits the toast's deliberate minimum width, so the room left
/// over is the floor's, not a constant nobody had checked. Measured on the
/// assembled screen it was 61 for the shortest sentence a gate says.
///
/// ⚠ The pre-R1811 figure is ARITHMETIC, not a measurement, and is written that
/// way deliberately: the old box was 560 wide whatever it said, and the same
/// sentence's ink spans roughly 137 of it from the bullet to the last glyph, so
/// it held on the order of four hundred pixels it never used. The gate could
/// not have measured that box — it was not the shape the first derivation
/// looked for, which is the finding `slack`'s own documentation records.
///
/// ★★★★★ R1865 — **64 -> 33, and BOTH halves of that move are findings.**
///
/// *Down*, because R1811's own note names the reason the residue was not zero:
/// the room left over was the deliberate MINIMUM WIDTH's, and that minimum
/// existed so a floating pill's bullet and rounded corners read as a strip. In
/// the band there is no pill — no fill, no border, no corner — so the minimum
/// went with it. ⇒ **a residue explained by a constant disappears when the
/// constant's reason does.**
///
/// *Up*, from the 16 that alone would have left, because removing the floor
/// exposed something the floor had been hiding for 54 rounds: the per-glyph
/// estimate was NARROW, and the whole sentence was being elided in the running
/// window. Widening it to two thirds of the face (`glyph_run`) fixed that and
/// put the room back — and the room is now a **structural** residue rather than
/// a floor's, because of what the repair measured:
///
/// ⚠⚠ ★★★★★ **The two ends of this estimate are checked against two different
/// fonts.** The lower end — *is the sentence elided* — is a fact about the
/// RENDERER's font, and this crate's tests shape with `pinion_text::
/// LayoutCache::new()`, which is not it: measured at R1865, the same 16-glyph
/// sentence inks to about 79px here and about 96px in the running window,
/// roughly 4.9 against 6.0 pixels a glyph. So an estimate wide enough to stop
/// the renderer eliding is necessarily wider than what this gate measures, and
/// the difference between the two fonts IS this number. It is a ratchet on that
/// difference, not on carelessness.
const TOAST_SLACK: u32 = 33;

// --- R1838: the arrangement, across a gesture rather than inside a frame -----
//
// A person reported, from a running desktop, that maximising and restoring and
// detaching made the pins of the node graph "spread enormously" and the
// arrangement fall apart. Every gate above this line paints ONE frame and
// asserts about it; none of them could see a claim about what a GESTURE does,
// which is what `debt-pins-spread-after-maximise-and-detach` recorded as the
// missing axis. These two are that axis.

/// Every pin the mounted node lab painted, by tag.
///
/// The graph's pins are the sharpest marks on that screen: fourteen of them,
/// two per card, so their positions are a fingerprint of the whole diagram's
/// arrangement — if the layout spreads, these move.
fn lab_pins(scene: &Scene) -> BTreeMap<String, Rect> {
    let mut out = BTreeMap::new();
    scene.for_each_node(&mut |visit| {
        if let (Some(tag), Some(rect)) = (visit.node.tag(), visit.absolute_rect())
            && tag.starts_with("lab.pin.")
        {
            out.entry(tag.to_owned()).or_insert(rect);
        }
    });
    out
}

/// ★★★★★ R1838 — **one window's arrangement is not decided by another
/// window's size**, which is what a maximise plus a detach can otherwise do.
///
/// Driven on the REAL per-window paint path ([`pinion_shell::ShellCore`]) and
/// not on this module's single-window sweep, because that is the only place the
/// question exists: a window painted while another window is a different size.
///
/// # What it caught
///
/// `VIEWPORT_SIZE` holds the PRIMARY window's extent by R1006's deliberate
/// decision, and `pinion_core::external::layout_size` read it inside every
/// view — so **every** window laid itself out at the primary's size. Measured
/// here before the repair, painting this application into a 520x380 window:
///
/// | primary | `shell.appbar.search` | marks outside the 520x380 window |
/// |---|---|---|
/// | 1440x900 | x = 1140 | 294 |
/// | 1920x1080 | x = 1620 | 304 |
/// | 2494x1531 | x = 2194 | 304 |
///
/// The chip's x is the primary's width less 300, in a window 520 wide: a
/// person maximising the main window pushed the content of every other window
/// further out of it. The repair is `with_window_extent` — the shell states
/// the extent of the window it is painting, and `layout_size` prefers that
/// statement over the viewport read.
///
/// # Why the assertion is EQUALITY across primaries and not a containment
///
/// Marks still fall outside a 520x380 window afterwards, and that is this
/// application's own declared policy rather than the defect:
/// [`SHRINK`](super::SHRINK) is `rigid`, so below its comfortable size this
/// shell clips instead of shrinking. A containment bound would therefore have
/// to be a number, and a number would be asserting the rigidity. What the
/// defect actually was is a DEPENDENCE, so a dependence is what is refused.
#[test]
fn r1838_a_windows_arrangement_is_not_decided_by_another_windows_size() {
    let mut sc = pinion_shell::ShellCore::<super::AnalyzerShellView>::new();
    let mut seen: Option<BTreeMap<String, Rect>> = None;
    let mut ran = 0;
    for primary in [(WIN_W, WIN_H), (1920, 1080), (2494, 1531)] {
        let _ = sc.compute_paint_scene(primary.0, primary.1);
        let scene = sc.compute_paint_scene_for_window("second", 520, 380);
        let arrangement: BTreeMap<String, Rect> = scene
            .absolute_rects_by_tag()
            .into_iter()
            .filter(|(tag, _)| tag.starts_with("shell."))
            .collect();
        assert!(
            !arrangement.is_empty(),
            "the second window painted this application, or nothing was compared"
        );
        match &seen {
            None => seen = Some(arrangement),
            Some(first) => assert_eq!(
                &arrangement, first,
                "\u{2605} a 520x380 window must paint the SAME arrangement whatever \
                 size the primary window is \u{2014} before the repair every mark \
                 moved with the primary's width (primary {}x{})",
                primary.0, primary.1,
            ),
        }
        ran += 1;
    }
    assert_eq!(ran, 3, "three primary sizes were compared");
}

/// ★★★★★ R1838 — **the mounted node lab's diagram survives the board's own
/// gestures and the window's**, which is the claim the person's report was
/// about and the one this repository had no instrument for.
///
/// The gestures are the reported ones, in the reported order: the window
/// maximised and restored, then a card maximised, restored, detached into a
/// window of its own and re-docked. The pins are read from the painted scene
/// each time.
///
/// ★ The fidelity assertion is what makes the equalities mean anything. A
/// mounted screen that never saw the resize at all would hold every pin still
/// and pass, so the sweep first proves the lab DOES reflow: measured here, of
/// its 171 painted marks at the opening size, 88 move when the window is
/// maximised (`lab.canvas` 846x802 -> 1900x1433). The pins do not, and that is
/// the correct answer rather than an absent one — the graph is a diagram
/// anchored at the canvas origin, so a larger window reveals more of it rather
/// than spreading it.
#[test]
fn r1838_the_mounted_labs_diagram_survives_maximise_and_detach() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        state.go("lab").expect("`lab` is an open destination");

        let (opening, at_opening) = {
            let (shot, scene) = painted_at((WIN_W, WIN_H));
            (shot.tags, lab_pins(&scene))
        };
        assert_eq!(at_opening.len(), 14, "the graph paints fourteen pins");

        // Fidelity first: the mounted screen reflows, so an equality below is
        // a measurement and not a screen that cannot see a resize.
        let maximised = painted_at((2494, 1531));
        let lab_marks = |tags: &BTreeMap<String, Rect>| -> BTreeMap<String, Rect> {
            tags.iter()
                .filter(|(tag, _)| tag.starts_with("lab."))
                .map(|(tag, rect)| (tag.clone(), *rect))
                .collect()
        };
        let before = lab_marks(&opening);
        let after = lab_marks(&maximised.0.tags);
        let moved = before
            .iter()
            .filter(|(tag, rect)| after.get(*tag).is_some_and(|now| now != *rect))
            .count();
        assert!(
            moved > before.len() / 4,
            "\u{2605} the mounted lab must REFLOW with the window, or the pin \
             equalities below prove nothing \u{2014} {moved} of {} marks moved",
            before.len(),
        );

        // The window maximises and comes back.
        assert_eq!(
            lab_pins(&painted_at((WIN_W, WIN_H)).1),
            at_opening,
            "\u{2605} the graph is where it was after a maximise and a restore",
        );

        // The board's own gestures, on the page beside it.
        state.go("dashboard").expect("`dashboard` is open");
        let id = state.placed()[0].id().as_str().to_owned();
        ShellOracle::maximize(&state, &id).expect("a placed card maximises");
        let _ = painted_at((WIN_W, WIN_H));
        ShellOracle::restore(&state).expect("and restores");
        ShellOracle::detach(&state, &id).expect("and detaches");
        let _ = painted_at((WIN_W, WIN_H));
        ShellOracle::redock(&state, &id).expect("and re-docks");
        state.go("lab").expect("`lab` is open");
        assert_eq!(
            lab_pins(&painted_at((WIN_W, WIN_H)).1),
            at_opening,
            "\u{2605}\u{2605} and after a card was maximised, restored, torn off \
             and re-docked on the page beside it",
        );
    });
}

/// ★★★★★ R1839 — **`ColorRole::Outline` draws component boundaries on this
/// application, and the count is what says so.**
///
/// `pinion_core::legibility::PAIRINGS` holds `outline/surface` to WCAG
/// 1.4.11's 3:1 boundary floor, and until R1839 that declaration rested on an
/// assumption running the other way: that the role does TWO jobs — a boundary
/// (3:1) and a decorative divider (no floor) — so no single floor could be
/// right. `debt-one-outline-role-does-two-jobs-and-clears-neither` said so,
/// and said the number had to be measured before the vocabulary was split.
///
/// This is that measurement, and it refutes the premise: over the six painted
/// screens of this application the role draws **97 boundaries and 2 dividers**.
/// One job, and the declared floor is right for it.
///
/// # Why the census reads the FRAME and not the source
///
/// `grep ColorRole::Outline` finds 145 mentions across this tree, most of them
/// a `theme.resolve(...)` binding a local that is then used several times and
/// sometimes differently. A mention is not a use, and WCAG is about the mark
/// on the frame — so the classifier walks the painted scene and asks which
/// slot the colour is in: the `border` of a box is the edge of something, and
/// anything else is not.
///
/// # ⚠ What this application's own palette measures, which is NOT repaired here
///
/// The framework's canonical palettes clear the floor since R1839. This
/// application replaces them with the reference tool's own tokens, and those
/// are further short than the framework's ever were — asserted below so the
/// number is a gated fact rather than an impression. Raising them would be a
/// deliberate divergence from a behaviour reference this project is under
/// standing instruction to reproduce, which is a decision rather than a fix:
/// `debt-the-reference-palette-fails-the-boundary-floor` carries it.
#[test]
fn r1839_the_outline_role_draws_boundaries_on_every_screen() {
    let owner = Owner::new();
    owner.run(|| {
        use pinion_core::contrast::contrast_ratio;
        use pinion_core::legibility::{Floor, StrokeCensus, stroke_census};
        use pinion_core::theme::ColorRole;

        let state = use_shell_state();
        let (light, dark) = super::reference_palettes();
        let outline = dark.resolve(ColorRole::Outline);

        let mut all = StrokeCensus::default();
        let mut screens = 0;
        let roster = spec::destinations();
        let keys: Vec<String> = roster
            .all()
            .iter()
            .map(|d| d.key.as_ref().to_owned())
            .collect();
        for destination in keys {
            if state.go(&destination).is_err() {
                // A closed seat is not a screen; the roster's own gate covers
                // that it refuses, and this one is about what is painted.
                continue;
            }
            let (_, scene) = painted_at((WIN_W, WIN_H));
            all.absorb(&stroke_census(&scene, outline));
            screens += 1;
        }
        assert!(
            screens >= 6,
            "every open destination was censused, not a sample: {screens}"
        );

        // ★ The finding. A ratio rather than two pinned numbers: what the
        // declaration needs is that the role is a boundary role, and a screen
        // gaining a divider must not fail a gate that is about the vocabulary.
        assert!(
            all.boundaries() > 0 && all.dividers() * 10 < all.boundaries(),
            "\u{2605} `outline` must be overwhelmingly a component boundary for \
             `PAIRINGS` to hold it to one floor \u{2014} measured {} boundary \
             marks and {} divider marks",
            all.boundaries(),
            all.dividers(),
        );

        // ★★ And this application's own palette, measured rather than assumed.
        // Both short of the boundary floor the framework now clears — see the
        // header for why that is carried and not repaired here.
        for (name, theme) in [("light", light), ("dark", dark)] {
            let ratio = contrast_ratio(
                theme.resolve(ColorRole::Outline),
                theme.resolve(ColorRole::Surface),
            );
            assert!(
                ratio < Floor::Boundary.ratio(),
                "{name}: the reference palette's outline now CLEARS {} at \
                 {ratio:.2} \u{2014} if it was raised deliberately, this gate \
                 and the debt it points at are what say so",
                Floor::Boundary.ratio(),
            );
        }
    });
}

/// ★★★★★ R1860 — **in the assembled tool, at the size a person runs, nothing
/// the capture viewer paints falls outside the window.**
///
/// Rule (7)'s form for a defect somebody SAW. The report was about the shipped
/// window — `target/release/hello-analyzer-shell`, 1440x900 — and named the
/// element: *"the right outline of the rectangle with `background best effort`
/// in it is cut"*. That rectangle is the third reassembly lane, and it was
/// painted at `x=998 w=457`, so its outline landed at **1455** on a window
/// **1440** wide.
///
/// # Why the assembly is where this has to be asked
///
/// `hello-packet-view`'s own sweep runs the screen at three sizes it chooses,
/// and every one of them is a size the screen fits in — a screen asked how it
/// lays out in a window IT declared cannot report being given less. Only the
/// host knows what it grants, so only here can the two be compared against the
/// paint. The sibling check above
/// (`r1781_the_shipping_window_cannot_give_every_screen_what_it_declares`)
/// compares the two DECLARATIONS and needs no surface; this one asks the frame,
/// and the pair is deliberate: a screen could declare a width it fits in and
/// still paint past it.
///
/// ⚠ **By the window's edge, not by the seat's.** What a reader loses is what
/// the *window* cuts. A mark may legitimately sit outside the seat — the host's
/// own rail does — so the seat is the wrong boundary for this question even
/// though it is the number the grant is about.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1860_the_walk -- --nocapture
/// ```
#[test]
fn r1860_the_walk_reaches_a_capture_viewer_inside_the_shipping_window() {
    const SAID: &str = "background";

    let owner = Owner::new();
    owner.run(|| {
        let painted = painted_at_destination("packets");

        // Every mark the mounted screen paints, by its own address family.
        let marks = painted.family("pv.");
        assert!(
            marks.len() > 40,
            "only {} mark(s) under `pv.` — the walk did not reach the capture \
             viewer, and every clause below would be vacuous over that",
            marks.len(),
        );

        let escaping: Vec<(&str, u32, u32)> = marks
            .iter()
            .filter_map(|tag| painted.rect(tag).map(|r| (*tag, r.x, r.x + r.w)))
            .filter(|(_, _, right)| *right > WIN_W)
            .collect();
        assert!(
            escaping.is_empty(),
            "★ the capture viewer paints past the right edge of the window this \
             tool ships in ({WIN_W}), so what is there is CUT and a reader \
             reaches it by resizing and by nothing else. Measured (mark, x, \
             right): {escaping:?}",
        );

        // ★ AND THE RUN THE READER NAMED, found by the word they used rather
        // than by any address or constant of the mounted screen — which is
        // what they had. R1852: a host reading a guest's internals can pass
        // while the guest is broken.
        let (said, run, _) = painted
            .runs
            .iter()
            .find(|(content, ..)| content.contains(SAID))
            .unwrap_or_else(|| panic!("no run on this screen says {SAID:?}"));

        // ⚠ The box is found by CONTAINMENT, not by the run's nearest tagged
        // ancestor: a lane's words are painted beside its outline rather than
        // inside it, so the ancestor is the whole strip and asking about that
        // would be asking about a mark that reaches the window edge legitimately.
        let (tag, seat) = painted
            .family("pv.reassembly.lane.")
            .into_iter()
            .filter_map(|tag| painted.rect(tag).map(|r| (tag, r)))
            .find(|(_, r)| run.x >= r.x && run.x < r.x + r.w)
            .unwrap_or_else(|| panic!("no reassembly lane holds the run saying {SAID:?}"));
        println!(
            "the lane a reader named: {said:?} in {tag} x={} right={} (window {WIN_W})",
            seat.x,
            seat.x + seat.w,
        );
        assert!(
            seat.x + seat.w <= WIN_W,
            "★ {tag}, the box holding {SAID:?}, ends at {} on a window {WIN_W} \
             wide — this is the outline a reader reported cut",
            seat.x + seat.w,
        );
    });
}

/// ★★★★★ R1861 §5.32 — **this host's floating overlay covers none of the
/// letters of the screen it is showing.**
///
/// # What this is, and what it is NOT
///
/// It is not `r1775_the_host_does_not_paint_on_the_screen_it_is_showing`
/// tightened. That one is a RATCHET on which host marks reach the guest's
/// region at all, and it has to stay one: the behaviour reference's own toast
/// floats over the content (`position: fixed; bottom: 22px`) and is tolerable
/// because it leaves after 2.6 seconds, so a gate forbidding the overlap would
/// forbid what the reference does.
///
/// **Covering a SENTENCE is a claim the reference never makes** — what its toast
/// floats over is empty canvas — so this one can be zero, and a screen appearing
/// here is a defect rather than a budget line. That is R1859's rule applied
/// before a reader has to find it: a ratchet is the shape of a backlog, and this
/// property is not a backlog.
///
/// # The population, and why it is not vacuous
///
/// Every mounted destination, derived from `mounted_keys` + `tag_of` so a screen
/// mounted in a later round is asked without anyone remembering to add it. Each
/// one is driven to, made to say something so the overlay is on the frame, and
/// asserted to have painted sentences of its own — because a guest with no words
/// satisfies the clause below by having nothing to cover.
///
/// Measured before the repair, at the shipping size: the node lab lost the top 6
/// pixels of its gesture hint — which is what a reader reported — and the
/// capture viewer lost two lane readouts ENTIRELY, which nobody had ever seen.
#[test]
fn r1861_the_hosts_overlay_covers_nobodys_letters() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = super::screen_roster();
        let mounted: Vec<(String, &'static str)> = roster
            .mounted_keys()
            .map(str::to_owned)
            .filter_map(|key| roster.tag_of(&key).map(|tag| (key, tag)))
            .collect();
        assert!(
            mounted.len() >= 4,
            "the roster reports {} mounted screen(s); a population this small \
             cannot be this tool's, and the clauses below would be vacuous",
            mounted.len(),
        );
        for (key, tag) in &mounted {
            state.go(key).unwrap_or_else(|why| panic!("{key}: {why:?}"));
            // Say something, so the overlay is on the frame for a reason a
            // person can cause rather than because the tool happens to open
            // having spoken.
            state.say(super::Utterance::done("a thing happened"));
            let (painted, scene) = painted_at((WIN_W, WIN_H));
            let toast = painted.rect("shell.toast").unwrap_or_else(|| {
                panic!(
                    "{key}: the host paints no toast, so nothing here is being \
                     tested — a check that passed would be reporting on an \
                     overlay that is not there"
                )
            });
            // ★★★★★ THE POPULATION IS EVERY SENTENCE ON THE FRAME, and the
            // first draft's was half of it. Asked only of the GUEST's runs
            // (`layering::host_marks_over_guest_text`), this went green while
            // the pixel demo one directory over found the overlay sitting on
            // **this host's own** help strip — and the reader's report had
            // named a run from each strip in one sentence. A host's overlay
            // covering the host's own words is the same defect to a reader.
            let mut covered: Vec<(String, Rect)> = Vec::new();
            let mut sentences = 0usize;
            scene.for_each_node(&mut |visit| {
                let (Scene::Text(text), Some(rect)) = (visit.node, visit.absolute_rect()) else {
                    return;
                };
                // The overlay's own sentence is inside it, not under it.
                if visit
                    .ancestors
                    .iter()
                    .any(|a| a.tag() == Some("shell.toast"))
                {
                    return;
                }
                sentences += 1;
                if !(toast.x >= rect.x + rect.w
                    || rect.x >= toast.x + toast.w
                    || toast.y >= rect.y + rect.h
                    || rect.y >= toast.y + toast.h)
                {
                    covered.push((text.content.clone(), rect));
                }
            });
            let guest_letters = scene_text_of(&scene, tag);
            assert!(
                guest_letters >= 8 && sentences > guest_letters,
                "{key}: {guest_letters} guest sentence(s) of {sentences} on the \
                 frame — a population this shape cannot be this tool's, and the \
                 clause below would pass by having nothing to cover. The strict \
                 inequality is the half the first draft was missing: the host \
                 says things too",
            );
            println!(
                "{key}: toast {toast:?} over {sentences} sentence(s), \
                 {guest_letters} of them the guest's"
            );
            assert!(
                covered.is_empty(),
                "★ {key}: the host's overlay is painted on top of {} \
                 sentence(s), so a reader cannot read them. This is the defect \
                 a person reported by looking at the window. Measured: \
                 {covered:#?}",
                covered.len(),
            );
        }
    });
}

/// ★★★★★ R1861/R1865 — **what a screen DECLARES it occupies is where that
/// screen actually has words.**
///
/// # Why this exists, and it is a counterfactual's finding
///
/// The gate above asks whether anything is covered, and it stayed green with
/// the node lab's declaration deleted — because this host avoids its OWN help
/// strip too, and the place that clears one happens to clear the other. So the
/// declaration was load-bearing on the capture viewer and redundant on the node
/// lab, and nothing could tell those two apart from a defect. The repair was to
/// assert the CONTRACT rather than the outcome.
///
/// # ★★★★★ R1865 — and the contract it asserts had to change, because the
/// consumer went away
///
/// R1861's contract was *if the host's floating placement would land on your
/// words, say so*, and it was checked by intersecting each guest's runs with
/// the toast's seat. R1865 moved the toast into the host's status band, so
/// there is no floating placement to intersect with — the shell is not a
/// consumer of `Screen::keeps_clear` any more.
///
/// ⚠ **A declaration nobody reads is a declaration nobody checks**, and this
/// tree has met that twice (R1856's optional declaration, R1861's own CF-5,
/// where a new framework predicate had no consumer at all). Deleting the axis
/// was refused for the reason its debt gives: the vocabulary is right and the
/// next floating overlay — a popover over a mounted screen, a drag preview —
/// needs it. So the contract asserted here is the one that stays true with no
/// overlay in sight, and it is a STRONGER one: **what a screen declares it
/// occupies is DERIVED from what it paints there.** A declaration is a claim
/// about the frame, so it is checked against the frame: the band a screen
/// names must be a band that screen has words in.
#[test]
fn r1861_a_screen_with_words_under_the_overlay_says_so() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = super::screen_roster();
        let mounted: Vec<(String, &'static str)> = roster
            .mounted_keys()
            .map(str::to_owned)
            .filter_map(|key| roster.tag_of(&key).map(|tag| (key, tag)))
            .collect();
        assert!(mounted.len() >= 4, "the population cannot be this tool's");
        let mut owing = 0usize;
        for (key, tag) in &mounted {
            state.go(key).unwrap_or_else(|why| panic!("{key}: {why:?}"));
            let (_, scene) = painted_at((WIN_W, WIN_H));
            let bands = roster.keeps_clear_of(key, super::page_rect(key));
            let Some(seat) = bands else {
                println!("{key}: declares no band");
                continue;
            };
            let mut under: Vec<String> = Vec::new();
            scene.for_each_node(&mut |visit| {
                let (Scene::Text(text), Some(rect)) = (visit.node, visit.absolute_rect()) else {
                    return;
                };
                if !visit
                    .ancestors
                    .iter()
                    .any(|a| a.tag().is_some_and(|it| it == *tag))
                {
                    return;
                }
                if !(seat.x >= rect.x + rect.w
                    || rect.x >= seat.x + seat.w
                    || seat.y >= rect.y + rect.h
                    || rect.y >= seat.y + seat.h)
                {
                    under.push(text.content.clone());
                }
            });
            println!(
                "{key}: declares {seat:?} and has {} sentence(s) in it",
                under.len()
            );
            owing += 1;
            assert!(
                !under.is_empty(),
                "★ {key} declares it occupies {seat:?} through \
                 `Screen::keeps_clear` and paints NO sentence there. A \
                 declaration is a claim about the frame: either the screen's \
                 painter moved and the declaration did not follow it — the \
                 derivation broke — or it is reserving space nothing needs, \
                 which is a host overlay pushed aside for no reader's benefit",
            );
        }
        assert!(
            owing >= 2,
            "only {owing} screen(s) declare a band — a population this small \
             makes the clause above nearly vacuous, and two is what was \
             measured",
        );
    });
}

/// How many sentences the guest at `tag` painted on this frame.
///
/// Counted from the scene rather than asked of the screen: what a host's overlay
/// can cover is what was PAINTED, and a screen that answered its own count
/// would be answering about a different population.
fn scene_text_of(scene: &Scene, guest_tag: &str) -> usize {
    let mut n = 0usize;
    scene.for_each_node(&mut |visit| {
        if matches!(visit.node, Scene::Text(_))
            && visit.absolute_rect().is_some()
            && visit
                .ancestors
                .iter()
                .any(|a| a.tag().is_some_and(|t| t == guest_tag))
        {
            n += 1;
        }
    });
    n
}

/// ★★★★★ R1861 — **in the assembled tool, at the size a person runs, the
/// sentence a reader named is whole.**
///
/// Rule (7)'s form for a defect somebody SAW. The report was about the shipped
/// window — `target/release/hello-analyzer-shell`, 1440x900 — pressing the rail
/// to reach the node lab, and it named the words: *"the toast floats over `drag
/// a pin = author a link` and I cannot read it"*. So the run is found **by those
/// words**, which is what the reader had, and the question asked of it is
/// whether the host's overlay is on top of it.
///
/// ⚠ **The host reaches into nothing of the guest's.** It does not import the
/// lab's geometry, its constants or its tags — R1852 established that a host
/// reading a guest's internals can pass while the guest is broken.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1861_the_walk -- --nocapture
/// ```
#[test]
fn r1861_the_walk_reaches_a_sentence_the_overlay_leaves_alone() {
    const SAID: &str = "drag a pin = author a link";

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        state.go("lab").expect("the node lab section is open");
        state.say(super::Utterance::done("a thing happened"));
        let (painted, _) = painted_at((WIN_W, WIN_H));

        let toast = painted
            .rect("shell.toast")
            .expect("the host is showing its overlay");
        let (content, seat, _) = painted
            .runs
            .iter()
            .find(|(content, ..)| content.contains(SAID))
            .unwrap_or_else(|| {
                panic!("no run in the assembled tool says {SAID:?} — the walk no longer reaches it")
            });
        println!("the sentence a reader named: {content:?} at {seat:?}, overlay at {toast:?}");
        let meets = !(toast.x >= seat.x + seat.w
            || seat.x >= toast.x + toast.w
            || toast.y >= seat.y + seat.h
            || seat.y >= toast.y + toast.h);
        assert!(
            !meets,
            "★ the host's overlay ({toast:?}) is painted over the sentence a \
             reader named ({seat:?}) — which is the report, reproduced",
        );
    });
}

/// ★★★★★ R1862 — **in the assembled tool, at the size a person runs, the
/// sentence a reader named lines up with the box beside it.**
///
/// Rule (7)'s form for a defect somebody SAW. The report was about the shipped
/// window — `target/release/hello-analyzer-shell`, 1440x900 — and named the
/// words and the thing they should line up with: *"`a pin that can call out`
/// should be in the middle of the box on its left and it is not"*. So the run
/// is found **by those words**, the box by the address family the samples
/// occupy, and the question asked of the pair is whether they share a centre.
///
/// ⚠ **Both read in window coordinates, from the paint.** A run's own `rect` is
/// in its scroll frame and a tag's is window-absolute; the sibling gate in
/// `hello-node-lab` reported these two centres 55 pixels apart on its first run
/// for exactly that reason, on a row already measured as agreeing to the pixel.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1862_the_walk -- --nocapture
/// ```
#[test]
fn r1862_the_walk_reaches_a_legend_row_that_lines_up() {
    const SAID: &str = "a pin that can call out";

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        state.go("lab").expect("the node lab section is open");
        let (painted, _) = painted_at((WIN_W, WIN_H));

        let (content, run, _) = painted
            .runs
            .iter()
            .find(|(content, ..)| content.contains(SAID))
            .unwrap_or_else(|| {
                panic!("no run in the assembled tool says {SAID:?} — the walk no longer reaches it")
            });

        // The box a reader means by "the one on its left": the nearest painted
        // sample, found by containment in the run's own band rather than by an
        // address this host would have to know.
        let samples = painted.family("lab.palette.pin.");
        assert!(
            samples.len() >= 3,
            "the legend paints {} sample(s); the specification declares three \
             appearances and the clause below needs the one beside this run",
            samples.len(),
        );
        let (tag, pin) = samples
            .into_iter()
            .filter_map(|tag| painted.rect(tag).map(|r| (tag, r)))
            .filter(|(_, r)| r.x + r.w <= run.x && r.y < run.y + run.h && run.y < r.y + r.h)
            .min_by_key(|(_, r)| run.x - (r.x + r.w))
            .unwrap_or_else(|| panic!("no legend sample lies beside {SAID:?}"));

        let run_mid = run.y + run.h / 2;
        let pin_mid = pin.y + pin.h / 2;
        println!("the row a reader named: {content:?} centre {run_mid} · {tag} centre {pin_mid}");
        // ★ EXACTLY, and the reason is worth the line: this allowed a pixel
        // until a counterfactual moved the sample by one — the size of the
        // defect the reader reported — and walked through. `band_in` rounds
        // once from the seat's centre, which is what makes equality reachable.
        assert_eq!(
            run_mid, pin_mid,
            "★ the words are centred at {run_mid} and the box beside them at \
             {pin_mid} — which is the report, reproduced",
        );
    });
}

/// Do two rectangles share a pixel?
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// ★★★★★ R1864 — **the host's status band is the host's, at every
/// destination.**
///
/// # The report, and the measurement that answered it
///
/// A reader said the gesture sentence "keeps overlapping other UI elements",
/// three times. Nobody had counted. This check is the counting, kept: painting
/// all six open destinations and intersecting `help_strip_rect()` with every
/// text run on the frame reported **seven runs across three destinations** —
/// four in the capture view's reassembly lanes, two in the node lab's
/// validation panel, one on the settings page — and at all six the strip lay
/// *inside* `page_rect`, which is the guest's rectangle.
///
/// # Why it asks about the BAND and not about the sentence
///
/// A sentence that happens to miss whatever is under it today is one scroll
/// position away from not missing it: `keys` and `logs` reported zero runs
/// while the strip sat squarely inside a scrolling list body. The property
/// worth having is structural — **the band the host draws in is disjoint from
/// the region the destination receives** — and it holds at every scroll
/// position and every window size because neither rectangle is free to reach
/// the other.
///
/// The run census stays as the second half, because "disjoint from the region"
/// is a claim about two rectangles and a reader complained about ink. Asserting
/// both is what makes this fail if a later round paints host chrome into the
/// band from somewhere else.
#[test]
fn r1864_the_status_band_is_the_hosts_at_every_destination() {
    let owner = Owner::new();
    owner.run(|| {
        let roster = spec::destinations();
        let mut destinations = 0;
        for destination in roster.all() {
            if !matches!(
                destination.standing,
                pinion_core::widgets::destination::Standing::Open
            ) {
                continue;
            }
            let key = destination.key.as_ref();
            let state = use_shell_state();
            if key != state.at() {
                state.go(key).unwrap_or_else(|why| panic!("{key}: {why:?}"));
            }
            // ★ R1865 — a toast is ALIVE for this, so the band is asked in the
            // state it is hardest to be right in: the slot holding a sentence
            // that is not the one this file can name.
            state.say(super::Utterance::done("a layout was loaded"));
            let (shot, scene) = painted_at((WIN_W, WIN_H));
            destinations += 1;
            let band = super::status_band_rect();
            let region = super::page_rect(key);
            assert!(
                !overlaps(band, region),
                "at {key} the status band {band:?} and the page region \
                 {region:?} share pixels, so the host's own furniture is drawn \
                 inside what the destination was given",
            );
            // And the slot is IN the band, which is what makes the clause
            // above a statement about the sentence too.
            let seat = super::status_slot_rect();
            assert!(
                seat.x >= band.x
                    && seat.y >= band.y
                    && seat.x + seat.w <= band.x + band.w
                    && seat.y + seat.h <= band.y + band.h,
                "at {key} the band's message slot sits at {seat:?}, outside the \
                 band {band:?} the region was shrunk to make room for",
            );

            // ★★★★★ R1865 — what may be in the band is what the BAND PAINTED,
            // asked by ANCESTRY and not by matching a sentence.
            //
            // This used to exclude one run by comparing its text with
            // `HELP_STRIP`, which was exact while the band held exactly one
            // known sentence and stopped being so the moment the toast moved in
            // — a toast says whatever just happened, so no literal can name it.
            // Ancestry is the question that was always meant: a mark in the
            // band is the band's if the band drew it.
            let intruders: Vec<String> = runs_over(&scene, band, super::STATUS_BAND);
            assert!(
                intruders.is_empty(),
                "at {key} the status band {band:?} has {} run(s) in it that the \
                 band did not draw: {intruders:?} — which is the report this \
                 check was written from, reproduced",
                intruders.len(),
            );
            // ⚠ And the band DID draw something, or the clause above is
            // satisfied by a band nobody filled.
            assert!(
                shot.rect(super::STATUS_SLOT)
                    .is_some_and(|r| overlaps(r, band)),
                "at {key} the band's message slot is not painted inside it, so \
                 the emptiness above is about a band that says nothing",
            );
        }
        assert!(
            destinations >= 6,
            "the rail declares {destinations} open destination(s) and this \
             check was measured against six; a population that shrank silently \
             is how a green sweep comes to mean nothing",
        );
    });
}

/// Every text run that meets `area` and was NOT painted inside `owner_tag`.
///
/// ★★★★★ R1865 — ancestry, not a text match. A gate that excluded the host's
/// own sentence by comparing it with a literal was exact while the band held
/// exactly one known sentence, and stopped being so the moment a toast — which
/// says whatever just happened — moved into the same slot. Whose mark this is
/// is a question about who drew it, and the scene knows; this is
/// `record_painted_marks`' rule (R1758) applied one gate over.
fn runs_over(scene: &Scene, area: Rect, owner_tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    scene.for_each_node(&mut |visit| {
        let (Scene::Text(text), Some(rect)) = (visit.node, visit.absolute_rect()) else {
            return;
        };
        if !overlaps(rect, area) {
            return;
        }
        if visit
            .ancestors
            .iter()
            .any(|a| a.tag().is_some_and(|t| t == owner_tag))
        {
            return;
        }
        out.push(text.content.clone());
    });
    out
}

/// ★★★★★ R1864 — **and no sentence the band can say is short of its own
/// face.**
///
/// The other half of R1864's report, and the one R1863's runtime warning named
/// on the first frame it was ever run against: `box_height=14 needs=18
/// short_by=4`. A band whose height is `line_box` plus its padding cannot be
/// short — but nothing said so, and "cannot be short by construction" is a
/// claim that wants a test for exactly the reason the constant `14` did not.
///
/// ★ R1865 — asked of BOTH sentences the slot can hold, not only the gesture
/// strip. One face ([`super::STATUS_FACE`]) is what makes that one question
/// rather than two: a band whose messages were set in two faces would have to
/// be checked against every message it can ever hold.
#[test]
fn r1864_the_gesture_sentence_is_not_short_of_its_own_face() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        for pose in ["the gesture strip", "a toast"] {
            if pose == "a toast" {
                state.say(super::Utterance::done("a layout was loaded"));
            } else {
                // Past the sentence's life, so the slot is back to the strip.
                for _ in 0..200 {
                    owner.tick_animations(1.0 / 60.0);
                }
            }
            let (painted, scene) = painted_at((WIN_W, WIN_H));
            let message = super::status_slot_rect();
            let mine: Vec<_> = pinion_core::containment::short_boxes(&scene)
                .iter()
                .filter(|s| overlaps(s.rect, message))
                .map(|s| (s.content.clone(), s.short_by))
                .collect();
            assert!(
                mine.is_empty(),
                "with {pose} in the slot, {mine:?} is short of the face it is \
                 set in",
            );
            // ⚠ The premise: something IS in the slot, or the clause above
            // passes by asking about nothing.
            assert!(
                painted.rect(super::STATUS_SLOT).is_some(),
                "with {pose} in the slot, the slot is not painted at all",
            );
        }
        // The band is what guarantees it, so say so where a reader will look:
        // the slot is exactly one line box of the band's own face.
        let seat = super::status_slot_rect();
        assert_eq!(
            seat.h,
            pinion_core::containment::line_box(super::STATUS_FACE),
            "the slot is {} tall and the face it is set in reserves {}",
            seat.h,
            pinion_core::containment::line_box(super::STATUS_FACE),
        );
    });
}

/// ★★★★★ R1864 — **the palette's catalogue clears its own footer, at every
/// height it is asked to work at.**
///
/// # The defect, measured
///
/// `PALETTE_ROW_H` was documented as "sized so the whole catalogue FITS the
/// panel" — an act somebody performed once, by hand, at one window height, and
/// then nothing re-performed. Measured when the status band took 28 pixels off
/// the panel: at the design height the last entry's bottom was **816** and the
/// footer's top **818**, a clearance of two pixels; with the panel 28 shorter
/// the counts were drawn straight through the last widget kind, and it was the
/// caption gate that noticed, by way of a run escaping the row it had landed
/// in.
///
/// # Why it sweeps heights
///
/// The pin was correct at exactly one of them. Asking at one height would
/// reproduce the defect it exists to prevent, so this asks at the height the
/// panel opens at and at heights around it — including ones where the ceiling
/// binds and ones where the room does.
#[test]
fn r1864_the_palette_catalogue_clears_its_own_footer() {
    let owner = Owner::new();
    owner.run(|| {
        for h in [560_u32, 700, 800, WIN_H, 1000, 1400] {
            let (shot, _) = painted_at((WIN_W, h));
            let rows = super::palette_rows();
            let last = rows.last().expect("the catalogue has entries").rect;
            let foot = super::palette_foot_rect();
            assert!(
                last.y + last.h <= foot.y,
                "at a {h}px window the catalogue's last entry ends at {} and \
                 the panel's footer band begins at {} — the counts are drawn \
                 through the last widget kind",
                last.y + last.h,
                foot.y,
            );
            // And the derivation never stretches the rhythm past the height
            // the reference's own rows have.
            assert!(
                super::palette_row_h() <= super::PALETTE_ROW_H,
                "at a {h}px window an entry is {} tall, past the comfortable {}",
                super::palette_row_h(),
                super::PALETTE_ROW_H,
            );
            // The panel really is on the frame at this height, so the two
            // clauses above are about something a reader can see.
            assert!(
                shot.rect("shell.palette").is_some(),
                "at a {h}px window the palette is not painted, so this check \
                 passed by asking about nothing",
            );
        }
    });
}

/// ★★★★★ R1865 — **no sentence the status band says is elided.**
///
/// # The half R1811 believed was covered, and was not
///
/// `toast_width` is an estimate, because `view` is sync and pure by §6.3 and
/// cannot shape. R1811's note says two gates bracket it in opposite directions:
/// *too narrow and `escapes` reports the sentence leaving its box, too wide and
/// `slack` reports the room.* The second half holds. The first does not, and
/// the reason is one word in the paint: these runs are drawn with
/// `TextOverflow::Ellipsis`, so a box too narrow for its sentence does not
/// overflow — **the renderer shortens the sentence instead**. `escapes` is
/// silent by construction, and the estimate had no lower bracket at all.
///
/// Found by looking at a photograph rather than at a check: R1865's pixel demo
/// printed `painted: 'Node Lab sec…'` beside `overflows: False`. The estimate
/// was `px - 6`, which is 5 at an 11-pixel face against a measured ~6 a glyph.
/// It had been narrow at 12 pixels too and `TOAST_MIN_W` was hiding it.
///
/// ⚠ Measured with the SAME cache the layout shaped with, for R1811's reason: a
/// stand-in measure answers about a font nobody painted.
///
/// ⚠⚠ **And this gate's font is not the RENDERER's**, which is the limit worth
/// stating rather than discovering: `pinion_text::LayoutCache::new()` inks the
/// same 16-glyph sentence to about 79px where the running window inks it to
/// about 96px. So a pass here is *this estimate is not narrow for the font this
/// crate's tests shape with* — the narrower of the two — and the running
/// window's half is what `tools/demos/r1861_a_hosts_overlay_becomes_ink.py`
/// reads, off a photograph, in the `painted` field of the run. The defect that
/// forced this gate was found there and not here, and that is the honest
/// division: this one catches the class cheaply on every `cargo test`, the demo
/// catches the instance a reader would actually see.
#[test]
fn r1865_the_bands_sentences_are_not_elided() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        for sentence in [
            "Node Lab section",
            "a saved layout was loaded",
            "you are in Packets",
        ] {
            state.say(super::Utterance::done(sentence));
            let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
            let mut cache = pinion_text::LayoutCache::new();
            pinion_runtime::layout::compute_layout(&mut scene, &mut cache, WIN_W, WIN_H);
            let slot = super::status_slot_rect();
            let mut checked = 0usize;
            scene.for_each_node(&mut |visit| {
                let (Scene::Text(text), Some(rect)) = (visit.node, visit.absolute_rect()) else {
                    return;
                };
                if !overlaps(rect, slot) {
                    return;
                }
                // What the WHOLE sentence needs, unbounded — the width the
                // renderer would have had to elide down to `rect.w`.
                let (wide, _) = cache.ink_size(&text.content, &text.style, &text.runs, None);
                assert!(
                    wide <= rect.w,
                    "★ the band says {:?}, which shapes to {wide}px, in a box \
                     {}px wide — the renderer elides it, and `escapes` cannot \
                     see that because eliding is how it makes it fit",
                    text.content,
                    rect.w,
                );
                checked += 1;
            });
            assert!(
                checked > 0,
                "nothing is in the band's slot while it says {sentence:?}, so \
                 this passed by measuring nothing",
            );
        }
    });
}

/// ★★★★★ R1865 — **a toast is in the same place at every destination, and it
/// is the place the application always speaks from.**
///
/// # The report, and the measurement that answered it
///
/// R1861 stopped the floating toast covering a guest's words by MOVING it off
/// them, and a reader saw the bill before anybody asked: *"it isn't covering
/// anything, but the toast is in a different place on the packet view."*
/// Measured at R1865 before this round's code existed — one sentence, one
/// window, the six open destinations — the box landed at **three different
/// heights, 96 pixels apart**: 838 at the dashboard, keys, logs and settings,
/// 804 at the node lab, 742 on the capture viewer. A toast lives 2.6 seconds,
/// and the property that makes one findable in that time is being where it was
/// last time.
///
/// # What it asserts, and why the second clause is not redundant
///
/// One rectangle across the whole rail, AND that rectangle is the band's
/// message slot. The first alone is satisfied by a toast pinned anywhere at
/// all — including back over a guest, as long as it is consistently over one.
/// The second is what says *the same place* is *the host's place*.
#[test]
fn r1865_a_toast_lands_in_one_place_at_every_destination() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = spec::destinations();
        // Keyed by the rectangle's four numbers, because `Rect` is not `Ord`
        // and the grouping is what the report needs: which destinations agreed.
        let mut seen: BTreeMap<(u32, u32, u32, u32), Vec<String>> = BTreeMap::new();
        let mut destinations = 0;
        for destination in roster.open() {
            let key = destination.key.as_ref();
            if key != state.at() {
                state.go(key).unwrap_or_else(|why| panic!("{key}: {why:?}"));
            }
            state.say(super::Utterance::done("a saved layout was loaded"));
            let shot = painted_at((WIN_W, WIN_H)).0;
            let got = shot
                .rect("shell.toast")
                .unwrap_or_else(|| panic!("at {key} a toast was just said and nothing painted it"));
            seen.entry((got.x, got.y, got.w, got.h))
                .or_default()
                .push(key.to_owned());
            destinations += 1;
        }
        assert!(
            destinations >= 6,
            "the rail declares {destinations} open destination(s); this was \
             measured against six and a population that shrank silently is how \
             a green sweep comes to mean nothing",
        );
        assert_eq!(
            seen.len(),
            1,
            "★ the toast lands in {} different places across the rail, which is \
             the report this check was written from, reproduced: {seen:#?}",
            seen.len(),
        );
        let ((x, y, w, h), _) = seen.into_iter().next().expect("one place");
        let slot = super::status_slot_rect();
        assert_eq!(
            (x, y, h),
            (slot.x, slot.y, slot.h),
            "★ the toast is consistent and it is not the band's message slot \
             {slot:?} — one place is only the right property if it is the place \
             this application speaks from",
        );
        assert!(
            w <= slot.w,
            "the toast is {w}px wide in a {}px slot, so it runs off the end of \
             the band",
            slot.w,
        );
    });
}
