//! R1663 — the model tests: the screen's own state, checked against
//! `crate::spec`.
//!
//! The *painted* screen is `painted.rs` next door, and the split is the R1653
//! lesson: a test that asks the geometry helper where a control is gets the
//! helper's answer, which has been right every time the painter was wrong. So
//! nothing here is allowed to stand in for that module — these are the checks
//! about the model that no amount of rendering would settle.

use pinion_core::marks::Mark;
use pinion_core::reactive::Owner;
use pinion_core::widgets::field_bytes::{Coverage, FieldSpan, SourceId};

use super::{
    NAME_COLUMN, PacketView, capture_filler, cell_texts, char_count, comma, decode, frame_bytes,
    lane_reading, list_cell_tag, pane_cursor, row_cells, select_byte, select_field, select_message,
    sibling_place, spec, use_view_state,
};
use pinion_a11y::WidgetA11y;
use pinion_core::WidgetCore;

/// R1707 — the query box at rest; see the peer in `painted.rs`.
const IDLE_FIELD: (pinion_core::widgets::text_field::TextFieldState, u32) =
    (pinion_core::widgets::text_field::TextFieldState::Idle, 0);

/// Run `body` inside a scope with the screen's state.
fn with_state(body: impl FnOnce(&std::rc::Rc<super::ViewState>)) {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        body(&state);
    });
}

/// The reference's decode table is a well-formed dissection, checked by
/// building it rather than by reading it.
///
/// ★ The specification is a table a person typed. `ByteMap::build` is what says
/// whether the table is a *dissection* — a field escaping its layer, two fields
/// sharing a byte, a run past the end of the frame are all things a table can
/// say and a decode cannot mean.
#[test]
fn r1663_the_reference_decode_table_is_a_well_formed_dissection() {
    let map = decode(spec::OPENING_ROW);
    assert_eq!(map.fields().len(), spec::FIELDS.len());
    assert_eq!(map.sources().len(), spec::SOURCES.len());
    for field in spec::FIELDS {
        match field.source {
            Some(source) => {
                let (got_source, extent) = map
                    .extent_of(field.path)
                    .unwrap_or_else(|| panic!("`{}` should have bytes", field.path));
                assert_eq!(got_source, SourceId::new(source), "{}", field.path);
                assert_eq!(
                    (extent.at(), extent.len()),
                    (field.at, field.len),
                    "{}",
                    field.path
                );
            }
            None => assert!(
                map.extent_of(field.path).is_none(),
                "`{}` is declared derived",
                field.path
            ),
        }
    }
}

/// Every layer the specification names is a field of the decode, and every
/// field belongs to one of them.
#[test]
fn r1663_every_field_belongs_to_a_declared_layer() {
    let map = decode(spec::OPENING_ROW);
    for (id, _) in spec::LAYERS {
        assert!(map.field(id).is_some(), "layer `{id}` is not a field");
    }
    for span in map.fields() {
        let path = span.path();
        assert!(
            spec::LAYERS.iter().any(|(id, _)| path.starts_with(id)),
            "`{path}` belongs to no declared layer"
        );
    }
}

/// The screen opens where the reference's screen opens.
#[test]
fn r1663_the_screen_opens_on_the_message_and_field_the_reference_shows() {
    with_state(|state| {
        assert_eq!(state.row.get(), spec::OPENING_ROW);
        assert_eq!(state.field.get(), spec::OPENING_FIELD);
        let (source, extent) = state
            .map
            .map()
            .extent_of(spec::OPENING_FIELD)
            .expect("the opening field has bytes");
        assert_eq!(source, SourceId::new(0));
        assert_eq!(extent.len(), 2, "the reference lights two bytes for it");
    });
}

/// ★ The round's law at the model level: selecting a field and pressing one of
/// its bytes are the same derivation, so they cannot answer differently.
#[test]
fn r1663_selecting_a_field_and_pressing_its_bytes_agree() {
    with_state(|state| {
        let map = state.map.map();
        let mut checked = 0;
        for span in map.fields() {
            let Ok((source, selection)) = map.selection_for(span.path()) else {
                continue;
            };
            if source != SourceId::new(0) {
                continue;
            }
            select_field(state, span.path());
            assert_eq!(state.lit_selection(), Some(selection), "{}", span.path());
            select_byte(state, selection.focus());
            let back = state.field.get();
            assert!(
                back == span.path() || back.starts_with(&format!("{}.", span.path())),
                "pressing a byte of `{}` selected `{back}`",
                span.path()
            );
            checked += 1;
        }
        assert!(checked >= 15, "only {checked} field(s) were checked");
    });
}

/// A byte in the frame that no field claims answers `unmapped`, and one past
/// the frame answers `out-of-buffer` — three answers, not two.
#[test]
fn r1663_the_frame_distinguishes_unclaimed_from_absent() {
    with_state(|state| {
        let map = state.map.map();
        let frame = SourceId::new(0);
        let last = spec::SOURCES[0].1;
        let unclaimed = (0..last)
            .find(|b| matches!(map.coverage_at(frame, *b), Coverage::Unmapped))
            .expect("the reference frame has bytes past its last field");
        assert!(unclaimed < last);
        assert_eq!(map.coverage_at(frame, last).as_str(), "out-of-buffer");
    });
}

/// Selecting a message the decode has no such field in falls back rather than
/// leaving the screen with a selection that resolves to nothing.
#[test]
fn r1663_a_message_without_the_selected_field_falls_back_to_a_layer() {
    with_state(|state| {
        select_field(state, "l3.payload");
        select_message(state, 1);
        assert!(
            state.map.map().field(&state.field.get()).is_some(),
            "the selection must name a field of the new decode, got {:?}",
            state.field.get()
        );
    });
}

/// Every message decodes into something, and the reassembled one is the only
/// message with a second byte source.
#[test]
fn r1663_every_message_decodes_and_only_one_has_a_second_source() {
    let mut with_two = Vec::new();
    for n in 0..spec::ROWS.len() {
        let map = decode(n);
        assert!(!map.fields().is_empty(), "message {n} decoded to nothing");
        if map.sources().len() > 1 {
            with_two.push(n);
        }
    }
    assert_eq!(
        with_two,
        vec![spec::OPENING_ROW],
        "only the reassembled message has a payload in another buffer"
    );
}

/// The marks the byte pane paints with come out of the map, so the ink and the
/// model are one fact.
#[test]
fn r1663_the_byte_panes_marks_are_the_maps() {
    with_state(|state| {
        let map = state.map.map();
        let marks = map.marks(SourceId::new(0));
        let painted: Vec<&str> = marks.iter().map(Mark::name).collect();
        let in_frame: Vec<&str> = map
            .fields()
            .iter()
            .filter(|s| {
                s.origin()
                    .source()
                    .is_some_and(|src| src == SourceId::new(0))
            })
            .map(FieldSpan::path)
            .collect();
        assert_eq!(painted.len(), in_frame.len());
        for name in &in_frame {
            assert!(painted.contains(name), "`{name}` is not marked");
        }
        // The mark on top of a byte is the field that owns it.
        for byte in 0..spec::SOURCES[0].1 {
            assert_eq!(
                marks.top_at(byte).map(Mark::name),
                map.owner_at(SourceId::new(0), byte).map(FieldSpan::path),
                "byte {byte}"
            );
        }
    });
}

