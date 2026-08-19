//! R1730 — the model half of this screen's gates.
//!
//! `painted.rs` next door runs the real pipeline and checks the section a
//! person sees. These check the section as a value: what the filter keeps, what
//! the keyboard walks, what the record pane says, and what the wire publishes
//! about all three.

use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::conformance::Unreconciled;
use pinion_core::external::{ExternalIntrospect, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::widgets::text_field::TextFieldState;

use super::{
    Hit, KeyPatternView, LIST_TAG, ViewOracle, built, conformance_json, declarer_standing, key_at,
    select_declaration, set_query, show_declarer, spec, use_view_state,
};

/// The posture the model checks run in.
const IDLE_FIELD: (TextFieldState, u32) = (TextFieldState::Idle, 0);

fn in_scope<R>(body: impl FnOnce(&std::rc::Rc<super::ViewState>) -> R) -> R {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        body(&state)
    })
}

/// An oracle attached to the scope's own state, which is what the wire drives.
fn oracle(state: &std::rc::Rc<super::ViewState>) -> ViewOracle {
    let mut oracle = ViewOracle::new();
    oracle.attach(std::rc::Rc::clone(state));
    oracle
}

// ── The specification ───────────────────────────────────────────────────────

/// ★★★★★ R1730 — the tables this screen RUNS on are the reference's, or the
/// difference is one somebody wrote down.
///
/// The painted peer of this check is in `painted.rs` and is the stronger claim.
/// This one is here because the wire publishes `conformance` from these tables
/// rather than from the paint: an agent asking the running screen how much of
/// the section is here gets an answer derived from what is asserted here, and
/// `painted.rs` is what ties these tables to the pixels.
#[test]
fn r1730_the_tables_reproduce_the_specification_or_say_where_they_do_not() {
    for &surface in spec::SURFACES {
        let canon = spec::canon(surface);
        let found = canon.diff(&built(surface));
        let unreconciled: Vec<String> = spec::owed(surface)
            .judge(&found)
            .iter()
            .map(Unreconciled::sentence)
            .collect();
        assert!(
            unreconciled.is_empty(),
            "the {surface} surface is not what docs/analyzer-keys-spec.json declares:\n  {}",
            unreconciled.join("\n  "),
        );
    }
}

/// The reproduction is a number rather than an impression, and every surface is
/// either reproduced or owed.
#[test]
fn r1730_the_wire_says_how_much_of_the_section_is_here() {
    let published = conformance_json();
    for &surface in spec::SURFACES {
        let row = &published[surface];
        let specified = row["specified"].as_u64().expect("a count");
        let reproduced = row["reproduced"].as_u64().expect("a count");
        let owed = row["owed"].as_array().expect("an array").len() as u64;
        assert_eq!(
            reproduced + owed,
            specified,
            "every part of the {surface} surface is either reproduced or owed, and none is both",
        );
        assert!(
            row["unreconciled"].as_array().expect("an array").is_empty(),
            "the {surface} surface publishes a difference its ledger does not declare",
        );
    }
}

/// ★★ A specification that failed to load must not read as *this build
/// diverges nowhere*, which is the most flattering possible lie. The loader
/// refuses a malformed pin; this asserts the pin that IS there is not empty.
#[test]
fn r1730_the_specification_is_not_empty() {
    for &surface in spec::SURFACES {
        assert!(
            spec::canon(surface).len() >= 3,
            "the {surface} specification is too small to be the reference's",
        );
    }
}

// ── The filter ──────────────────────────────────────────────────────────────

#[test]
fn r1730_the_section_opens_unfiltered_on_the_references_own_row() {
    in_scope(|state| {
        assert_eq!(state.kept().len(), spec::ROWS.len());
        assert_eq!(state.cursor_row(), spec::OPENING_ROW);
        assert_eq!(
            state.record().pattern,
            spec::ROWS[spec::OPENING_ROW].pattern
        );
    });
}

/// A query narrows the list, and what it kept is what everything downstream
/// reads.
#[test]
fn r1730_a_query_narrows_the_list_and_says_which_clause_dropped_a_row() {
    in_scope(|state| {
        set_query(state, "direction in (declare publish)");
        let kept = state.kept();
        assert_eq!(
            kept.iter().map(|&n| spec::ROWS[n].id).collect::<Vec<_>>(),
            ["2", "3", "7"],
            "the query keeps the declarations that publish",
        );
        let hidden = oracle(state)
            .query("why_hidden")
            .expect("the screen says why a row is hidden");
        let IntrospectValue::Json(hidden) = hidden else {
            panic!("why_hidden is json");
        };
        let rows = hidden.as_array().expect("an array");
        assert_eq!(rows.len(), spec::ROWS.len() - kept.len());
        for row in rows {
            assert_eq!(
                row["clause"].as_str(),
                Some("direction in (declare publish)"),
                "a hidden row names the clause that dropped it",
            );
        }
    });
}

