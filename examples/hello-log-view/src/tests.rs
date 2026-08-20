//! R1731 — the model half of this screen's gates.
//!
//! `painted.rs` next door runs the real pipeline and checks the section a
//! person sees. These check the section as a value: what the two narrowings
//! keep, what the decode pane says about a frame that never arrived, and what
//! the wire publishes about both.

use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::conformance::Unreconciled;
use pinion_core::external::{ExternalIntrospect, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::widgets::text_field::TextFieldState;

use super::{
    Hit, LIST_TAG, LogView, ViewOracle, built, choose_severity, conformance_json, detail_parts,
    key_at, select_event, set_capturing, set_query, spec, use_view_state,
};

const IDLE_FIELD: (TextFieldState, u32) = (TextFieldState::Idle, 0);

/// One part of the decode pane for `record`, by key.
///
/// The tests' own helper rather than the screen's: the decode pane has no
/// pressable part, so a production lookup by key would be a function nothing
/// calls — and `-D dead-code` says so, which is the compiler doing the job a
/// census would otherwise have to.
fn part_rect(record: &'static spec::RowSpec, key: &str) -> Option<pinion_core::scene::Rect> {
    detail_parts(record)
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, rect)| rect)
}

fn in_scope<R>(body: impl FnOnce(&std::rc::Rc<super::ViewState>) -> R) -> R {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        body(&state)
    })
}

fn oracle(state: &std::rc::Rc<super::ViewState>) -> ViewOracle {
    let mut oracle = ViewOracle::new();
    oracle.attach(std::rc::Rc::clone(state));
    oracle
}

// ── The specification ───────────────────────────────────────────────────────

#[test]
fn r1731_the_tables_reproduce_the_specification_or_say_where_they_do_not() {
    let doc = spec::document();
    for surface in doc.surfaces() {
        // ★ R1742 — and `expect` here is a check, not plumbing: this section's
        // surfaces are drawn whenever it is showing, so a build that started
        // answering `away` for one of them would be declining a judgement it
        // has no session-dependent reason to decline.
        let parts = built(surface)
            .parts()
            .unwrap_or_else(|| panic!("the {surface} surface is always on this screen"))
            .to_vec();
        let unreconciled: Vec<String> = doc
            .unreconciled(surface, &parts)
            .iter()
            .map(Unreconciled::sentence)
            .collect();
        assert!(
            unreconciled.is_empty(),
            "the {surface} surface is not what docs/analyzer-logs-spec.json declares:\n  {}",
            unreconciled.join("\n  "),
        );
    }
}

#[test]
fn r1731_the_wire_says_how_much_of_the_section_is_here() {
    let published = conformance_json();
    let doc = spec::document();
    assert_eq!(doc.surfaces().count(), 3, "the pin fixes three surfaces");
    for surface in doc.surfaces() {
        let row = &published[surface];
        let specified = row["specified"].as_u64().expect("a count");
        let reproduced = row["reproduced"].as_u64().expect("a count");
        let owed = row["owed"].as_array().expect("an array").len() as u64;
        assert_eq!(
            reproduced + owed,
            specified,
            "every part of the {surface} surface is either reproduced or owed",
        );
        assert!(
            row["unreconciled"].as_array().expect("an array").is_empty(),
            "the {surface} surface publishes a difference its ledger does not declare",
        );
    }
}

// ── The two narrowings ──────────────────────────────────────────────────────

#[test]
fn r1731_the_section_opens_on_the_newest_event_with_everything_shown() {
    in_scope(|state| {
        assert_eq!(state.kept().len(), spec::ROWS.len());
        assert_eq!(state.cursor_row(), spec::OPENING_ROW);
        assert!(state.capturing.get(), "a log section opens capturing");
    });
}

/// ★★★★★ The severity choice is EXCLUSIVE and ORDERED — *warnings* means
/// warnings **and** errors.
///
/// Three independent toggles could not say that, which is why the control is
/// one choice with a floor rather than three flags. The check is the ordering
/// itself: each choice keeps a superset of the next.
#[test]
fn r1731_a_severity_choice_keeps_that_severity_and_worse() {
    in_scope(|state| {
        choose_severity(state, 0);
        let all = state.kept();
        choose_severity(state, 1);
        let warn = state.kept();
        choose_severity(state, 2);
        let error = state.kept();

        assert_eq!(all.len(), spec::ROWS.len());
        assert!(
            warn.iter().all(|n| all.contains(n)) && warn.len() < all.len(),
            "warnings keep fewer events, and every one of them was already shown",
        );
        assert!(
            error.iter().all(|n| warn.contains(n)) && error.len() < warn.len(),
            "★ errors keep a SUBSET of warnings — the ordering is what a floor means",
        );
        for &n in &warn {
            assert!(
                spec::ROWS[n].severity >= spec::Severity::Warn,
                "an event below the floor survived it",
            );
        }
        assert!(!error.is_empty(), "the fixture needs an error to keep");
    });
}