/// The frame bytes are deterministic — the dump must be the same on every host
/// and in every run, or a screenshot gate is a coin toss.
#[test]
fn r1663_the_frame_bytes_are_the_same_every_time() {
    let once = frame_bytes(spec::OPENING_ROW);
    assert_eq!(once.len(), spec::SOURCES[0].1);
    assert_eq!(once, frame_bytes(spec::OPENING_ROW));
    assert_ne!(
        once,
        frame_bytes(spec::OPENING_ROW + 1),
        "two messages must not dump the same bytes"
    );
}

/// How many framed fields state what their bytes encode — a **ratchet**, and
/// it may only ever go up.
///
/// ★★★★★ R1814 — stated as what the table HAS rather than as a ceiling on what
/// it lacks, which is R1813's lesson applied one round later on a different
/// screen: a complement is not monotone when the work can widen the population,
/// and adding a field to `spec::FIELDS` widens this one. Raising it means
/// declaring an encoding the reference's own printed value determines. It is
/// *not* raised by inventing a code for `reliable` or `drop` — see
/// `spec::Wire`, and see the second assertion below, which is what would catch
/// that.
///
/// ⚠ **Its ceiling is not the number of framed fields, and never will be.** A
/// layer heading must never declare an encoding at all — its extent is its
/// children's — and a leaf whose printed value does not determine a wire value
/// cannot be declared without inventing one. The size of each group is printed
/// by the test below rather than written here, because the first draft of this
/// paragraph got both numbers wrong in the round that measured them.
const DECLARED_ENCODINGS: usize = 6;

/// Which fields of the described message state what their bytes hold, and
/// whether the painted frame reads back as that — the **third direction**.
///
/// ★★★★★ R1814 — the direction this screen was built for and could not answer.
/// R1663 made the field-to-bytes relation true and tested in both directions;
/// the closing audit then measured that the bytes themselves were a hash of the
/// row index, so `sn` lit two bytes reading 6172 beside a tree that said 3419,
/// and the reference draws `0d 5b` there.
///
/// The assertion that carries the weight is the SECOND one. Reading the bytes
/// back and comparing them to the declaration only proves the writer agrees
/// with itself; a declaration fitted to whatever bytes were already there would
/// pass it. So every declared value must also be a value **the screen prints**
/// — a whole number token in the reference's own text, or the text itself.
#[test]
fn r1814_a_declared_field_encodes_the_value_the_screen_shows() {
    let bytes = frame_bytes(spec::OPENING_ROW);
    let mut declared = 0usize;
    let mut covered = 0usize;
    for field in spec::FIELDS {
        if field.source != Some(0) || !field.wire.is_declared() {
            continue;
        }
        declared += 1;
        covered += field.len;

        let slice = bytes
            .get(field.at..field.at + field.len)
            .unwrap_or_else(|| panic!("`{}` is declared outside the frame", field.path));
        assert!(
            field.wire.reads(slice),
            "`{}` declares {:?} and the painted frame holds {:02x?} at {:#04x}..{:#04x}",
            field.path,
            field.wire,
            slice,
            field.at,
            field.at + field.len
        );

        // ★★★★★ The anti-decoration half: the declared value has to be one the
        // reader can see, or the round trip above is the writer checking the
        // writer. This is the check that refuses a code invented to fit bytes.
        let shown = match field.wire {
            spec::Wire::Be(n) => number_tokens(field.value).contains(&n),
            spec::Wire::Ascii(text) => field.value.contains(text),
            spec::Wire::Flag(set) => field.value == if set { "true" } else { "false" },
            spec::Wire::Undeclared(_) => unreachable!("filtered above"),
        };
        assert!(
            shown,
            "`{}` declares {:?}, which is not a value the screen shows — it prints {:?}",
            field.path, field.wire, field.value
        );
    }

    assert!(
        declared >= DECLARED_ENCODINGS,
        "only {declared} framed field(s) state what their bytes encode, down \
         from the {DECLARED_ENCODINGS} this table had when the ratchet was set"
    );

    // ★★★★★ The census, printed rather than written down anywhere. R1814's
    // closing audit found this round's own prose claiming `nine of fifteen`
    // where the table says twelve of eighteen, which is what a hand-written
    // count beside a list always eventually says.
    let framed = spec::FIELDS.iter().filter(|f| f.source == Some(0)).count();
    let headings = spec::FIELDS
        .iter()
        .filter(|f| f.source == Some(0) && spans_another(f))
        .count();
    let leaves_undeclared = framed - declared - headings;
    println!(
        "R1814 framed={framed} declared={declared} headings={headings} \
         undeclared_leaves={leaves_undeclared} covered={covered}B of {}B frame",
        spec::SOURCES[0].1
    );
}

/// Whether another framed field's extent lies inside this one's — what makes a
/// row a layer heading rather than a leaf, derived from the table instead of
/// from the path's shape.
///
/// ★ Not `path.contains('.')`: that reads the NAME to answer a question about
/// EXTENTS, and the two agree here only by convention. A table that nested a
/// field one level deeper would break the name rule silently and this one not
/// at all.
fn spans_another(field: &spec::FieldSpecRow) -> bool {
    spec::FIELDS.iter().any(|other| {
        other.source == Some(0)
            && !std::ptr::eq(field, other)
            && other.len > 0
            && other.at >= field.at
            && other.at + other.len <= field.at + field.len
    })
}

/// Every whole number the text prints, commas removed.
///
/// ★ A *token* rather than a substring, which is the whole strength of the
/// check it serves: `4` is a substring of `3,419`, so a substring test would
/// let a wrong declaration pass on a table where two fields hold 4.
fn number_tokens(text: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut digits = String::new();
    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if ch == ',' && !digits.is_empty() {
            // A thousands separator continues the number; a comma anywhere
            // else ends it, and an empty run means it was not a separator.
        } else {
            if let Ok(n) = digits.parse::<u64>() {
                out.push(n);
            }
            digits.clear();
        }
    }
    out
}