/// ★ A malformed query keeps everything rather than nothing, and the refusal is
/// not swallowed.
///
/// A half-typed query is malformed on nearly every keystroke, so a screen that
/// emptied its list while a person typed would flash the section away and back.
#[test]
fn r1730_a_malformed_query_keeps_everything_and_says_why() {
    in_scope(|state| {
        set_query(state, "direction in (");
        assert_eq!(state.kept().len(), spec::ROWS.len());
        assert!(
            state.query_fault().is_some(),
            "the screen holds the reason the query could not be read",
        );
    });
}

/// ★★★ The summary is DERIVED, which is what the reference does.
///
/// A build that painted the sentence as a constant would show the whole count
/// under a filter that kept two — and that is exactly the defect this tree has
/// closed twice on sibling screens.
#[test]
fn r1730_the_summary_counts_what_the_list_is_showing() {
    in_scope(|state| {
        let all = state.summary();
        assert!(all.starts_with("8 declared"), "{all}");
        assert!(all.ends_with("1 numeric-only"), "{all}");
        set_query(state, "direction in (declare publish)");
        let narrowed = state.summary();
        assert_ne!(all, narrowed, "the summary did not follow the filter");
        assert!(narrowed.starts_with("3 of 8 declared"), "{narrowed}");
        assert!(
            narrowed.ends_with("0 numeric-only"),
            "the one that resolved to a number only is not among the publishers: {narrowed}",
        );
    });
}

// ── The keyboard ────────────────────────────────────────────────────────────

/// The arrows walk the declarations the query kept, and stop at the ends.
#[test]
fn r1730_the_keyboard_walks_the_rows_the_query_kept() {
    in_scope(|state| {
        set_query(state, "direction in (declare publish)");
        let kept = state.kept();
        assert!(kept.len() > 1, "the fixture needs somewhere to walk");
        // `Home` is asserted by where it LEAVES the cursor rather than by what
        // it returns, because the section opens on the first row the filter
        // keeps and a key that reported a move it did not make would repaint
        // the screen for nothing.
        key_at(state, Some(LIST_TAG), "Home");
        assert_eq!(state.cursor_row(), kept[0]);
        let mut seen = vec![state.cursor_row()];
        while key_at(state, Some(LIST_TAG), "ArrowDown") {
            seen.push(state.cursor_row());
            assert!(
                seen.len() <= kept.len(),
                "the walk left the rows the query kept",
            );
        }
        assert_eq!(seen, kept, "the walk is exactly the kept rows, in order");
        assert!(
            !key_at(state, Some(LIST_TAG), "ArrowDown"),
            "the walk runs off the end of the list",
        );
        // `End` on the last kept row moves nothing, and says so: a key that
        // reported a move it did not make would repaint the screen every press.
        assert!(!key_at(state, Some(LIST_TAG), "End"));
        assert_eq!(state.cursor_row(), *kept.last().expect("a kept row"));
        assert!(key_at(state, Some(LIST_TAG), "Home"));
        assert_eq!(state.cursor_row(), kept[0]);
        assert!(!key_at(state, Some(LIST_TAG), "ArrowUp"));
    });
}

/// ★★★★★ R1730 — **a key aimed at somebody else's stop is refused.**
///
/// Measured by mounting this section: the first draft matched on the chord
/// alone, and walking the shell's navigation rail with the arrows stopped one
/// seat short the moment this page was placed at that rail's third seat,
/// because the page took the press the rail was aimed at. The shell's own gate
/// is what reported it — this is the same fact asserted here, where the defect
/// lives, so it cannot come back the next time this screen grows a chord.
///
/// The population is every chord this screen binds, so a new one is covered on
/// the day it is written rather than on the day somebody remembers.
#[test]
fn r1730_a_key_aimed_at_another_stop_is_left_alone() {
    in_scope(|state| {
        for chord in ["ArrowDown", "ArrowUp", "Home", "End", "Enter", "Space"] {
            let before = state.cursor_row();
            assert!(
                !key_at(state, Some("shell.rail"), chord),
                "{chord} aimed at the host's rail was taken by this page",
            );
            assert_eq!(
                state.cursor_row(),
                before,
                "{chord} aimed at the host's rail moved this page's cursor",
            );
        }
        // And the same chords DO work at this screen's own stop, so the check
        // above is not passing because the screen binds nothing.
        select_declaration(state, 0);
        assert!(key_at(state, Some(LIST_TAG), "ArrowDown"));
    });
}