/// The two narrowings compose, and a hidden row says WHICH one dropped it.
#[test]
fn r1731_a_hidden_event_says_which_narrowing_dropped_it() {
    in_scope(|state| {
        choose_severity(state, 1);
        set_query(state, "source in (P-03)");
        let kept = state.kept();
        assert_eq!(
            kept.iter()
                .map(|&n| spec::ROWS[n].source)
                .collect::<Vec<_>>(),
            ["P-03"],
            "both narrowings apply, not only the last one set",
        );
        let IntrospectValue::Json(hidden) = oracle(state)
            .query("why_hidden")
            .expect("the screen says why a row is hidden")
        else {
            panic!("why_hidden is json");
        };
        let rows = hidden.as_array().expect("an array");
        assert_eq!(rows.len(), spec::ROWS.len() - kept.len());
        assert!(
            rows.iter().any(|r| r["severity"] == true),
            "★ some rows went for the severity, and the reader is told so",
        );
        assert!(
            rows.iter().any(|r| r["clause"].is_string()),
            "★★ and others for the query, which is a different thing to undo",
        );
    });
}

#[test]
fn r1731_a_malformed_query_keeps_everything_and_says_why() {
    in_scope(|state| {
        set_query(state, "type in (");
        assert_eq!(state.kept().len(), spec::ROWS.len());
        assert!(state.query_fault().is_some());
    });
}

/// ★ Narrowing to nothing is SAID, because an empty log looks exactly like a
/// screen that broke.
#[test]
fn r1731_narrowing_to_nothing_says_so() {
    in_scope(|state| {
        set_query(state, "source in (P-03)");
        choose_severity(state, 2);
        assert!(state.kept().is_empty(), "the fixture narrows to nothing");
        assert!(
            state.said_sentence().contains("nothing"),
            "the screen said {:?}",
            state.said_sentence(),
        );
    });
}

// ── The capture mark ────────────────────────────────────────────────────────

/// ★★ The part is always drawn and its READING is the state.
///
/// The reference draws the mark while capturing and nothing when it is not.
/// This build keeps the part, because a blank cannot be told apart from a build
/// that forgot to draw it — and the specification fixes that the part is there
/// rather than what it reads.
#[test]
fn r1731_the_capture_mark_reads_the_state_rather_than_vanishing() {
    in_scope(|state| {
        let live = state.capture_reading();
        assert!(live.starts_with("LIVE"), "{live}");
        set_capturing(state, false);
        let paused = state.capture_reading();
        assert!(paused.starts_with("PAUSED"), "{paused}");
        assert_ne!(live, paused);
        assert!(
            !paused.is_empty(),
            "the part still says something when the capture is not running",
        );
    });
}

// ── The decode pane ─────────────────────────────────────────────────────────

/// ★★★ A frame that never arrived is DRAWN as such.
///
/// The reference has this row and this build has it: a warning whose keep-alive
/// timed out has no bytes, and a byte pane that simply went blank would be
/// indistinguishable from a decode that failed.
#[test]
fn r1731_an_event_with_no_frame_says_so_rather_than_going_blank() {
    in_scope(|state| {
        let empty = spec::ROWS
            .iter()
            .position(|r| r.bytes.is_empty())
            .expect("the fixture holds an event whose frame never arrived");
        select_event(state, empty);
        let record = state.record();
        assert!(record.bytes.is_empty());
        let nodes = LogView::access_node(&IDLE_FIELD, None);
        let bytes = nodes
            .iter()
            .find(|n| n.tag == "lv.detail.bytes")
            .expect("the byte part announces itself");
        assert!(
            format!("{:?}", bytes.value).contains(spec::NO_FRAME),
            "a reader is told the frame never arrived: {:?}",
            bytes.value,
        );
        // And the part still has a rectangle, so the pane does not collapse.
        assert!(
            part_rect(record, "bytes").is_some_and(|r| r.h > 0),
            "the byte part keeps its place",
        );
    });
}