/// A declared field must not span another, because the frame is written field
/// by field and a layer heading's extent is its children's.
///
/// ★ Asserted rather than relied on: `frame_bytes` writes in table order, so
/// with a declaration on `l1` the transport layer's four leaves would be
/// silently overwritten or silently overwrite it, depending only on where the
/// row sat in the list. That is a defect no amount of reading catches and one
/// assertion makes impossible.
#[test]
fn r1814_no_declared_field_spans_another() {
    for field in spec::FIELDS {
        if field.source != Some(0) || !field.wire.is_declared() {
            continue;
        }
        for other in spec::FIELDS {
            if other.source != Some(0) || std::ptr::eq(field, other) || other.len == 0 {
                continue;
            }
            let inside = other.at >= field.at && other.at + other.len <= field.at + field.len;
            assert!(
                !inside,
                "`{}` declares an encoding over {:#04x}..{:#04x}, which contains `{}`",
                field.path,
                field.at,
                field.at + field.len,
                other.path
            );
        }
    }
}

/// A field that declares nothing says **why**, and its bytes stay the capture.
///
/// ★★★★★ R1814 — the debt this round repays asked for exactly one of two
/// things: encode the values, or *state in the screen's own documentation why
/// not*. Nine fields take the second branch, and putting the reason in the
/// specification rather than in a doc comment is what makes it checkable: an
/// empty reason is a silence wearing a declaration's clothes.
#[test]
fn r1814_an_undeclared_field_says_why_and_keeps_the_capture() {
    let painted = frame_bytes(spec::OPENING_ROW);
    let filler = capture_filler(spec::OPENING_ROW, spec::SOURCES[0].1);
    let mut undeclared = 0usize;
    for field in spec::FIELDS {
        let spec::Wire::Undeclared(reason) = field.wire else {
            continue;
        };
        undeclared += 1;
        assert!(
            reason.len() > 12,
            "`{}` declares no encoding and gives no reason worth reading: {reason:?}",
            field.path
        );
        // A leaf that declares nothing must be untouched capture. Headings are
        // skipped: their extent holds their children, which ARE written.
        if field.source == Some(0) && field.len > 0 && !spans_another(field) {
            assert_eq!(
                painted.get(field.at..field.at + field.len),
                filler.get(field.at..field.at + field.len),
                "`{}` declares nothing, so its bytes must be the capture and \
                 not something that looks like a decode",
                field.path
            );
        }
    }
    assert!(
        undeclared > 0,
        "a table in which everything is declared would make the reason field \
         dead, and this test vacuous"
    );
}

/// The reference's own illustration, pinned as a number rather than a rule.
///
/// ★★★★★ R1814 — the debt names this exact case: the reference draws `0d 5b`
/// where `sn` is lit, and this tree drew `18 1c`. A rule-level test can pass
/// while the one byte pair a reader was told to look at is still wrong, so the
/// pair is asserted directly.
#[test]
fn r1814_the_opening_field_paints_the_bytes_the_reference_draws() {
    let field = spec::FIELDS
        .iter()
        .find(|f| f.path == spec::OPENING_FIELD)
        .expect("the field the screen opens on is in the table");
    let bytes = frame_bytes(spec::OPENING_ROW);
    assert_eq!(
        &bytes[field.at..field.at + field.len],
        &[0x0d, 0x5b],
        "the field the screen opens with selected must light the bytes that \
         encode the number the tree prints beside it"
    );
    assert_eq!(field.value, "3419", "and 0x0d5b is that number");
}

/// ★★★★★ A layer heading OPENS when it is pressed, and the two channels agree.
///
/// R1747 measured the defect by driving the running screen: pressing the middle
/// of `pv.tree.field.l0` flipped `folded` and left `selected_field` alone, while
/// `invoke select_field "l0"` on the same row opened it and lit twelve bytes. A
/// screen whose pointer and whose wire disagree about what one row does is one
/// this tree has closed under several names on its sibling screens.
///
/// The behaviour canon settles which of the two is right: its capture section
/// puts selection on EVERY row of the decode tree, depth-0 rows included, and
/// has no fold at all. Folding is this screen's own second-pass addition — kept,
/// by the standing rule that what the canon lacks and we have is not removed —
/// but it may not be paid for with the canon's own gesture, so it moves onto the
/// chevron that draws it.
#[test]
fn r1815_a_layer_heading_opens_by_tag_and_folds_by_its_chevron() {
    with_state(|state| {
        let layer = spec::LAYERS[0].0;
        assert_ne!(state.field.get(), layer, "the screen opens on a leaf");

        // The row's own tag selects, which is the canon's contract and was the
        // wire's answer all along.
        assert!(
            super::act_on_hit(
                state,
                super::Hit::of_tag(state, &format!("pv.tree.field.{layer}"))
            ),
            "the layer row must answer a press"
        );
        assert_eq!(
            state.field.get(),
            layer,
            "pressing a layer heading opens it"
        );
        assert!(
            !state.folded.get()[0],
            "and opening it must not fold it — that is the defect this repairs"
        );
        assert!(
            state.lit_selection().is_some(),
            "an opened layer lights its bytes, which is what selecting it is FOR"
        );

        // ★★★★★ THE THIRD CHANNEL, which is the one that was right all along and
        // the one nothing compared. `r1699_every_cursor_member_resolves_to_the
        // _hit_its_tag_names` asserts `Hit::of_tag == Hit::at` and stayed green
        // through this entire defect, because BOTH pointer channels answered
        // `Layer` — an agreement gate is blind to an error its two sides share.
        // The wire reaches `select_field` directly, never through `Hit`, so it
        // was outside that comparison; R1747 found the defect by driving the
        // running screen and noticing the wire and the pointer disagreeing.
        let by_tag = state.field.get();
        select_field(state, layer);
        assert_eq!(
            state.field.get(),
            by_tag,
            "pressing the row and invoking `select_field` on it are one act"
        );

        // The chevron's tag folds, and touches the selection not at all.
        assert!(
            super::act_on_hit(
                state,
                super::Hit::of_tag(state, &format!("pv.tree.layer.{layer}"))
            ),
            "the chevron must answer a press — it had a tag and no arm until R1815"
        );
        assert!(state.folded.get()[0], "the chevron folds its layer");
        assert_eq!(
            state.field.get(),
            layer,
            "and folding does not move the selection"
        );
    });
}