// ── The action out of the section ───────────────────────────────────────────

/// ★★★★★ R1730 — the reference's own action out of this section is drawn, and
/// it refuses with the reason the **rail specification** gives that seat.
///
/// Derived rather than spelled: the reason comes out of
/// `docs/analyzer-rail-spec.json`, the same artifact the shell's rail is judged
/// against. A second copy here is how a button comes to promise something the
/// navigation refuses.
#[test]
fn r1730_the_action_out_of_the_section_refuses_with_the_rails_own_reason() {
    let why = declarer_standing();
    assert_eq!(
        why.kind(),
        pinion_core::availability::UnavailableKind::Reserved,
        "the section this action leads to is one the reference itself locks",
    );
    assert!(
        why.detail().contains("requirement"),
        "the refusal names what the seat is booked under, and reads {:?}",
        why.detail(),
    );
    in_scope(|state| {
        show_declarer(state);
        let said = state.said_sentence();
        assert!(
            said.contains(spec::DECLARER_SECTION) && said.contains(why.detail()),
            "the refusal reaches the person and reads {said:?}",
        );
        assert!(
            said.contains("release"),
            "and it says what would open it: {said:?}",
        );
    });
}

/// The action's standing is published, so an agent can read it rather than
/// press a button to find out.
#[test]
fn r1730_the_wire_publishes_why_the_action_refuses() {
    in_scope(|state| {
        let IntrospectValue::Json(declarer) = oracle(state)
            .query("declarer")
            .expect("the screen publishes its action's standing")
        else {
            panic!("declarer is json");
        };
        assert_eq!(
            declarer["section"].as_str(),
            Some(spec::DECLARER_SECTION),
            "it names the section it leads to",
        );
        assert_eq!(declarer["kind"].as_str(), Some("reserved"));
        assert_eq!(
            declarer["recourse"].as_str(),
            Some("await_release"),
            "and what the reader can do about it",
        );
    });
}

// ── The pointer ─────────────────────────────────────────────────────────────

/// A press on a cell is a press on its row — the model half of the painted
/// check next door.
#[test]
fn r1730_a_cell_is_addressed_as_its_row() {
    for (n, row) in spec::ROWS.iter().enumerate() {
        for column in spec::COLUMNS {
            assert_eq!(
                Hit::of_tag(&format!("kp.list.cell.{n}_{}", column.key)),
                Hit::Declaration(n),
                "the {} cell of declaration {} does not address its row",
                column.key,
                row.id,
            );
        }
    }
}

/// The wire's press and a pointer press reach the same handler.
#[test]
fn r1730_the_wire_selects_the_same_declaration_a_press_would() {
    in_scope(|state| {
        let mut oracle = oracle(state);
        oracle
            .invoke("select_declaration", IntrospectValue::Int(5))
            .expect("the wire selects a declaration");
        assert_eq!(state.cursor_row(), 5);
        assert!(
            oracle
                .invoke("select_declaration", IntrospectValue::Int(99))
                .is_err(),
            "a declaration nobody captured is refused rather than clamped",
        );
    });
}

// ── What the screen tells a person it can do ────────────────────────────────

/// ★★★ Every gesture the screen advertises is one it actually binds.
///
/// R1703 built this gate on a sibling screen after a gesture it advertised for
/// its whole life turned out to be dead. The population is the advertisement,
/// so a line added to the strip with nothing behind it fails here.
#[test]
fn r1730_every_advertised_gesture_is_bound() {
    in_scope(|state| {
        for (gesture, _) in spec::GESTURES {
            let bound = match *gesture {
                "click a declaration" => {
                    select_declaration(state, 0);
                    state.cursor_row() == 0
                }
                "type in the filter" => {
                    set_query(state, "direction in (declare subscribe)");
                    let narrowed = state.kept().len() < spec::ROWS.len();
                    set_query(state, "");
                    narrowed
                }
                "up and down" => {
                    select_declaration(state, 0);
                    key_at(state, Some(LIST_TAG), "ArrowDown")
                }
                other => panic!("the strip advertises {other} and nothing checks it"),
            };
            assert!(bound, "the screen advertises {gesture} and nothing does it");
        }
    });
}

// ── Accessibility ───────────────────────────────────────────────────────────