/// The decode pane's parts are measured from what they hold, so a record with
/// more fields makes a taller pane rather than an overlapping one.
#[test]
fn r1731_a_longer_decode_makes_a_taller_pane() {
    let most = spec::ROWS
        .iter()
        .max_by_key(|r| r.fields.len())
        .expect("the fixture holds events");
    let least = spec::ROWS
        .iter()
        .min_by_key(|r| r.fields.len())
        .expect("the fixture holds events");
    assert!(most.fields.len() > least.fields.len(), "the fixture varies");
    let tall = part_rect(most, "layers").expect("the pane has a layers part");
    let short = part_rect(least, "layers").expect("the pane has a layers part");
    assert!(
        tall.h > short.h,
        "a record with more fields gets a taller part: {tall:?} vs {short:?}",
    );
    // And the part after it moves down rather than being drawn over.
    let after_tall = part_rect(most, "bytes").expect("the pane has a byte part");
    let after_short = part_rect(least, "bytes").expect("the pane has a byte part");
    assert!(after_tall.y > after_short.y);
}

// ── The keyboard and the pointer ────────────────────────────────────────────

#[test]
fn r1731_the_keyboard_walks_the_events_the_narrowings_kept() {
    in_scope(|state| {
        choose_severity(state, 1);
        let kept = state.kept();
        assert!(kept.len() > 1, "the fixture needs somewhere to walk");
        key_at(state, Some(LIST_TAG), "Home");
        assert_eq!(state.cursor_row(), kept[0]);
        let mut seen = vec![state.cursor_row()];
        while key_at(state, Some(LIST_TAG), "ArrowDown") {
            seen.push(state.cursor_row());
            assert!(seen.len() <= kept.len(), "the walk left the kept events");
        }
        assert_eq!(seen, kept, "the walk is exactly the kept events, in order");
        assert!(!key_at(state, Some(LIST_TAG), "ArrowDown"));
    });
}

/// ★★★★★ A key aimed at somebody else's stop is refused — the rule R1730
/// measured by mounting its sibling and watching the host's rail walk stop one
/// seat short.
#[test]
fn r1731_a_key_aimed_at_another_stop_is_left_alone() {
    in_scope(|state| {
        for chord in ["ArrowDown", "ArrowUp", "Home", "End"] {
            let before = state.cursor_row();
            assert!(
                !key_at(state, Some("shell.rail"), chord),
                "{chord} aimed at the host's rail was taken by this page",
            );
            assert_eq!(state.cursor_row(), before);
        }
        select_event(state, 0);
        assert!(key_at(state, Some(LIST_TAG), "ArrowDown"));
    });
}

#[test]
fn r1731_a_cell_is_addressed_as_its_row() {
    for (n, row) in spec::ROWS.iter().enumerate() {
        for column in spec::COLUMNS {
            assert_eq!(
                Hit::of_tag(&format!("lv.list.cell.{n}_{}", column.key)),
                Hit::Event(n),
                "the {} cell of event {} does not address its row",
                column.key,
                row.time,
            );
        }
    }
}

#[test]
fn r1731_the_wire_drives_the_same_handlers_a_press_would() {
    in_scope(|state| {
        let mut oracle = oracle(state);
        oracle
            .invoke("select_event", IntrospectValue::Int(3))
            .expect("the wire selects an event");
        assert_eq!(state.cursor_row(), 3);
        oracle
            .invoke("choose_severity", IntrospectValue::Text("error".to_owned()))
            .expect("the wire chooses a severity");
        assert_eq!(state.choice.get(), 2);
        assert!(
            oracle
                .invoke("choose_severity", IntrospectValue::Text("worse".to_owned()))
                .is_err(),
            "a severity nobody declared is refused rather than clamped",
        );
        oracle
            .invoke("capture", IntrospectValue::Text("off".to_owned()))
            .expect("the wire pauses the capture");
        assert!(!state.capturing.get());
    });
}

// ── What the screen tells a person it can do ────────────────────────────────