/// ★★★★★ The keyboard keeps the fold the pointer repair moved off its row.
///
/// This is not a feature beside the repair — without it the repair REMOVES a
/// capability. `Enter` on a layer heading used to fold it, because the whole row
/// answered `Hit::Layer`; once the row selects, the chevron owns the fold, and
/// the chevron is declared part of its tree item rather than a stop of its own.
/// That is the right ARIA shape — a tree item owns its expansion — and it is
/// precisely why the ARROWS are where expansion has to live.
///
/// The screen was already announcing `aria-expanded` on its four layer items
/// while no key on any keyboard could change it.
#[test]
fn r1815_the_arrows_expand_and_collapse_what_the_item_announces() {
    with_state(|state| {
        let layer = spec::LAYERS[0].0;
        select_field(state, layer);
        assert!(!state.folded.get()[0], "it starts open");

        assert!(
            super::key_at(state, Some("pv.tree"), "ArrowLeft"),
            "ArrowLeft on an open layer collapses it"
        );
        assert!(state.folded.get()[0]);
        assert!(
            super::key_at(state, Some("pv.tree"), "ArrowRight"),
            "ArrowRight on a collapsed layer expands it"
        );
        assert!(!state.folded.get()[0]);

        // ★★★★★ WHAT `Enter` DOES, MEASURED — and this assertion exists because
        // reading the code gave the wrong answer TWICE, in both directions.
        //
        // The tree's roving cursor declares `Activation::Follows`, so the cursor
        // IS the selection: an arrow moves it and `seat_pane_cursor` calls
        // `select_field`. There is nothing left for `Enter` to activate, so the
        // roving consumes no such chord, and the fallback arm asks
        // `Hit::of_tag` about the focused stop — which is the PANE tag
        // `pv.tree`, never the row's — and gets `Hit::None`.
        //
        // ⇒ the keyboard could already SELECT a layer heading, by walking onto
        // it. What no key could do was FOLD one, which is why the arrows are the
        // repair and why the pointer was the channel that was broken.
        select_field(state, layer);
        assert!(
            !super::key_at(state, Some("pv.tree"), "Enter"),
            "a `Follows` tree has nothing for Enter to activate, so it reports \
             that it did nothing rather than pretending"
        );
        assert_eq!(state.field.get(), layer, "and it moved nothing");

        // A chord that would only navigate falls through rather than pretending.
        assert!(
            !super::key_at(state, Some("pv.tree"), "ArrowRight"),
            "ArrowRight on an ALREADY open layer is the ARIA move-to-first-child \
             case, which is not built — it must report that it did nothing"
        );
        select_field(state, "l1.sn");
        assert!(
            !super::key_at(state, Some("pv.tree"), "ArrowLeft"),
            "and a leaf row has nothing to collapse"
        );
    });
}

/// A count is printed the way the reference prints one.
#[test]
fn r1663_counts_carry_thousands_separators() {
    assert_eq!(comma(spec::MATCHED), "12,418");
    assert_eq!(comma(spec::CAPTURED), "184,392");
    assert_eq!(comma(0), "0");
    assert_eq!(comma(999), "999");
    assert_eq!(comma(1_000), "1,000");
}

/// ★★★★★ R1721 — **at most one saved filter is on**, which is what this bar
/// declares and is now the reason it is announced as a `listbox`.
///
/// The name of the test next door — "the switches are independent" — was true of
/// the layer folds and false of the saved filters for the whole time it stood
/// there: `toggle_saved` cleared the vector by hand and nothing asserted it, so
/// the rule lived in one function while three readers announced another. Pinning
/// the BEHAVIOUR here is what keeps the declaration honest: change
/// `spec::SAVED_ROW` and this fails, rather than the census quietly agreeing with
/// whatever the rule became.
#[test]
fn r1721_at_most_one_saved_filter_is_on() {
    with_state(|state| {
        super::toggle_saved(state, 1);
        assert_eq!(state.saved.get(), vec![false, true, false]);
        super::toggle_saved(state, 2);
        assert_eq!(
            state.saved.get(),
            vec![false, false, true],
            "choosing a second saved filter REPLACES the first"
        );
        super::toggle_saved(state, 2);
        assert_eq!(
            state.saved.get(),
            vec![false, false, false],
            "and choosing the one that is on empties the row"
        );
        assert_eq!(
            super::saved_row(state).choice(),
            pinion_core::widgets::chip_group::Choice::AtMostOne,
            "the behaviour above IS the declared rule, not a coincidence"
        );
    });
}

/// ★★★★★ R1721 — **the accessibility tree reports the saved filter that is on.**
///
/// Found by a counterfactual that PASSED: replacing the live row with an all-off
/// one in `filter_nodes` — so the tree announced "no saved filter applied" while
/// the bar painted one lit — was caught by nothing in this crate's suite. The
/// integration demo caught it, and a gate that lives one process away from the
/// defect is the R1712 / R1719 class: the layer the defect is in had no test.
#[test]
fn r1721_the_tree_reports_the_saved_filter_that_is_on() {
    with_state(|state| {
        let selected = |state: &std::rc::Rc<super::ViewState>| -> Vec<bool> {
            super::filter_nodes(state)
                .into_iter()
                .filter(|node| node.tag.starts_with("pv.filter.saved."))
                .map(|node| node.selected == Some(true))
                .collect()
        };
        assert_eq!(
            selected(state),
            vec![false, false, false],
            "the bar opens with nothing applied"
        );
        super::toggle_saved(state, 1);
        assert_eq!(
            selected(state),
            vec![false, true, false],
            "★ the option the row has chosen is the one announced as selected"
        );
        super::toggle_saved(state, 2);
        assert_eq!(
            selected(state),
            vec![false, false, true],
            "★ and it MOVES with the choice rather than being read once"
        );
    });
}

/// The saved filters and the layer folds are separate, and the wire and a press
/// reach the same ones.
#[test]
fn r1663_the_switches_are_independent() {
    with_state(|state| {
        super::toggle_saved(state, 0);
        super::toggle_layer(state, 1);
        assert_eq!(state.saved.get(), vec![true, false, false]);
        assert_eq!(state.folded.get(), vec![false, true, false, false]);
        super::toggle_saved(state, 0);
        assert_eq!(state.saved.get(), vec![false, false, false]);
        assert_eq!(
            state.folded.get(),
            vec![false, true, false, false],
            "clearing a filter must not unfold a layer"
        );
    });
}

/// A folded layer hides its children and nothing else.
#[test]
fn r1663_folding_a_layer_hides_its_children_only() {
    with_state(|state| {
        let before = super::visible_fields(state).len();
        super::toggle_layer(state, 1);
        let after: Vec<String> = super::visible_fields(state)
            .into_iter()
            .map(|(p, ..)| p)
            .collect();
        assert!(after.len() < before, "folding hid nothing");
        assert!(after.contains(&"l1".to_owned()), "the heading stays");
        assert!(
            !after.iter().any(|p| p.starts_with("l1.")),
            "a child of the folded layer is still shown: {after:?}"
        );
        assert!(
            after.iter().any(|p| p.starts_with("l3.")),
            "another layer's children were hidden too"
        );
    });
}

// ── R1693: what a reader is told, computed ─────────────────────────────────

