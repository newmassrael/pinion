//! R1648/R1649 §5.21 — the shell's pure parts.
//!
//! The demo (`tools/demos/r1648_the_analyzer_shell_is_assembled.py`) drives the
//! live window over RPC and is where every wire claim is checked. These pin the
//! functions where a unit test is the sharper instrument — in particular the
//! ones that must be **total over a vocabulary**, which a demo can only sample.

use pinion_core::scene::Rect;
use pinion_core::widgets::card::{CardAffordance, CardState, Remedy};
use pinion_core::widgets::tile_grid::Tile;
use pinion_core::widgets::transport::TransportStatus;

use super::{
    BarChip, DEFS, GRID_COLS, KEYMAP, OVERVIEW, RAIL, SECTIONS, SOURCES, STEPPERS, SubChip, TABS,
    cell_at, cell_rect, def_of, kind_of, parse_state, remedy_label, remedy_word, state_sentence,
    transport_word,
};

/// R1649 — the catalogue is twelve kinds, distinct, each in a listed section.
///
/// The palette's footer states the count, so a kind added without a section
/// would be offered nowhere and the two numbers on screen would disagree.
#[test]
fn r1649_the_catalogue_is_twelve_kinds_in_listed_sections() {
    assert_eq!(DEFS.len(), 12, "the palette's footer states this count");
    let mut seen = std::collections::BTreeSet::new();
    for def in DEFS {
        assert!(seen.insert(def.kind), "{} appears twice", def.kind);
        assert!(
            SECTIONS.iter().any(|(key, _)| *key == def.section),
            "{} is in section {:?}, which the palette does not list",
            def.kind,
            def.section
        );
        assert!(!def.label.is_empty() && !def.desc.is_empty());
        assert!(def.cols >= 1 && def.cols <= GRID_COLS, "{} fits", def.kind);
        assert!(def.rows >= 1, "{} has height", def.kind);
        assert!(!def.chrome.is_empty(), "{} offers something", def.kind);
    }
}

/// R1649 — the opening layout names catalogue kinds and fits the grid.
#[test]
fn r1649_the_overview_layout_is_a_legal_board() {
    assert_eq!(OVERVIEW.len(), 3, "three placed of twelve offered");
    for (kind, col, _row) in OVERVIEW {
        let def = def_of(kind).unwrap_or_else(|| panic!("{kind} is in the catalogue"));
        assert!(
            col + def.cols <= GRID_COLS,
            "{kind} at column {col} would run off a {GRID_COLS}-column grid"
        );
    }
}

/// R1649 — a card id carries its kind, so a definition is recoverable without
/// a side table and a kind can be placed more than once.
#[test]
fn r1649_a_card_id_carries_its_kind() {
    assert_eq!(kind_of("topology#0"), "topology");
    assert_eq!(kind_of("topology#17"), "topology");
    assert_eq!(kind_of("bare"), "bare", "an id with no ordinal is its kind");
    assert!(def_of(kind_of("packet#3")).is_some());
}

/// R1649 — the cell arithmetic round-trips: the pixels a cell is drawn at map
/// back to that cell.
///
/// The one place the paint's forward direction and the gesture's inverse meet,
/// and the property that makes a drag land where the preview said it would.
#[test]
fn r1649_a_cell_and_its_pixels_are_inverses() {
    for col in 0..GRID_COLS {
        for row in 0..4 {
            let rect = cell_rect(&Tile::new("probe", col, row, 1, 1));
            let (back_col, back_row) = cell_at(rect.x + rect.w / 2, rect.y + rect.h / 2);
            assert_eq!(
                (back_col, back_row),
                (col, row),
                "cell ({col},{row}) draws at ({},{}) and reads back as \
                 ({back_col},{back_row})",
                rect.x,
                rect.y
            );
        }
    }
}