/// The grid announces the rows the query kept, not the rows it holds.
#[test]
fn r1730_the_announced_grid_is_the_list_a_reader_sees() {
    in_scope(|state| {
        set_query(state, "direction in (declare publish)");
        let nodes = KeyPatternView::access_node(&IDLE_FIELD, Some(LIST_TAG));
        let grid = nodes
            .iter()
            .find(|n| n.tag == LIST_TAG)
            .expect("the list announces itself");
        assert_eq!(grid.role, AriaRole::Grid);
        assert_eq!(
            grid.row_count,
            Some(u32::try_from(state.kept().len() + 1).expect("a small count")),
            "the announced row count is what is presented, header included",
        );
        let announced: Vec<&str> = nodes
            .iter()
            .filter(|n| n.tag.starts_with("kp.list.row."))
            .map(|n| n.tag.as_str())
            .collect();
        assert_eq!(
            announced.len(),
            state.kept().len(),
            "a reader is told about the rows the screen draws and no others",
        );
    });
}

/// ★★ The record pane's action carries its reason into the accessibility tree.
///
/// A disabled bit would tell a reader the button cannot be pressed and not one
/// word about what would open it.
#[test]
fn r1730_the_action_carries_its_reason_to_a_listener() {
    in_scope(|_| {
        let nodes = KeyPatternView::access_node(&IDLE_FIELD, None);
        let action = nodes
            .iter()
            .find(|n| n.tag == "kp.detail.declarer")
            .expect("the action announces itself");
        assert_eq!(action.role, AriaRole::Button);
        let why = action
            .unavailable
            .as_ref()
            .expect("the action says why it refuses");
        assert_eq!(why.kind(), declarer_standing().kind());
        assert_eq!(why.detail(), declarer_standing().detail());
    });
}

/// Every part of the record pane a reader can land on has a name and a reading.
#[test]
fn r1730_every_record_part_reads_as_something() {
    in_scope(|_| {
        let nodes = KeyPatternView::access_node(&IDLE_FIELD, None);
        for part in &spec::DETAIL[1..] {
            let tag = format!("kp.detail.{}", part.key);
            let node = nodes
                .iter()
                .find(|n| n.tag == tag)
                .unwrap_or_else(|| panic!("{tag} is not announced"));
            assert!(
                node.name.as_ref().is_some_and(|name| !name.is_empty()),
                "{tag} is announced with no name a reader could hear",
            );
        }
    });
}

// ── The rows ────────────────────────────────────────────────────────────────

/// The declarations are the reference's eight, and the one it draws needing
/// attention is among them.
#[test]
fn r1730_the_section_holds_the_references_own_declarations() {
    assert_eq!(spec::ROWS.len(), 8);
    let unresolved: Vec<&str> = spec::ROWS
        .iter()
        .filter(|r| r.health == spec::Health::NumericOnly)
        .map(|r| r.id)
        .collect();
    assert_eq!(
        unresolved,
        ["5"],
        "the reference's section has exactly one declaration it could only number",
    );
    for row in spec::ROWS {
        assert!(
            !row.endpoints.is_empty(),
            "declaration {} matches nothing, so its record pane has an empty part",
            row.id,
        );
        assert_eq!(
            row.attributes().len(),
            spec::COLUMNS.len(),
            "declaration {} does not answer every column a query may address",
            row.id,
        );
    }
}

/// Every cell a query can address is a cell the list draws, and the other way
/// round.
#[test]
fn r1730_a_query_addresses_the_columns_the_list_draws() {
    assert_eq!(
        spec::query_columns(),
        spec::COLUMNS.iter().map(|c| c.key).collect::<Vec<_>>(),
    );
}

// ── The layout floor ────────────────────────────────────────────────────────

/// ★★ The floor this screen declares is one it can actually lay out at.
///
/// The width is a sum of facts elsewhere and needs no check. The height is the
/// one term arithmetic at the declaration site cannot reach — the record pane's
/// stack is built by walking a table with per-part heights — so it is asserted
/// here against the stack itself. A screen that grew a part and left the number
/// alone would otherwise declare a floor at which its own pane runs off the
/// bottom, which is the shape R1711 measured on a sibling: two declarations
/// about one screen, contradicting each other, and nothing comparing them.
#[test]
fn r1730_the_declared_floor_is_one_the_record_pane_fits_in() {
    let bottom = super::detail_parts()
        .iter()
        .map(|(_, rect)| rect.y + rect.h)
        .max()
        .expect("the record pane has parts");
    assert!(
        bottom + spec::PAD <= super::MIN_H,
        "the record pane's stack ends at {bottom} and the declared floor is {}",
        super::MIN_H,
    );
    assert_eq!(
        super::SHRINK.comfortable(),
        (super::MIN_W, super::MIN_H),
        "the size the layout is complete at IS what the window policy calls comfortable",
    );
}