/// ★★★ A cell announces what is painted in it, and the name column announces
/// **all three** of its runs.
///
/// The paint and the accessibility layer share [`cell_texts`] so the six plain
/// columns cannot drift; the seventh is the one place they differ, and this is
/// where that difference is stated as a fact rather than as a comment. A reader
/// told only `sensors/unit-1/depth` would never learn the message was
/// reassembled from three pieces — or, on the dropped row, that a piece is gone.
#[test]
fn r1693_the_name_cell_announces_the_annotations_painted_beside_it() {
    for (n, message) in spec::ROWS.iter().enumerate() {
        let painted = cell_texts(message);
        let announced = row_cells(message);
        assert_eq!(
            painted.len(),
            spec::COLUMNS.len(),
            "row {n} paints one run per column",
        );
        assert_eq!(announced.len(), painted.len());
        for c in 0..spec::COLUMNS.len() {
            if c == NAME_COLUMN {
                continue;
            }
            assert_eq!(
                painted[c], announced[c],
                "row {n} column {c} announces something other than what it paints",
            );
        }
        assert!(
            announced[NAME_COLUMN].starts_with(message.name),
            "row {n}'s name cell opens with the resource name",
        );
        if !message.note.is_empty() {
            assert!(
                announced[NAME_COLUMN].contains(message.note),
                "row {n} paints {:?} in the name column and does not announce it",
                message.note,
            );
        }
        if let Some(fragment) = &message.fragment {
            assert!(
                announced[NAME_COLUMN].contains(fragment.marker),
                "row {n} is one piece of a larger message and does not say so",
            );
        }
    }
    // ★ And the discriminating row: the reassembled one carries BOTH, which is
    // what makes the two branches above more than a pair of no-ops.
    let both = &spec::ROWS[spec::OPENING_ROW];
    assert!(both.fragment.is_some() && !both.note.is_empty());
}

/// ★★★ `aria-posinset` counts within a **level**, not within the flattened list
/// the tree is painted as: a field announced "3 of 24" when it is the third of
/// four under its layer tells a reader the wrong shape.
#[test]
fn r1693_a_tree_items_position_is_among_its_own_siblings() {
    with_state(|state| {
        let visible = super::visible_fields(state);
        let layers = spec::LAYERS.len();
        // The opening screen shows every layer expanded, so the top level is
        // the four layers and each field sits under the one it belongs to.
        for (n, (path, _, _, depth)) in visible.iter().enumerate() {
            let (position, siblings) = sibling_place(&visible, n);
            assert!(position < siblings, "{path} is {position} of {siblings}");
            if *depth == 0 {
                assert_eq!(siblings, layers, "{path} is a layer among the layers");
            } else {
                assert!(
                    siblings < visible.len(),
                    "{path} counts within the whole tree instead of its layer",
                );
                let owner = path.split('.').next().unwrap_or(path);
                let under = visible
                    .iter()
                    .filter(|(p, _, _, d)| *d > 0 && p.starts_with(owner))
                    .count();
                assert_eq!(siblings, under, "{path} counts the wrong siblings");
            }
        }
        // The first field of the first layer is its first child, not the
        // second row of the tree.
        let first_child = visible
            .iter()
            .position(|(_, _, _, d)| *d > 0)
            .expect("a leaf");
        assert_eq!(sibling_place(&visible, first_child).0, 0);
    });
}

/// ★★★★★ R1693 — the arrow keys move the selection, driven through the chord a
/// **real key press** produces.
///
/// This screen matched `Down` and `Up`, which no keyboard sends: the shell
/// spells a named key the way the web platform does, and every other example in
/// this tree matches `ArrowDown`. So the keyboard navigation had never worked
/// from a keyboard, and nothing said so because every test that drove it used
/// the screen's own spelling — the test and the defect were the same mistake.
///
/// Asserted against `apply_key`, the shell's entry point, rather than against
/// `key` directly: a test that calls the private matcher can agree with it about
/// a spelling neither of them shares with the world.
#[test]
fn r1693_the_arrow_keys_a_keyboard_sends_move_the_selection() {
    use pinion_core::widget_core::WidgetCore;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        // ★ The STATE scene, which is what the shell hands `apply_key` — an
        // `External` carrying the widget's tag. The first draft passed the PAINT
        // scene, whose root is a container with the same tag, and `apply_key`
        // found no external and answered `false` for every chord: a test that
        // would have reported the repair as broken and the defect as fixed.
        let mut scene = pinion_core::Scene::External(
            pinion_core::scene::ExternalNode::new(super::PacketView::create_external())
                .with_tag(super::VIEW_TAG),
        );
        let press = |scene: &mut pinion_core::Scene, chord: &str| {
            super::PacketView::apply_key(scene, None, chord, pinion_core::Modifiers::empty())
        };

        let opening = state.row.get();
        assert!(press(&mut scene, "ArrowDown"), "ArrowDown was not handled");
        assert_eq!(state.row.get(), opening + 1, "the selection did not move");
        assert!(press(&mut scene, "ArrowUp"), "ArrowUp was not handled");
        assert_eq!(state.row.get(), opening, "and it did not come back");

        // ★ The spelling this screen used to match is not a chord anything
        // sends, so it must NOT be handled — accepting both would leave the
        // wrong one alive and the next screen would copy it.
        assert!(
            !press(&mut scene, "Down"),
            "`Down` is not a key a keyboard has"
        );
        assert_eq!(state.row.get(), opening);

        // Escape still resets the open field, so the whole keymap is covered.
        super::select_field(&state, "l3.payload");
        assert!(press(&mut scene, "Escape"));
        assert_eq!(state.field.get(), spec::LAYERS[0].0);
    });
}

/// ★★★★★ R1693 — the screen has a **keyboard ring**, and it had none.
///
/// It announced three `button` chips and three composite panes, and a keyboard
/// user could reach not one of them: `focus/next` answered `None` at every step.
/// That is the same defect as announcing a `table` with no rows, one axis over —
/// a role that promises something the screen cannot do — and it was invisible
/// until this round put an interactive role on the screen at all.
///
/// The ring is the WAI-ARIA composite pattern: one stop per composite, because
/// the arrows already move *within* the grid, plus one per plain button.
/// Asserted as an equality, not a floor: a stop nobody meant is as wrong as a
/// missing one, and a floor would let the next round add either.
#[test]
fn r1693_the_screen_is_a_keyboard_ring_of_its_composites_and_buttons() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_view_state();
        let scene = super::view(
            (pinion_core::widgets::text_field::TextFieldState::Idle, 0),
            pinion_core::Frame::default(),
        );
        let mut want: Vec<String> = vec![
            "pv.list".to_owned(),
            "pv.tree".to_owned(),
            "pv.bytes".to_owned(),
            // R1707 — the query box. A filter a person cannot Tab to is a
            // filter only a mouse has.
            "pv.filter.query".to_owned(),
        ];
        // ★★★★★ R1721 — the saved-filter bar's stops come from its RULE, and
        // this is what the derivation costs a keyboard: three stops became one,
        // with arrows, `Home`, `End` and `Enter` inside it. The list is not
        // written down here — `spec::SAVED_ROW` is, and a screen that changed the
        // rule without changing the ring would fail this rather than drift.
        want.extend(
            super::saved_row(&state)
                .stops()
                .into_iter()
                .map(str::to_owned),
        );
        let mut got = scene.collect_focusable_tags();
        got.sort();
        want.sort();
        assert_eq!(got, want, "the tab ring is not the composites and buttons");
    });
}