/// R1649 — the chrome's pressable rectangles do not overlap each other.
///
/// Two controls sharing pixels means one of them can never be pressed, and the
/// hit test would resolve whichever the loop reached first — silently.
#[test]
fn r1649_no_two_chrome_controls_share_pixels() {
    let overlaps =
        |a: Rect, b: Rect| a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    for (i, one) in BarChip::ALL.iter().enumerate() {
        for other in &BarChip::ALL[i + 1..] {
            assert!(
                !overlaps(one.rect(), other.rect()),
                "{one:?} and {other:?} overlap"
            );
        }
    }
    for (i, one) in SubChip::ALL.iter().enumerate() {
        for other in &SubChip::ALL[i + 1..] {
            assert!(
                !overlaps(one.rect(), other.rect()),
                "{one:?} and {other:?} overlap"
            );
        }
    }
}

/// R1649 — every chrome control's tag is distinct, because a tag is the
/// address the wire and the demo both compare.
#[test]
fn r1649_every_chrome_tag_is_distinct() {
    let mut seen = std::collections::BTreeSet::new();
    for chip in BarChip::ALL {
        assert!(seen.insert(chip.tag()), "{} is used twice", chip.tag());
    }
    for chip in SubChip::ALL {
        assert!(seen.insert(chip.tag()), "{} is used twice", chip.tag());
    }
}

/// R1648 — a card's state sentence is total, and the two arms that carry a
/// reason SAY it.
#[test]
fn r1648_every_state_has_a_sentence_and_the_carried_ones_quote_it() {
    for state in CardState::ALL {
        assert!(
            !state_sentence(&state).is_empty(),
            "{state:?} has no sentence"
        );
    }
    assert!(
        state_sentence(&CardState::Failed("collector unreachable".into()))
            .contains("collector unreachable"),
        "a failure quotes its reason"
    );
    assert!(
        state_sentence(&CardState::Denied("operator role".into())).contains("operator role"),
        "and a denial names the right"
    );
}

/// R1648 — every remedy has a label, and they are distinct.
///
/// Two remedies reading the same on screen would undo the distinction the
/// vocabulary exists for: "request access" and "nothing can be done" have to
/// look different or the six arms buy nothing a person can see.
#[test]
fn r1648_every_remedy_reads_differently() {
    let mut seen = std::collections::BTreeSet::new();
    for remedy in Remedy::ALL {
        let text = remedy_label(remedy);
        assert!(!text.is_empty(), "{remedy:?} has no label");
        assert!(seen.insert(text), "{text:?} labels two remedies");
    }
    assert_eq!(seen.len(), Remedy::ARMS);
}

/// R1648 — the wire word for "no remedy" is not one of the remedies.
#[test]
fn r1648_the_absent_remedy_is_not_a_remedy() {
    assert_eq!(remedy_word(None), "none");
    assert!(
        !Remedy::ALL.map(Remedy::wire).contains(&"none"),
        "`none` must not collide with a published remedy"
    );
    for remedy in Remedy::ALL {
        assert_eq!(remedy_word(Some(remedy)), remedy.wire());
    }
}

/// R1648 — the detail is required by exactly the two arms that carry one.
///
/// Both directions. Accepting a reason on `empty` silently drops what the
/// caller sent; refusing one on `failed` is how a reason gets lost.
#[test]
fn r1648_the_detail_arity_is_a_fact_about_the_vocabulary() {
    for word in ["ready", "loading", "empty", "opaque"] {
        assert!(parse_state(word, None).is_ok(), "{word} takes no reason");
        let refused = parse_state(word, Some("why")).expect_err("and refuses one");
        assert!(refused.contains("carries no reason"), "{refused}");
    }
    for word in ["failed", "denied"] {
        let refused = parse_state(word, None).expect_err("a carried arm needs its reason");
        assert!(refused.contains("carries a reason"), "{refused}");
        let made = parse_state(word, Some("because")).expect("with one it parses");
        assert_eq!(made.detail(), Some("because"), "and it round-trips");
    }
    let unknown = parse_state("sideways", None).expect_err("closed set");
    assert!(unknown.contains("is not a card state"), "{unknown}");
}

