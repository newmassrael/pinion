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
    NAME_COLUMN, cell_texts, char_count, comma, decode, frame_bytes, lane_reading, row_cells,
    select_byte, select_field, select_message, sibling_place, spec, use_view_state,
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
        let _state = use_view_state();
        let scene = super::view((), pinion_core::Frame::default());
        let mut want: Vec<String> = vec![
            "pv.list".to_owned(),
            "pv.tree".to_owned(),
            "pv.bytes".to_owned(),
        ];
        want.extend((0..spec::SAVED_FILTERS.len()).map(|n| format!("pv.filter.saved.{n}")));
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