/// A lane's reading is one function, so the strip and the accessibility tree
/// cannot disagree about whether a channel's sequence is unbroken.
#[test]
fn r1693_a_lane_reads_the_same_to_both_of_its_readers() {
    for lane in spec::LANES {
        let said = lane_reading(lane);
        assert!(said.contains(&lane.sn.to_string()));
        assert_eq!(
            said.contains("unbroken"),
            lane.continuous,
            "{} says the wrong thing about its continuity",
            lane.name,
        );
        if !lane.continuous {
            assert!(said.contains(&lane.dropped.to_string()));
        }
    }
    // ★ Both arms are exercised: this capture has a broken lane, and a table of
    // only continuous ones would make half of the assertion above vacuous.
    assert!(spec::LANES.iter().any(|l| !l.continuous));
}

/// ★★ The compile-time character count the name column's floor is derived from.
///
/// It counts UTF-8 characters and not bytes, and every string it is applied to
/// today is ASCII — so the distinction is invisible in the floor it computes and
/// would stay invisible until the first row with a non-Latin name, at which
/// point the column would be sized for a string half again as long as it is.
#[test]
fn r1693_the_compile_time_character_count_counts_characters() {
    assert_eq!(char_count(""), 0);
    assert_eq!(char_count("sensors/unit-1/depth"), 20);
    assert_eq!(char_count("reassembled 3,144 B"), 19);
    assert_eq!(char_count("메시지"), 3, "three characters, nine bytes");
    assert_eq!(char_count("piece 1 of 3 · 안"), 16);
}

/// ★★★ Every family the voice specification names expands to the members the
/// screen actually has — including the four that are **conditional**.
///
/// A population that over-counts makes the gate demand regions the screen never
/// paints; one that under-counts makes it satisfiable by a screen missing them.
/// Both directions are wrong in the same way — the population is the claim.
#[test]
fn r1693_every_voice_population_expands_to_what_the_capture_holds() {
    use spec::Population;

    assert_eq!(Population::One.members().len(), 1);
    assert_eq!(Population::Rows.members().len(), spec::ROWS.len());
    assert_eq!(Population::Columns.members().len(), spec::COLUMNS.len());
    assert_eq!(
        Population::Cells.members().len(),
        spec::ROWS.len() * spec::COLUMNS.len(),
        "a product, which is the shape screen A's families do not have",
    );
    assert_eq!(Population::Bytes.members().len(), spec::SOURCES[0].1);
    assert_eq!(
        Population::ByteRows.members().len(),
        spec::SOURCES[0].1.div_ceil(spec::BYTES_PER_ROW),
    );
    assert_eq!(Population::Fields.members().len(), spec::FIELDS.len());

    // The conditional four, each counted against the table's own predicate.
    assert_eq!(
        Population::Annotated.members().len(),
        spec::ROWS.iter().filter(|r| !r.note.is_empty()).count(),
    );
    assert_eq!(
        Population::Fragmented.members().len(),
        spec::ROWS.iter().filter(|r| r.fragment.is_some()).count(),
    );
    assert_eq!(
        Population::Derived.members().len(),
        spec::FIELDS.iter().filter(|f| f.source.is_none()).count(),
    );
    // ★ And none of the four is the whole table, which is what makes them
    // conditional rather than a longer spelling of `Rows` / `Fields`.
    for narrow in [
        Population::Annotated,
        Population::Fragmented,
        Population::Derived,
        Population::LitBytes,
    ] {
        assert!(
            !narrow.members().is_empty(),
            "a family with no members would satisfy the gate by holding nothing",
        );
    }
    assert!(Population::Annotated.members().len() < spec::ROWS.len());
    assert!(Population::Derived.members().len() < spec::FIELDS.len());
    assert!(Population::LitBytes.members().len() < spec::SOURCES[0].1);

    // The lit bytes are exactly the extent of the field the screen opens on.
    let opening = spec::FIELDS
        .iter()
        .find(|f| f.path == spec::OPENING_FIELD)
        .expect("the opening field is in the table");
    assert_eq!(
        Population::LitBytes.members(),
        (opening.at..opening.at + opening.len)
            .map(|b| b.to_string())
            .collect::<Vec<_>>(),
    );
}

// ── R1698: the cursor inside each pane ───────────────────────────────────────

/// Press a key the way the SHELL does — through `WidgetCore::apply_key`, with
/// the focus manager's tag.
///
/// ★★★★★ Not through `key_at`, which is what the first draft of these gates
/// drove: a counterfactual dropping `focused` inside `apply_key` — the exact
/// state this screen was in before the round, where all six stops moved the
/// message list — left every one of them green, because none went through the
/// door a person's key comes in by.
fn press_key(focused: Option<&str>, chord: &str) -> bool {
    let mut scene = super::view(
        (pinion_core::widgets::text_field::TextFieldState::Idle, 0),
        pinion_core::Frame::default(),
    );
    PacketView::apply_key(
        &mut scene,
        focused,
        chord,
        pinion_core::input::Modifiers::default(),
    )
}

/// The three panes that own a keyboard cursor, and the key that advances each.
const PANE_CURSORS: [(&str, &str); 3] = [
    ("pv.list", "ArrowDown"),
    ("pv.tree", "ArrowDown"),
    ("pv.bytes", "ArrowRight"),
];

/// ★★★★★ R1698 — **each pane's arrows move that pane's cursor, and nobody
/// else's.**
///
/// Measured on this running screen the day the round started: at ALL SIX Tab
/// stops — the three saved-filter chips, the decode tree and the byte grid
/// included — `ArrowDown` moved the **message list**, because `apply_key`
/// dropped the `focused` argument the shell hands it. An arrow meant one thing
/// no matter where anybody was standing, which is the other half of the
/// composite pattern R1693 left open when it gave these panes their Tab stops.
#[test]
fn r1698_each_panes_arrows_move_that_panes_cursor() {
    for (stop, advance) in PANE_CURSORS {
        with_state(|state| {
            let before = pane_cursor(state, stop)
                .and_then(|r| r.cursor_tag().map(str::to_owned))
                .unwrap_or_else(|| panic!("{stop} has a cursor"));
            let others: Vec<(&str, Option<String>)> = PANE_CURSORS
                .iter()
                .filter(|(other, _)| *other != stop)
                .map(|(other, _)| {
                    (
                        *other,
                        pane_cursor(state, other).and_then(|r| r.cursor_tag().map(str::to_owned)),
                    )
                })
                .collect();

            assert!(press_key(Some(stop), advance), "{stop} took {advance}");
            let after = pane_cursor(state, stop).and_then(|r| r.cursor_tag().map(str::to_owned));
            assert_ne!(Some(before), after, "{stop}: {advance} moved its cursor");

            // ★ And nothing else moved. This is the assertion that fails on the
            // pre-R1698 screen, where every stop drove the message list.
            for (other, was) in others {
                // The byte grid's cursor is the selected field's first byte, so
                // moving the tree legitimately moves it. Every other pair is
                // independent.
                if stop == "pv.tree" && other == "pv.bytes" {
                    continue;
                }
                assert_eq!(
                    pane_cursor(state, other).and_then(|r| r.cursor_tag().map(str::to_owned)),
                    was,
                    "{stop}'s {advance} moved {other}'s cursor"
                );
            }
        });
    }
}

