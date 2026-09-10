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
    Hit, LIST_TAG, LogView, ViewOracle, address, choose_severity, conformance_json, detail_parts,
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

/// The parts one surface's own table declares, in the table's order.
///
/// ★ R1758 — this used to be `super::built`, which is what the WIRE answered
/// with. It is a fixture now, because the wire answers from the paint: see
/// `crate::judge`. The check below is a **weaker** claim than it was — that
/// this screen's tables agree with the pin — and `painted.rs` is what ties
/// those tables to the pixels.
fn tabled(surface: &str) -> Vec<pinion_core::conformance::Part> {
    use pinion_core::conformance::Part;
    match surface {
        "header" => spec::HEADER
            .iter()
            .map(|p| Part::new(p.key, p.title))
            .collect(),
        "columns" => spec::COLUMNS
            .iter()
            .map(|c| Part::new(c.key, c.title))
            .collect(),
        "detail" => spec::DETAIL
            .iter()
            .map(|p| Part::new(p.key, p.title))
            .collect(),
        other => panic!("no surface named {other}"),
    }
}

#[test]
fn r1731_the_tables_reproduce_the_specification_or_say_where_they_do_not() {
    let doc = spec::document();
    for surface in doc.surfaces() {
        let parts = tabled(surface);
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

/// ★★★★★ R1758 — **a screen that has not painted cannot say it reproduced
/// anything.**
///
/// The sibling of the key-pattern section's gate of the same shape, written
/// against the same measurement: standing on another page of the assembled
/// application, this section reported `15 of 15 reproduced, away none` while it
/// had drawn no frame in that session. The peer of this check is in
/// `painted.rs`, where the same slot reports every surface standing once a
/// frame is recorded — both are needed, because a verdict that is always away
/// is as uninformative as one that is never away.
#[test]
fn r1758_the_wire_declines_to_judge_a_screen_that_has_not_painted() {
    pinion_core::painted::forget_painted_regions(super::VIEW_TAG);

    let published = conformance_json();
    let doc = spec::document();
    assert_eq!(doc.surfaces().count(), 3, "the pin fixes three surfaces");
    assert_eq!(
        published["evidence"], "paint",
        "the verdict says what it was read from, and this screen reads the frame",
    );
    assert_eq!(published["reproduced"], 0);
    assert_eq!(published["standing"], 0);
    assert!(
        !published["reconciles"].as_bool().expect("a verdict"),
        "declining to be judged is not passing",
    );
    let surfaces = &published["surfaces"];
    for surface in doc.surfaces() {
        let row = &surfaces[surface];
        assert_eq!(row["standing"], false, "the {surface} surface is not shown");
        assert!(
            row["why"].as_str().is_some_and(|why| !why.is_empty()),
            "the {surface} surface says WHY it is not judged rather than going silent",
        );
        assert_eq!(
            row["canon"].as_array().expect("an array").len(),
            doc.canon(surface).expect("a declared surface").len(),
            "★ R1758 — the verdict names WHICH parts it is about",
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
            .find(|n| n.tag == address::detail("bytes"))
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
                Hit::of_tag(&address::cell(n, column.key)),
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
            .find(|n| n.tag == address::SEVERITY_GROUP)
            .expect("the severity control announces itself");
        assert_eq!(group.role, AriaRole::RadioGroup);
        let selected: Vec<&str> = nodes
            .iter()
            .filter(|n| address::severity_key(&n.tag).is_some() && n.selected == Some(true))
            .map(|n| n.tag.as_str())
            .collect();
        assert_eq!(
            selected,
            [address::severity(spec::CHOICES[1].key).as_str()],
            "exactly one member is selected, and it is the chosen one",
        );
        assert_eq!(
            nodes
                .iter()
                .filter(|n| address::severity_key(&n.tag).is_some())
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

// ── The addresses ───────────────────────────────────────────────────────────

/// Every module of this crate, for the address gate to read.
fn crate_sources() -> [(&'static str, &'static str); 6] {
    [
        ("address.rs", include_str!("address.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("spec.rs", include_str!("spec.rs")),
        ("judge.rs", include_str!("judge.rs")),
        ("painted.rs", include_str!("painted.rs")),
        ("tests.rs", include_str!("tests.rs")),
    ]
}

/// ★★★★★ R2124 — **every module this crate declares is one the address gate
/// reads.**
///
/// Derived from `lib.rs`'s own `mod` lines rather than compared with a second
/// list: a module added tomorrow could otherwise spell a hundred addresses
/// while the namespace gate below reported zero — the shape R2053 measured on
/// the node lab, where the gate's own population was six modules of eleven.
#[test]
fn r2124_every_module_is_read() {
    const LIB: &str = include_str!("lib.rs");
    let mut declared: Vec<String> = LIB
        .lines()
        .filter_map(|line| {
            let name = line
                .trim()
                .strip_prefix("pub mod ")
                .or_else(|| line.trim().strip_prefix("mod "))?
                .strip_suffix(';')?;
            (!name.contains(' ')).then(|| format!("{name}.rs"))
        })
        .collect();
    declared.push("lib.rs".to_owned());
    declared.sort_unstable();
    declared.dedup();
    let mut read: Vec<String> = crate_sources()
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    read.sort_unstable();
    assert_eq!(
        declared, read,
        "★★★★★ the address gate reads {read:?} and this crate declares \
         {declared:?} — a module outside that roster can spell any address it \
         likes and the namespace gate below will report zero"
    );
}

/// ★★★★★ R2124 — **nobody in this CRATE but the declaration spells this
/// screen's stem.**
///
/// Measured at entry: **fifty-one literals** across four modules, all inside
/// this crate, so the crate half of this section's debt is an assertion of zero
/// rather than a ratchet.
///
/// ⚠⚠ **This section's population is THREE, and this gate covers ONE of them.**
/// That sentence is the reason this test exists in the shape it does, and it is
/// what the twenty-five instalments before it could not say: `sv.*` and `tv.*`
/// were crate-only, so one assertion finished them; `kp.*` was crate + walk. The
/// log section is the first family with all three alive, and the two this gate
/// cannot see are held elsewhere, by name:
///
/// * the **host** — `hello-analyzer-shell` mounts this section and spelled two
///   of its addresses. `include_str!` reaches this crate's files and no others,
///   so a gate here is structurally blind to that file. The host's own
///   `r2124_this_host_spells_no_mounted_screens_painted_address` takes the
///   needle from [`crate::address::NAMESPACE`] and refuses a member there;
/// * the **walks** — Python, which no Rust gate reads.
///   `tools/painted_addresses.py` ratchets them, and
///   [`r2124_the_wire_hands_over_every_address_a_walk_would_spell`] is what
///   makes the repair possible at all.
///
/// Saying so here is not decoration: a reader who took this green as covering
/// the section would be wrong in exactly the direction that let two host
/// spellings sit unnoticed since R2049.
///
/// ⚠ The needle is the BARE stem, so a module spelling it with no separator —
/// `paint_stems` wanted exactly that until this round — is caught too.
///
/// ⚠⚠ The vacuity guard comes FIRST: a needle that matched nothing would clear
/// every module below and read exactly like success, which is the failure this
/// family of gates is most exposed to.
#[test]
fn r2124_no_module_but_the_declaration_spells_this_screens_namespace() {
    let anchor = format!("\"{}", address::STEM);
    let sources = crate_sources();
    let declaration = sources
        .iter()
        .find(|(name, _)| *name == "address.rs")
        .expect("the declaring module is in the roster");
    let declared = declaration.1.matches(anchor.as_str()).count();
    assert!(
        declared >= 15,
        "★ `address.rs` spells the stem {declared} time(s), which is not what a \
         module declaring three panes, a root, a tip, a query field and five \
         keyed families looks like — the anchor is wrong and the emptiness \
         below means nothing"
    );
    for (name, body) in &sources {
        assert!(
            !body.is_empty(),
            "★ `{name}` reads as empty — the `include_str!` is not reaching the \
             file, and a gate counting over nothing is green"
        );
    }
    let mut spellers: Vec<(&str, usize)> = Vec::new();
    for (name, body) in &sources {
        if *name == "address.rs" {
            continue;
        }
        let count = body.matches(anchor.as_str()).count();
        if count > 0 {
            spellers.push((name, count));
        }
    }
    assert!(
        spellers.is_empty(),
        "★★★★★ every painted address of this screen begins with this crate's \
         stem and is declared in `address.rs`; these (file, times) spell one \
         themselves: {spellers:?}. A literal here is a second copy of a \
         composition the declaration already publishes, and one wrong letter \
         in it paints a mark no reader can find."
    );
}

/// ★★★★★ R2124 — **the declaration agrees with itself and with the
/// specification, and its inverses are disjoint.**
///
/// *Nobody re-spells the address* and *the composition is right* are two
/// claims, and the first is satisfied by a declaration that composes nonsense.
#[test]
fn r2124_every_log_address_is_derived() {
    // The two spellings of the stem agree.
    assert_eq!(address::NAMESPACE, format!("{}.", address::STEM));
    // Each surface's two forms agree, so a reader classifying on the bare form
    // cannot drift from one composing on the separator form.
    assert_eq!(address::HEADER_SEAT, format!("{}.", address::HEADER));
    assert_eq!(address::LIST_SEAT, format!("{}.", address::LIST));
    assert_eq!(address::DETAIL_SEAT, format!("{}.", address::DETAIL));
    // ★ The marks a caller needs as a `&'static str` ARE the derivation.
    assert_eq!(address::LIST_HEADER, address::list("header"));
    assert_eq!(address::LIST_BODY, address::list("body"));
    assert_eq!(address::LIST_OPEN, address::list("open"));
    assert_eq!(address::HEADER_LIVE, address::header("live"));
    assert_eq!(address::SEVERITY_GROUP, address::header("severity"));
    // ★★ Every part the SPECIFICATION names round-trips through its own
    // surface's inverse and is refused by the others — the surface is half the
    // answer, because a key alone cannot say which table it came from.
    let mut checked = 0;
    for part in spec::HEADER {
        let tag = address::header(part.key);
        assert_eq!(address::header_part(&tag), Some(part.key));
        assert_eq!(address::detail_part(&tag), None);
        assert_eq!(address::list_part(&tag), None);
        checked += 1;
    }
    for part in spec::DETAIL {
        let tag = address::detail(part.key);
        assert_eq!(address::detail_part(&tag), Some(part.key));
        assert_eq!(address::header_part(&tag), None);
        checked += 1;
    }
    for column in spec::COLUMNS {
        let tag = address::column(column.key);
        assert_eq!(address::column_key(&tag), Some(column.key));
        assert_eq!(address::list_part(&tag), None);
        checked += 1;
    }
    for choice in spec::CHOICES {
        let tag = address::severity(choice.key);
        assert_eq!(address::severity_key(&tag), Some(choice.key));
        assert_eq!(address::header_part(&tag), None);
        checked += 1;
    }
    assert!(
        checked >= 18,
        "{checked} part(s) were round-tripped, and a specification whose tables \
         had emptied would pass every assertion above by describing nothing"
    );
    // ★★★ A surface's own tag is not one of its parts — container and content.
    assert_eq!(address::header_part(address::HEADER), None);
    assert_eq!(address::list_part(address::LIST), None);
    assert_eq!(address::detail_part(address::DETAIL), None);
    // ★★★★ The filter FIELD is deliberately not under the header part it sits
    // in, so a reader composing the address they expected finds nothing. Pinned
    // rather than left as prose — R2123 did the same for that section's
    // endpoints, and for the same reason.
    assert!(!address::QUERY.starts_with(address::HEADER_SEAT));
    assert_eq!(address::header_part(address::QUERY), None);
    // ★★★★★ Everything this module composes is in the namespace.
    for (_, _, tag) in address::parts() {
        assert!(
            address::ours(&tag),
            "`{tag}` is not in this screen's namespace"
        );
    }
}

/// ★★★★★ R2124 — **a deeper seat tells this stem's four heads apart.**
///
/// `lv.list.` carries a PART and three INDEXED families. R2118 needed a
/// classifier for the node lab's wire family because both its heads survive
/// `strip_prefix`; here each head is a word in the segment after the stem, so a
/// seat one segment deeper separates them and the inverses are disjoint by
/// construction. Which of the two shapes a stem has is the question R2123 says
/// to ask before writing any gate, and this test is what says the answer rather
/// than leaving a reader to re-derive it.
///
/// 🟥 It also pins the JOIN. A cell is `<row>_<column>` where most joined keys
/// in this tree join on the separator, and three sites did that split by hand
/// before this round.
#[test]
fn r2124_a_deeper_seat_tells_this_stems_four_heads_apart() {
    // ★ Every indexed seat is UNDER the list's seat, which is what makes the
    // separation a fact about the addresses rather than about the reader.
    for seat in [address::ROW_SEAT, address::CELL_SEAT, address::DOT_SEAT] {
        assert!(
            seat.starts_with(address::LIST_SEAT) && seat.len() > address::LIST_SEAT.len(),
            "`{seat}` is meant to be one segment deeper than the list's parts"
        );
    }
    // ★★ A row, a cell and a dot are NOT list parts, and the list's parts are
    // none of those three — in both directions, over the real population.
    for part in ["header", "body", "open"] {
        let tag = address::list(part);
        assert_eq!(address::list_part(&tag), Some(part));
        assert_eq!(address::row_index(&tag), None);
        assert_eq!(address::cell_of(&tag), None);
        assert_eq!(address::dot_index(&tag), None);
    }
    let mut cells = 0;
    for n in 0..spec::ROWS.len() {
        let row = address::row(n);
        assert_eq!(address::row_index(&row), Some(n));
        assert_eq!(
            address::list_part(&row),
            None,
            "★ `{row}` is a row and the list's PART inverse claimed it — a \
             reader handed `row` as a part key looks it up in the list's parts \
             and finds nothing, quietly"
        );
        assert_eq!(address::cell_of(&row), None);
        let dot = address::dot(n);
        assert_eq!(address::dot_index(&dot), Some(n));
        assert_eq!(address::row_index(&dot), None);
        for column in spec::COLUMNS {
            let cell = address::cell(n, column.key);
            assert_eq!(
                address::cell_of(&cell),
                Some((n, column.key)),
                "★★ `{cell}` did not round-trip — the JOIN is an underscore \
                 here, where most of this tree joins on the separator"
            );
            assert_eq!(
                address::row_index(&cell),
                None,
                "★★★ `{cell}` is a cell and the row inverse claimed it — a \
                 press on it would then open whatever that spelling parsed as"
            );
            cells += 1;
        }
    }
    assert!(
        cells >= 40,
        "{cells} cell(s) were round-tripped, and an empty grid would satisfy \
         every assertion above by describing nothing"
    );
    // ★★★★ The join is the declaration's, and it is NOT the separator. Without
    // this the pair above would pass with both spellings changed together.
    assert_ne!(address::CELL_JOIN, '.');
    assert!(address::cell(3, "time").contains(address::CELL_JOIN));
    // ★★★★★ A part's CHILD is not that part.
    let kind = address::detail("kind");
    assert_eq!(address::detail_part(&address::child(&kind, "pill")), None);
}

/// ★★★★★ R2124 — **the screen HANDS the walk every address it used to spell.**
///
/// # Why this is a gate and not a convenience
///
/// This section's population is three, and the walk half cannot be asserted from
/// Rust at all: a walk is Python and no Rust gate reads it. What CAN be asserted
/// is the thing the walk depends on — that the wire carries an address for every
/// roster row and every seat, so a walk never has to compose one. If this
/// regressed, the walk would fall back to spelling and nothing would say so;
/// `published_tags` in `tools/rpc_verify.py` refuses a row with no `tag` for the
/// same reason, from the other side.
#[test]
fn r2124_the_wire_hands_over_every_address_a_walk_would_spell() {
    let published = super::spec_json();
    let addresses = &published["addresses"];
    assert_eq!(addresses["namespace"], address::NAMESPACE);
    assert_eq!(addresses["query"], address::QUERY);
    assert_eq!(addresses["cell_join"], address::CELL_JOIN.to_string());
    for (name, seat) in [
        ("header", address::HEADER_SEAT),
        ("list", address::LIST_SEAT),
        ("detail", address::DETAIL_SEAT),
        ("column", address::COLUMN_SEAT),
        ("severity", address::SEVERITY_SEAT),
        ("row", address::ROW_SEAT),
        ("cell", address::CELL_SEAT),
        ("dot", address::DOT_SEAT),
    ] {
        assert_eq!(
            addresses["seats"][name], seat,
            "the wire's `{name}` seat is not the one the declaration composes"
        );
    }
    // ★ Every roster row carries the address its key names — the half R2116
    // measured as missing, here with the reader in another language.
    let mut rows = 0;
    for (roster, table) in [
        (
            "columns",
            spec::COLUMNS.iter().map(|c| c.key).collect::<Vec<_>>(),
        ),
        ("detail", spec::DETAIL.iter().map(|p| p.key).collect()),
        ("header", spec::HEADER.iter().map(|p| p.key).collect()),
        ("severities", spec::CHOICES.iter().map(|c| c.key).collect()),
    ] {
        let published_rows = published[roster]
            .as_array()
            .unwrap_or_else(|| panic!("the wire publishes a `{roster}` roster"));
        assert_eq!(published_rows.len(), table.len());
        for row in published_rows {
            let key = row["key"].as_str().expect("every row names its key");
            let tag = row["tag"]
                .as_str()
                .unwrap_or_else(|| panic!("`{roster}.{key}` carries no address"));
            assert!(
                address::ours(tag) && tag.ends_with(key),
                "`{roster}.{key}` publishes `{tag}`, which is not this screen's \
                 address for that key"
            );
            rows += 1;
        }
    }
    assert_eq!(
        rows,
        spec::COLUMNS.len() + spec::DETAIL.len() + spec::HEADER.len() + spec::CHOICES.len(),
        "a roster that had emptied would satisfy every assertion above"
    );
}
