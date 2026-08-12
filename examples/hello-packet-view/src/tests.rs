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
    comma, decode, frame_bytes, select_byte, select_field, select_message, spec, use_view_state,
};

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

/// A count is printed the way the reference prints one.
#[test]
fn r1663_counts_carry_thousands_separators() {
    assert_eq!(comma(spec::MATCHED), "12,418");
    assert_eq!(comma(spec::CAPTURED), "184,392");
    assert_eq!(comma(0), "0");
    assert_eq!(comma(999), "999");
    assert_eq!(comma(1_000), "1,000");
}

/// The saved filters and the layer folds are independent switches, and the wire
/// and a press reach the same ones.
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