/// ★★★★★ R1698 — **a plain button owns no cursor, and an arrow there moves
/// nothing.**
///
/// The three saved-filter chips are single controls rather than composites, so
/// they legitimately have no cursor — and that is exactly where the old
/// fall-through did its damage, because a key nothing consumed reached a global
/// handler that moved a pane the reader was not in.
#[test]
fn r1698_an_arrow_on_a_plain_button_moves_no_pane() {
    with_state(|state| {
        let before: Vec<Option<String>> = PANE_CURSORS
            .iter()
            .map(|(stop, _)| {
                pane_cursor(state, stop).and_then(|r| r.cursor_tag().map(str::to_owned))
            })
            .collect();
        for n in 0..spec::SAVED_FILTERS.len() {
            let chip = format!("pv.filter.saved.{n}");
            assert!(pane_cursor(state, &chip).is_none(), "{chip} owns no cursor");
            assert!(
                !press_key(Some(&chip), "ArrowDown"),
                "{chip} does not claim an arrow"
            );
        }
        let after: Vec<Option<String>> = PANE_CURSORS
            .iter()
            .map(|(stop, _)| {
                pane_cursor(state, stop).and_then(|r| r.cursor_tag().map(str::to_owned))
            })
            .collect();
        assert_eq!(before, after, "★ and no pane's cursor moved");

        // The wire's own channel still reaches the list, so an agent asking for
        // it is not collateral damage of the scoping.
        assert!(press_key(None, "ArrowDown"), "the wire still drives it");
    });
}

/// ★★★ R1698 — the panes publish their cursor, and the message list's cursor
/// **is** its selection.
///
/// The `Follows` arm's real consumer: moving down a message list means reading
/// the next message, which is the opposite of what a navigation rail must do —
/// and the sibling screen declares every one of its composites `Explicit` for
/// exactly that reason. Both arms are load-bearing across the two screens.
#[test]
fn r1698_the_list_cursor_is_the_selection_and_it_is_published() {
    with_state(|state| {
        let roving = pane_cursor(state, "pv.list").expect("the list has a cursor");
        assert_eq!(
            roving.spec().activation,
            pinion_core::widgets::roving::Activation::Follows,
            "the list's cursor IS its selection"
        );
        assert_eq!(
            roving.cursor(),
            Some(state.row.get()),
            "and it reports the row the screen already holds, not a second one"
        );

        assert!(press_key(Some("pv.list"), "ArrowDown"));
        assert_eq!(
            pane_cursor(state, "pv.list").and_then(|r| r.cursor()),
            Some(state.row.get()),
            "moving the cursor moved the selection — one fact, not two"
        );

        let focus = PacketView::access_focus_target(&IDLE_FIELD, Some("pv.list"))
            .expect("a focused pane reports a focus target");
        assert_eq!(focus.focus_tag, "pv.list");
        assert_eq!(
            focus.active_descendant,
            Some(format!("pv.list.row.{}", state.row.get())),
            "and the active descendant names the row the cursor is on"
        );
    });
}

// ── R1699: a row is a composite, and a stop can be acted on ──────────────────

/// ★★★★★ R1699 — **a message row is entered and its cells are walked.**
///
/// This screen announces its list as a `grid`, and WAI-ARIA's grid pattern is
/// two axes: the vertical one moves between rows, the horizontal one between
/// the cells of the row you are on. Measured the day the round opened, by
/// driving the running window: all sixteen rows report seven cells to the
/// accessibility tree, and `ArrowRight`, `ArrowLeft`, `Enter` and `Space`
/// standing on a row moved the active descendant nowhere. The columns existed
/// for a reader and were unreachable by one.
///
/// The floor does this — measured at 6.11.1, an item view is one Tab stop and
/// both axes move a cell cursor, and its accessibility interface names the
/// focused cell. What it cannot do is say the row is a unit with an inside:
/// there is no entering, no leaving, and (measured) `Tab` inside the view moves
/// between cells instead of leaving the widget at all.
#[test]
fn r1699_a_message_row_is_entered_and_its_cells_are_walked() {
    with_state(|state| {
        let descendant = || {
            PacketView::access_focus_target(&IDLE_FIELD, Some("pv.list"))
                .and_then(|t| t.active_descendant)
        };
        let row = state.row.get();
        assert_eq!(descendant(), Some(format!("pv.list.row.{row}")));
        assert_eq!(state.cell.get(), None, "the screen opens on the row");

        // The cross-axis arrow descends, because a row IS a composite.
        assert!(press_key(Some("pv.list"), "ArrowRight"));
        assert_eq!(
            descendant(),
            Some(list_cell_tag(row, 0)),
            "★ entering a row lands on its first cell"
        );
        assert_eq!(state.cell.get(), Some(0));

        assert!(press_key(Some("pv.list"), "ArrowRight"));
        assert_eq!(descendant(), Some(list_cell_tag(row, 1)));
        assert!(press_key(Some("pv.list"), "End"));
        assert_eq!(
            descendant(),
            Some(list_cell_tag(row, spec::COLUMNS.len() - 1)),
            "End reaches the last column"
        );
        // ★★★★★ The `Ends::Stop` declaration is tested by an ADVANCE past the
        // last member, not by pressing `End` twice: `Step::Last` lands on the
        // last index whatever the ends policy says, so the first draft's second
        // `End` asserted that `Last` is idempotent and nothing else. A
        // counterfactual flipping this row's cells to `Ends::Wrap` PASSED
        // against it — the assertion was in a place it could not fail.
        assert!(press_key(Some("pv.list"), "ArrowRight"));
        assert_eq!(
            descendant(),
            Some(list_cell_tag(row, spec::COLUMNS.len() - 1)),
            "and an advance past the last column STOPS — a row is not a ring, \
             unlike the tab list beside it on the sibling screen"
        );
        assert_eq!(
            state.row.get(),
            row,
            "walking the cells did not change which message is decoded"
        );

        // Escape leaves the row without leaving the pane.
        assert!(press_key(Some("pv.list"), "Escape"));
        assert_eq!(descendant(), Some(format!("pv.list.row.{row}")));
        assert_eq!(state.cell.get(), None);
        assert!(
            press_key(Some("pv.list"), "ArrowDown"),
            "and the pane's own axis answers again"
        );
        assert_eq!(state.row.get(), row + 1, "which moves between rows");
    });
}