#[test]
fn r1731_every_advertised_gesture_is_bound() {
    in_scope(|state| {
        for (gesture, _) in spec::GESTURES {
            let bound = match *gesture {
                "click an event" => {
                    select_event(state, 2);
                    state.cursor_row() == 2
                }
                "type in the filter" => {
                    set_query(state, "type in (Data)");
                    let narrowed = state.kept().len() < spec::ROWS.len();
                    set_query(state, "");
                    narrowed
                }
                "click a severity" => {
                    choose_severity(state, 2);
                    let narrowed = state.kept().len() < spec::ROWS.len();
                    choose_severity(state, 0);
                    narrowed
                }
                "up and down" => {
                    select_event(state, 0);
                    key_at(state, Some(LIST_TAG), "ArrowDown")
                }
                other => panic!("the strip advertises {other} and nothing checks it"),
            };
            assert!(bound, "the screen advertises {gesture} and nothing does it");
        }
    });
}

// ── Accessibility ───────────────────────────────────────────────────────────

#[test]
fn r1731_the_announced_grid_is_the_list_a_reader_sees() {
    in_scope(|state| {
        choose_severity(state, 1);
        let nodes = LogView::access_node(&IDLE_FIELD, Some(LIST_TAG));
        let grid = nodes
            .iter()
            .find(|n| n.tag == LIST_TAG)
            .expect("the list announces itself");
        assert_eq!(grid.role, AriaRole::Grid);
        assert_eq!(
            grid.row_count,
            Some(u32::try_from(state.kept().len() + 1).expect("a small count")),
        );
    });
}

/// ★★ The severity control announces as ONE choice with exactly one selected
/// member, which is what makes it a choice rather than three switches.
#[test]
fn r1731_the_severity_control_announces_as_one_choice() {
    in_scope(|state| {
        choose_severity(state, 1);
        let nodes = LogView::access_node(&IDLE_FIELD, None);
        let group = nodes
            .iter()
            .find(|n| n.tag == "lv.header.severity")
            .expect("the severity control announces itself");
        assert_eq!(group.role, AriaRole::RadioGroup);
        let selected: Vec<&str> = nodes
            .iter()
            .filter(|n| n.tag.starts_with("lv.severity.") && n.selected == Some(true))
            .map(|n| n.tag.as_str())
            .collect();
        assert_eq!(
            selected,
            [format!("lv.severity.{}", spec::CHOICES[1].key).as_str()],
            "exactly one member is selected, and it is the chosen one",
        );
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n.tag.starts_with("lv.severity."))
                .count(),
            spec::Severity::ALL.len(),
            "one member per severity the vocabulary has",
        );
        let _ = state;
    });
}

// ── The events ──────────────────────────────────────────────────────────────

#[test]
fn r1731_the_section_holds_the_references_own_events() {
    assert_eq!(spec::ROWS.len(), 10);
    let warnings = spec::ROWS
        .iter()
        .filter(|r| r.severity == spec::Severity::Warn)
        .count();
    let errors = spec::ROWS
        .iter()
        .filter(|r| r.severity == spec::Severity::Error)
        .count();
    assert_eq!(
        (warnings, errors),
        (1, 1),
        "the reference's capture has one warning and one error, which is what makes \
         the severity choice worth having",
    );
    for row in spec::ROWS {
        assert!(
            !row.fields.is_empty(),
            "the event at {} decodes to nothing, so its pane has an empty part",
            row.time,
        );
        assert_eq!(row.attributes().len(), spec::COLUMNS.len());
    }
}

#[test]
fn r1731_a_query_addresses_the_columns_the_list_draws() {
    assert_eq!(
        spec::query_columns(),
        spec::COLUMNS.iter().map(|c| c.key).collect::<Vec<_>>(),
    );
}

// ── The layout floor ────────────────────────────────────────────────────────

/// The floor this screen declares is one its **tallest** decode can lay out in.
///
/// The taller of the two columns is the pane, and its height depends on the
/// record — so the check is over the record with the most decoded fields rather
/// than over the one the screen opens on.
#[test]
fn r1731_the_declared_floor_fits_the_tallest_decode() {
    let most = spec::ROWS
        .iter()
        .max_by_key(|r| r.fields.len())
        .expect("the fixture holds events");
    let bottom = super::detail_parts(most)
        .iter()
        .map(|(_, rect)| rect.y + rect.h)
        .max()
        .expect("the pane has parts");
    assert!(
        bottom + spec::PAD <= super::MIN_H,
        "the decode pane's tallest stack ends at {bottom} and the declared floor is {}",
        super::MIN_H,
    );
    assert_eq!(super::SHRINK.comfortable(), (super::MIN_W, super::MIN_H));
}