/// R1648 — every state word the wire publishes is one `parse_state` accepts.
///
/// Publishing a vocabulary the parser does not take is two definitions of one
/// set (R1642), and the demo can only sample it.
#[test]
fn r1648_every_published_state_word_is_accepted() {
    for state in CardState::ALL {
        let word = state.wire();
        let detail = state.detail().map(|_| "a reason");
        let made = parse_state(word, detail)
            .unwrap_or_else(|why| panic!("{word} is published but refused: {why}"));
        assert_eq!(made.wire(), word);
    }
}

/// R1648 — `live` is derived, and it is the only word capture can change.
///
/// The whole cross product, because the derivation is over two inputs and a
/// test that pushed one axis at a time would not see that a replaying board
/// reports `replaying` whether or not capture is on (R1623's lesson).
#[test]
fn r1648_the_transport_word_is_derived_from_the_clock_and_the_toggle() {
    for status in [
        TransportStatus::Playing,
        TransportStatus::Paused,
        TransportStatus::Stopped,
    ] {
        for capturing in [true, false] {
            let expected = match (status, capturing) {
                (TransportStatus::Playing, _) => "replaying",
                (TransportStatus::Stopped, true) => "live",
                _ => "paused",
            };
            assert_eq!(
                transport_word(status, capturing),
                expected,
                "{status:?} with capture={capturing}"
            );
        }
    }
}

/// R1649 — the app bar's source list is a set the shell opens within.
#[test]
fn r1649_the_sources_are_a_set_the_shell_opens_within() {
    let mut seen = std::collections::BTreeSet::new();
    for source in SOURCES {
        assert!(seen.insert(source), "{source} listed twice");
    }
    assert_eq!(seen.len(), SOURCES.len());
    // NOT `assert!(!SOURCES.is_empty())`: on a `const` array that cannot fail,
    // and an assertion that cannot fail reads as coverage (R1644.1).
    assert!(
        SOURCES.contains(&SOURCES[0]),
        "the app bar opens on a source it offers"
    );
}

/// R1649 — the catalogue exercises every affordance, in both directions.
///
/// The second half is what gives the wire's refusal path a case to demonstrate:
/// a catalogue where every kind offers an affordance can never show it being
/// refused. R1648's first draft failed exactly this, and its own test caught it.
#[test]
fn r1649_the_catalogue_exercises_every_affordance_in_both_directions() {
    let mut offered = std::collections::BTreeSet::new();
    for def in DEFS {
        offered.extend(def.chrome.iter().copied());
    }
    assert_eq!(
        offered.len(),
        CardAffordance::ARMS,
        "unexercised affordances: {:?}",
        CardAffordance::ALL
            .into_iter()
            .filter(|a| !offered.contains(a))
            .collect::<Vec<_>>()
    );
    // ★ `Close` is UNIVERSAL by design — every widget in the reference tool has
    // a ✕, and a card a person cannot get rid of is not a thing that shell
    // offers. Stated here rather than left as an accident of the table, because
    // the check below would otherwise read as "nobody withholds close YET".
    assert!(
        DEFS.iter()
            .all(|d| d.chrome.contains(&CardAffordance::Close)),
        "every widget kind can be closed"
    );
    // The other three must each have a kind that withholds them, or the wire's
    // refusal path has no case to demonstrate on. R1648's first draft failed
    // exactly this for `Settings`, and its own test caught it.
    for affordance in CardAffordance::ALL {
        if affordance == CardAffordance::Close {
            continue;
        }
        assert!(
            DEFS.iter().any(|d| !d.chrome.contains(&affordance)),
            "every kind offers {affordance:?}, so nothing can refuse it"
        );
    }
}

/// R1649 — the published vocabularies have no repeats.
#[test]
fn r1649_the_published_vocabularies_have_no_repeats() {
    let distinct = |what: &str, words: Vec<&str>| {
        let mut seen = std::collections::BTreeSet::new();
        for word in &words {
            assert!(seen.insert(*word), "{what}: {word:?} appears twice");
        }
        assert_eq!(seen.len(), words.len());
    };
    distinct("steppers", STEPPERS.iter().map(|(v, _)| *v).collect());
    distinct("rail", RAIL.iter().map(|(k, _)| *k).collect());
    distinct("tabs", TABS.to_vec());
    distinct("keymap", KEYMAP.iter().map(|(c, _)| *c).collect());
}