/// ★★★★★ R1699 — **the grid publishes the cell a reader is in.**
///
/// `GridCell::focused` has existed since R1694 and this screen hard-coded it
/// `false` at all 112 cells, which is what a grid with no way into its rows
/// looks like from the accessibility side: seven cells per row, none of them
/// ever current.
#[test]
fn r1699_the_grid_publishes_the_cell_a_reader_is_in() {
    with_state(|state| {
        let focused_cells = || {
            PacketView::access_node(&IDLE_FIELD, None)
                .into_iter()
                .filter(|n| n.state.focused && n.tag.starts_with("pv.list.cell."))
                .map(|n| n.tag)
                .collect::<Vec<_>>()
        };
        assert!(
            focused_cells().is_empty(),
            "nobody has gone into a row, so no cell is current"
        );

        let row = state.row.get();
        assert!(press_key(Some("pv.list"), "ArrowRight"));
        assert!(press_key(Some("pv.list"), "ArrowRight"));
        assert_eq!(
            focused_cells(),
            vec![list_cell_tag(row, 1)],
            "★ exactly one cell is current, and it is the one the arrows reached"
        );

        // And the row it is in still publishes its own roster, which is what
        // makes "what is inside this row" askable without pressing a key.
        let nodes = PacketView::access_node(&IDLE_FIELD, None);
        let row_node = nodes
            .iter()
            .find(|n| n.tag == format!("pv.list.row.{row}"))
            .expect("the row is in the tree");
        let nav = row_node
            .navigation
            .as_ref()
            .expect("★ a row is a composite and publishes what its arrows reach");
        assert_eq!(nav.members().len(), spec::COLUMNS.len());
        assert!(nav.entered() || nav.cursor().is_some());
    });
}

/// ★★★★★ R1699 — **a filter chip is pressed from the keyboard.**
///
/// Measured before this existed: the three chips announce `role=button`, a
/// keyboard reaches all three, and `Enter` and `Space` at every one of them
/// changed nothing painted. A button a keyboard cannot press is below the floor
/// rather than above it — measured at 6.11.1, a push button activates on both
/// keys, always.
#[test]
fn r1699_a_filter_chip_is_pressed_from_the_keyboard() {
    with_state(|state| {
        for (n, saved) in spec::SAVED_FILTERS.iter().enumerate() {
            let name = saved.name;
            let chip = format!("pv.filter.saved.{n}");
            let before = state.saved.get();
            assert!(
                press_key(Some(&chip), "Enter"),
                "{chip} announces itself a button and must answer Enter"
            );
            assert_ne!(state.saved.get(), before, "{name} toggled");
            assert!(press_key(Some(&chip), "Space"), "{chip} answers Space too");
            assert_eq!(
                state.saved.get(),
                before,
                "{name} toggled back — both keys are the same verb"
            );
        }
    });
}

/// ★★★★★ R1699 — **the tag a cursor names and the tag a press lands on are the
/// same thing.**
///
/// `Hit::of_tag` is a second address space beside `Hit::at`, and two address
/// spaces drift. So every member of every composite — the nested cell rosters
/// included — must resolve to exactly what a press at the centre of that tag's
/// **painted** rectangle answers. The paint is the arbiter, which is what makes
/// this a check rather than a comparison of a table with itself (R1669).
#[test]
fn r1699_every_cursor_member_resolves_to_the_hit_its_tag_names() {
    with_state(|state| {
        let mut scene = super::view(
            (pinion_core::widgets::text_field::TextFieldState::Idle, 0),
            pinion_core::Frame::default(),
        );
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
        let rects = scene.absolute_rects_by_tag();

        let mut tags: Vec<String> = Vec::new();
        for (stop, _) in PANE_CURSORS {
            let roving = pane_cursor(state, stop).expect("a pane cursor");
            for member in roving.members() {
                tags.push(member.tag.clone());
                if let Some(inner) = member.inner() {
                    tags.extend(inner.members().iter().map(|m| m.tag.clone()));
                }
            }
        }
        for n in 0..spec::SAVED_FILTERS.len() {
            tags.push(format!("pv.filter.saved.{n}"));
        }

        let mut wrong = Vec::new();
        let mut checked = 0;
        for tag in &tags {
            let hit = super::Hit::of_tag(state, tag);
            if hit == super::Hit::None {
                wrong.push(format!("{tag}: a cursor rests here and no hit names it"));
                continue;
            }
            // A member the pane has scrolled out of view has no painted
            // rectangle to press; the address half above still had to hold.
            let Some(rect) = rects.get(tag).copied() else {
                continue;
            };
            let at = super::Hit::at(state, rect.x + rect.w / 2, rect.y + rect.h / 2);
            if at != hit {
                wrong.push(format!("{tag}: a press at its centre answers {at:?}"));
            }
            checked += 1;
        }
        assert!(
            wrong.is_empty(),
            "{} member(s):\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );
        assert!(
            tags.len() >= 150,
            "the rosters are smaller than the screen has: {}",
            tags.len()
        );
        assert!(
            checked >= 20,
            "only {checked} member(s) were checked against the paint"
        );
    });
}

/// ★★★★★ R1747 — **every message's decode has the layers this session says it
/// has, named by their own names.**
///
/// Two defects in one assertion, and both were found by the conformance verdict
/// rather than here — which is the argument for the verdict. Measured before
/// the repair, on the message the list opens on and then on message 1:
///
/// * the stand-in decode this screen builds for a message `spec::FIELDS` does
///   not describe had **three** layers, while the context strip beside it says
///   the session negotiated four. The screen contradicted itself, and nothing
///   noticed because the only decode anything asserted about was the described
///   one — `r1693_a_tree_items_position_is_among_its_own_siblings` says in its
///   own comment that the top level is the four layers, and only ever looked at
///   the one message where that was true.
/// * a layer HEADING fell through to the leaf of its path there, so a reader on
///   any other message saw `l0` where the reference draws the layer's name.
///
/// The population is every row of the list, so a message added to the fixture
/// is covered without this test being told.
#[test]
fn r1747_every_messages_decode_names_the_layers_the_session_declares() {
    with_state(|state| {
        for row in 0..spec::ROWS.len() {
            super::select_message(state, row);
            let headings: Vec<(String, String)> = super::visible_fields(state)
                .into_iter()
                .filter(|(_, _, _, depth)| *depth == 0)
                .map(|(path, name, ..)| (path, name))
                .collect();
            let wanted: Vec<(String, String)> = spec::LAYERS
                .iter()
                .map(|(id, title)| ((*id).to_owned(), (*title).to_owned()))
                .collect();
            assert_eq!(
                headings, wanted,
                "message {row}'s decode does not name the layers this session \
                 declares, in order",
            );
        }
    });
}
