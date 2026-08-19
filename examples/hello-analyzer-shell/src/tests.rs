//! R1648/R1649 §5.21 — the shell's pure parts.
//!
//! The demo (`tools/demos/r1648_the_analyzer_shell_is_assembled.py`) drives the
//! live window over RPC and is where every wire claim is checked. These pin the
//! functions where a unit test is the sharper instrument — in particular the
//! ones that must be **total over a vocabulary**, which a demo can only sample.

use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::widgets::card::{CardAffordance, CardState, Remedy};
use pinion_core::widgets::tile_grid::Tile;
use pinion_core::widgets::transport::TransportStatus;
use pinion_screen::ScreenState;

use std::collections::BTreeMap;

use super::{
    AnalyzerShellView, BarChip, GRID_COLS, KEYMAP, SOURCES, STEPPERS, SubChip, TABS, cell_at,
    cell_rect, chrome, def_of, kind_of, kind_span, parse_state, remedy_label, remedy_word, spec,
    state_sentence, transport_word, type_ink, use_shell_state,
};
use pinion_a11y::WidgetA11y;
use pinion_core::WidgetCore;

/// A palette whose roles are all distinct, so a test that asks "did this take
/// the fallback ink" gets an answer rather than a coincidence.
fn probe_palette() -> super::Palette {
    use pinion_core::style::Color;
    let mut n = 0_u8;
    let mut next = || {
        n += 1;
        Color::rgb(n, n, n)
    };
    super::Palette {
        ink: next(),
        muted: next(),
        accent: next(),
        on_accent: next(),
        accent_fg: next(),
        canvas: next(),
        panel: next(),
        raised: next(),
        high: next(),
        outline: next(),
        grid: next(),
        warn: next(),
        refused: next(),
    }
}

/// R1668 — the catalogue is thirteen entries in two tiers, each in a listed
/// section, and each section is homogeneous in tier.
///
/// The palette's footer states **both** counts, so an entry that moved tier
/// without moving section would put two numbers on the screen that disagree
/// with the list under them.
#[test]
fn r1668_the_catalogue_is_four_placeable_and_nine_reserved() {
    assert_eq!(spec::CATALOGUE.len(), 13);
    assert_eq!(spec::placeable_count(), 4, "the palette's footer says this");
    assert_eq!(spec::reserved_count(), 9, "and this");

    let mut seen = std::collections::BTreeSet::new();
    let mut codes = std::collections::BTreeSet::new();
    for def in spec::CATALOGUE {
        assert!(seen.insert(def.kind), "{} appears twice", def.kind);
        assert!(codes.insert(def.code), "{} shares its code", def.kind);
        let section = spec::SECTIONS
            .iter()
            .find(|(key, ..)| *key == def.section)
            .unwrap_or_else(|| panic!("{} is in an unlisted section", def.kind));
        assert_eq!(
            section.2, def.tier,
            "{} sits in a {:?} section at tier {:?}; the heading a reader scans \
             carries the tier, so a mixed section is a heading that lies",
            def.kind, section.2, def.tier
        );
        assert!(!def.label.is_empty() && !def.gist.is_empty());
        assert_eq!(
            def.tier == spec::Tier::Reserved,
            !def.reserved_for.is_empty(),
            "{} states a booking iff it is reserved",
            def.kind
        );
    }
}

/// R1668 — the opening board places **every** placeable entry, legally.
///
/// Every, not a subset: the palette's footer counts placed against placeable,
/// and the reference's screen opens with nothing left to offer for this
/// release. A board holding three of four is a different claim.
#[test]
fn r1668_the_opening_board_places_every_placeable_entry() {
    assert_eq!(spec::BOARD.len(), spec::placeable_count());
    let mut placed = std::collections::BTreeSet::new();
    for tile in spec::BOARD {
        let def = def_of(tile.kind).unwrap_or_else(|| panic!("{} is in the catalogue", tile.kind));
        assert_eq!(
            def.tier,
            spec::Tier::Placeable,
            "{} is on the opening board and reserved",
            tile.kind
        );
        assert!(placed.insert(tile.kind), "{} is placed twice", tile.kind);
        assert!(
            tile.col + tile.cols <= GRID_COLS,
            "{} at column {} would run off a {GRID_COLS}-column grid",
            tile.kind,
            tile.col
        );
        assert!(tile.cols >= 1 && tile.rows >= 1, "{} has extent", tile.kind);
        assert_eq!(kind_span(tile.kind), Some((tile.cols, tile.rows)));
    }
    for def in spec::CATALOGUE
        .iter()
        .filter(|w| w.tier == spec::Tier::Placeable)
    {
        assert!(
            placed.contains(def.kind),
            "{} is placeable and the opening board leaves it off",
            def.kind
        );
    }
    // And no two tiles overlap -- checked here rather than trusted, because the
    // board is a hand-written arrangement and an overlap paints one card on top
    // of another with nothing to say so.
    for (a, b) in spec::BOARD.iter().zip(spec::BOARD.iter().skip(1)) {
        let _ = (a, b);
    }
    let mut cells = std::collections::BTreeSet::new();
    for tile in spec::BOARD {
        for col in tile.col..tile.col + tile.cols {
            for row in tile.row..tile.row + tile.rows {
                assert!(
                    cells.insert((col, row)),
                    "{} overlaps another tile at cell ({col}, {row})",
                    tile.kind
                );
            }
        }
    }
}

/// R1668 — a reserved entry states a booking, and the rail's reserved seats do
/// too.
///
/// The screen's whole claim is that a later release's work is *visible* rather
/// than absent, and a seat that is merely grey states nothing. Each booking is
/// what the shell hands to the framework's availability channel, so this is
/// also what `scene/disabled` will report.
#[test]
fn r1668_every_reserved_seat_names_what_it_waits_for() {
    let reserved: Vec<_> = spec::CATALOGUE
        .iter()
        .filter(|w| w.tier == spec::Tier::Reserved)
        .collect();
    assert_eq!(reserved.len(), 9);
    for def in reserved {
        assert!(
            def.reserved_for.starts_with("requirement "),
            "{} is booked under {:?}, which names no requirement",
            def.kind,
            def.reserved_for
        );
    }
    let locked: Vec<_> = spec::RAIL
        .iter()
        .filter_map(|seat| seat.reserved_for().map(|why| (seat.key, why)))
        .collect();
    assert_eq!(locked.len(), 2, "the reference locks two rail seats");
    for (key, why) in locked {
        assert!(
            why.starts_with("requirement "),
            "the {key} seat is booked under {why:?}, which names no requirement"
        );
    }
    assert!(
        spec::RAIL
            .iter()
            .any(|seat| seat.key == spec::RAIL_ACTIVE && seat.reserved_for().is_none()),
        "the seat this screen IS cannot be a reserved one",
    );
    // ★★ R1695 — a seat that is neither this application's page nor booked for
    // a release names the surface that HAS it, and names a real one. The arm
    // exists because *reserved* would send a reader to wait for something that
    // has already shipped.
    // ★★★★★ R1724 — **three, then two.** The node lab's seat stopped saying
    // *elsewhere* because the lab is mounted here now
    // (`pinion_screen::Mount<NodeLabView>`), so the tool is one application at
    // that seat rather than two executables.
    //
    // ★★★★★ R1728 — **two, then one.** The other two were never one screen
    // behind two seats: measured against the reference, `stream` and `decode`
    // were this application's invention.
    //
    // ★★★★★ R1729 — **one, then NONE, and the arm itself is gone.** The capture
    // viewer is mounted, so nothing constructed `Seat::Elsewhere` and the
    // compiler said so — an unconstructed variant is a `dead_code` error here.
    // There is no assertion left to write on this axis, which is the strongest
    // form the claim can take: *no seat of this rail can say "built, shipping,
    // and not here", because the screen has no way to spell it.* The framework's
    // `UnavailableKind::Elsewhere` is untouched and still has consumers.
    //
    // ★★ R1728 — what IS still spelled: a seat that is neither a page nor
    // booked names *what specifies it*, so a reader is told the thing exists in
    // the plan rather than being left with a dead icon.
    let unbuilt: Vec<_> = spec::RAIL
        .iter()
        .filter_map(|seat| match seat.seat {
            spec::Seat::Unbuilt(specified_by) => Some((seat.key, specified_by)),
            spec::Seat::Page | spec::Seat::Reserved(_) => None,
        })
        .collect();
    assert!(!unbuilt.is_empty(), "the fourth arm has no seat using it");
    for (key, specified_by) in unbuilt {
        assert!(
            specified_by.starts_with("the ") && specified_by.len() > 8,
            "the {key} seat cites {specified_by:?}, which names no specification",
        );
    }
}

/// ★★★★★ R1728 — **the rail this application runs on IS the rail the reference
/// draws**, and where it is not, the difference is one somebody wrote down.
///
/// The population is the specification's, loaded from
/// `docs/analyzer-rail-spec.json`, and the comparison runs in **both**
/// directions: a seat the reference has and this build lacks, and a seat this
/// build has that the reference does not, are both failures. The one-directional
/// version of this check is what let three invented keys sit on the rail for
/// several hundred rounds.
///
/// The assertion is *equality* with the declared remainder rather than
/// containment. Equality is what makes paying a divergence off fail too — the
/// gate then says "you fixed it, record it", which is the direction a floor
/// cannot see.
/// ★ R1730 — the *judgement* is the framework's now
/// ([`pinion_core::conformance::Ledger`]), which reports WHICH direction failed
/// instead of leaving a reader to compare two vectors. The three per-entry
/// conditions this test used to assert inline — the sentence names its key, the
/// entry names a round, the entry states a reason — are refused at load, so
/// every specification written from now on gets them without remembering to.
#[test]
fn r1728_the_rail_reproduces_the_reference_or_says_where_it_does_not() {
    let built = spec::destinations();
    let found = spec::canon_spec().diff(&built);
    let owed = spec::owed();
    let unreconciled: Vec<String> = owed
        .judge(&found)
        .iter()
        .map(pinion_core::conformance::Unreconciled::sentence)
        .collect();
    assert!(
        unreconciled.is_empty(),
        "the rail's difference from the reference is not the difference \
         `docs/analyzer-rail-spec.json` declares:\n  {}",
        unreconciled.join("\n  "),
    );
    // And the reproduction is a number rather than an impression.
    let reproduced = spec::canon_spec().len() - owed.len();
    assert_eq!(
        reproduced + owed.len(),
        built.len(),
        "every seat is either reproduced or owed, and none is both",
    );
}

/// ★★ R1728 — the specification itself is a roster, and it is the reference's
/// eight seats rather than whatever this application happens to hold.
///
/// Separate from the conformance test above on purpose: if the pin were
/// malformed or truncated, a diff against it could come out empty and read as
/// success. This asserts the thing being compared against is the right size and
/// shape first.
#[test]
fn r1728_the_specification_is_the_references_own_rail() {
    let canon = spec::canon_spec();
    assert_eq!(
        canon.len(),
        8,
        "the reference draws eight seats on every one of its screens",
    );
    let keys: Vec<&str> = canon.seats().iter().map(|s| s.key.as_ref()).collect();
    assert_eq!(
        keys,
        [
            "dashboard",
            "packets",
            "keys",
            "logs",
            "lab",
            "topology",
            "sessions",
            "settings"
        ],
        "the specification is the reference's rail in the reference's order",
    );
    // Two seats the reference draws locked itself, and the rest it opens. This
    // is a fact about the REFERENCE, not about this build — which is why it
    // belongs here and not in the diff.
    let locked: Vec<&str> = canon
        .seats()
        .iter()
        .filter(|s| s.required != pinion_core::widgets::destination::Required::Open)
        .map(|s| s.key.as_ref())
        .collect();
    assert_eq!(locked, ["topology", "sessions"]);
}

/// R1668 — the header controls the specification names all map onto an
/// affordance this shell can paint.
#[test]
fn r1668_every_named_header_control_is_one_the_shell_has() {
    let painted = chrome();
    assert_eq!(painted.len(), spec::CARD_CHROME.len());
    for (name, affordance) in spec::CARD_CHROME.iter().zip(painted) {
        assert_eq!(*name, affordance.wire(), "the two vocabularies agree");
    }
}

/// R1668 — every message type on a specified row is one the legend colours.
///
/// The ink is looked up by position in the legend, so a row carrying an
/// unlisted type would be drawn in the muted ink; this asserts no row is.
#[test]
fn r1668_every_streamed_type_is_one_the_legend_lists() {
    for (_, kind, ..) in spec::STREAM_ROWS {
        assert!(
            spec::STREAM_TYPES.contains(kind),
            "a row carries type {kind:?}, which the legend does not list"
        );
    }
    let palette = probe_palette();
    let muted = palette.muted;
    for kind in spec::STREAM_TYPES {
        assert_ne!(
            type_ink(kind, palette),
            muted,
            "{kind:?} is in the legend and takes the fallback ink"
        );
    }
    assert_eq!(
        type_ink("not-a-type", palette),
        muted,
        "an unlisted type takes the fallback rather than another type's colour",
    );
}

/// R1668 — the selected decode row's byte span is inside the bytes drawn, and
/// is not empty.
///
/// The law screen B is built on, held here too: what is drawn lit is what the
/// map says the selection occupies. A span past the end would light nothing and
/// look exactly like a selection with no bytes.
#[test]
fn r1668_the_lit_span_is_inside_the_bytes_it_lights() {
    let (start, end) = spec::DECODE_SELECTED_SPAN;
    let total = spec::DECODE_BYTES.len() * 4;
    assert!(start < end, "an empty span lights nothing");
    assert!(end <= total, "the span runs past the bytes drawn");
    assert!(
        spec::DECODE_SELECTED < spec::DECODE_ROWS.len(),
        "the selected row is one of the rows",
    );
    let (depth, ..) = spec::DECODE_ROWS[spec::DECODE_SELECTED];
    assert!(depth > 0, "a layer heading is not a field and has no bytes");
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

/// R1668 — the board's chrome is UNIFORM, deliberately, and the wire's refusal
/// path still has a case to demonstrate on.
///
/// R1649 made the chrome vary between kinds so that a refusal could be shown at
/// all, and its own test pinned that. The reference does not: every placed card
/// carries the same four controls, and a uniform board is what makes a missing
/// control legible. So the variation goes and the refusal case moves to the one
/// this round introduced -- a reserved kind, which the palette will not place.
/// That is a better case anyway: it refuses for a reason a person can read.
#[test]
fn r1668_the_chrome_is_uniform_and_a_refusal_is_still_demonstrable() {
    let offered: std::collections::BTreeSet<_> = chrome().into_iter().collect();
    assert_eq!(
        offered.len(),
        CardAffordance::ARMS,
        "unexercised affordances: {:?}",
        CardAffordance::ALL
            .into_iter()
            .filter(|a| !offered.contains(a))
            .collect::<Vec<_>>()
    );
    assert!(
        offered.contains(&CardAffordance::Close),
        "a card a person cannot get rid of is not a thing this shell offers",
    );

    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let reserved = spec::CATALOGUE
            .iter()
            .find(|w| w.tier == spec::Tier::Reserved)
            .expect("nine of them");
        let refusal = super::ShellOracle::add(&state, reserved.kind)
            .expect_err("a reserved kind is not placed");
        let said = format!("{refusal:?}");
        assert!(
            said.contains(reserved.reserved_for),
            "the refusal is {said:?} and does not say what it is waiting for",
        );
        // And a placeable one is placed, so the refusal is about the tier and
        // not about the path being broken.
        let placeable = spec::CATALOGUE
            .iter()
            .find(|w| w.tier == spec::Tier::Placeable)
            .expect("four of them");
        super::ShellOracle::add(&state, placeable.kind).expect("the palette offers it");
    });
}

/// R1649.1 — ★★ a REAL pointer reaches this surface.
///
/// The §5.35 router resolves the hit target by hit-testing the paint scene for
/// the deepest TAGGED node under the cursor, then looks up an `External`
/// carrying that tag. Every tag here is an address and there is exactly one
/// `External` — the root — so a tagged child that is not `pointer_transparent`
/// makes the lookup fail and the router forwards NOTHING: the whole shell is
/// dead to a mouse.
///
/// ★ That shipped, and the demo did not catch it, because the demo drives
/// `point` / `send` over the wire and those BYPASS the router. A capability
/// verified only through a bypass is not verified
/// (debt-a-surface-can-be-dead-to-a-real-pointer). This is the assertion that
/// makes it impossible to ship again: every point in the window must hit-test
/// to the root's tag, which is the one the router can resolve an `External`
/// for.
#[test]
fn r1649_every_tag_but_the_root_is_pointer_transparent() {
    let owner = Owner::new();
    owner.run(|| {
        let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut tagged = 0;
        let mut walk = vec![(&scene, true)];
        while let Some((node, is_root)) = walk.pop() {
            if let Some(tag) = node.tag() {
                tagged += 1;
                if is_root {
                    assert_eq!(tag, super::VIEW_TAG, "the root carries the External's tag");
                    assert!(
                        !node.is_pointer_transparent(),
                        "the ROOT must stay opaque, or there is no hit target at all"
                    );
                } else {
                    assert!(
                        node.is_pointer_transparent(),
                        "{tag:?} carries a tag and is NOT pointer-transparent, so the \
                         router resolves it as the hit target, finds no External with \
                         that tag, and forwards nothing — the surface is dead to a real \
                         pointer. Give it `with_pointer_transparent(true)`."
                    );
                }
            }
            if let pinion_core::Scene::Container(container) = node {
                for child in &container.children {
                    walk.push((child, false));
                }
            }
        }
        assert!(tagged > 25, "the shell tags plenty to check: {tagged}");
    });
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
    distinct("rail", spec::RAIL.iter().map(|seat| seat.key).collect());
    distinct("tabs", TABS.to_vec());
    distinct("keymap", KEYMAP.iter().map(|(c, _)| *c).collect());
}

/// R1653 — ★ the second surface, asked the question that found five defects on
/// the first.
///
/// A text run carries no tag, so every tag-keyed assertion in this file is
/// blind to where one is painted — and this shell is where that class was first
/// measured (R1649 stacked every card's text down the left edge with 118 wire
/// assertions passing). Two runs of one widget landing on each other is the
/// signature: it is what flow does, and it is what an over-wide box does.
///
/// The rectangles are the boxes the view GAVE the runs, not the extent of their
/// glyphs — a string wider than its box still wraps over what is below it, and
/// nothing here can see that (`debt-a-text-run-cannot-be-elided`).
#[test]
fn r1653_no_two_text_runs_of_one_widget_are_painted_on_top_of_each_other() {
    use std::collections::BTreeMap;
    let owner = Owner::new();
    owner.run(|| {
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);

        let mut by_owner: BTreeMap<String, Vec<(String, pinion_core::scene::Rect)>> =
            BTreeMap::new();
        let mut runs = 0;
        scene.for_each_node(&mut |visit| {
            let (pinion_core::Scene::Text(text), Some(rect)) = (visit.node, visit.absolute_rect())
            else {
                return;
            };
            runs += 1;
            let owner_tag = visit
                .ancestors
                .iter()
                .rev()
                .find_map(|a| a.tag())
                .unwrap_or("<root>")
                .to_owned();
            by_owner
                .entry(owner_tag)
                .or_default()
                .push((text.content.clone(), rect));
        });
        assert!(runs > 40, "the shell paints text: {runs} run(s)");

        let overlaps = |a: pinion_core::scene::Rect, b: pinion_core::scene::Rect| {
            a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
        };
        let mut smeared = Vec::new();
        for (owner_tag, group) in &by_owner {
            for (i, (a_text, a)) in group.iter().enumerate() {
                for (b_text, b) in &group[i + 1..] {
                    if overlaps(*a, *b) {
                        smeared.push((owner_tag, a_text, *a, b_text, *b));
                    }
                }
            }
        }
        assert!(
            smeared.is_empty(),
            "{} pair(s) of text runs are painted over each other: {smeared:?}",
            smeared.len()
        );
    });
}

/// ★★ R1662 — a board taller than the canvas is still a board a person can
/// reach.
///
/// The defect, as R1649 measured it: past roughly four and a half rows a card
/// is painted below the window and no gesture reaches it — the one structural
/// difference from the reference tool that the rebuild had not closed
/// ([[debt-the-analyzer-canvas-does-not-scroll]]). Cards are added until the
/// board is taller than the canvas, and then the property is asked of the
/// framework: every painted mark is on screen or some offset brings it there.
///
/// ★ The end of it is driven rather than derived — the offset the report
/// publishes is scrolled to and the card is pressed at the centre it lands in,
/// so the claim is settled by the screen and not by the arithmetic behind it.
///
/// ★★ R1714 — a card below the fold and the offset that shows it, or `None` for
/// a row that is not a whole card or that no single offset shows.
///
/// The answer is a CHAIN of viewports to move now, and this screen is one clip
/// deep. The assertion says so rather than taking the first entry and hoping: a
/// round that puts a second clip above this canvas makes the recipe below
/// incomplete, and this fails loudly instead of scrolling one of two things.
fn card_to_scroll_to(o: &pinion_core::reach::OutOfSight) -> Option<(String, (i32, i32))> {
    let tag = o.tag.clone()?;
    let card = tag.strip_prefix("card.")?;
    if card.contains('.') {
        return None;
    }
    match &o.reach {
        pinion_core::reach::Reach::Scrollable { moves } => {
            assert_eq!(
                moves.len(),
                1,
                "{tag}: this screen's clip chain is one deep, so one move is the \
                 whole recipe: {moves:?}"
            );
            Some((tag.clone(), moves[0].to))
        }
        // R1713 — a card the range reaches only part of has no single offset
        // that shows it, so there is nothing to press towards.
        pinion_core::reach::Reach::Clipped { .. } | pinion_core::reach::Reach::Lost { .. } => None,
    }
}

#[test]
fn r1662_a_board_taller_than_the_canvas_is_reachable_by_scrolling() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        // Enough cards that the board outgrows the canvas whatever the opening
        // layout holds: the canvas is a fixed height and each row has a pitch.
        for _ in 0..12 {
            super::ShellOracle::add(&state, spec::BOARD[0].kind).expect("the palette offers it");
        }
        let paint = || {
            let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
            let mut cache = pinion_runtime::LayoutCache::new();
            pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
            scene
        };
        let scene = paint();
        let ink = &mut |t: &pinion_core::scene::TextNode| {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a label is a handful of characters"
            )]
            let chars = t.content.chars().count() as u32;
            (chars * t.style.font_size_px.max(1), t.rect.h)
        };
        let out = pinion_core::reach::out_of_sight(&scene, (super::WIN_W, super::WIN_H), ink);
        let lost: Vec<String> = out
            .iter()
            .filter(|o| o.reach.is_lost())
            .map(|o| {
                format!(
                    "{:?} past {} (content {:?}, range {:?})",
                    o.tag.clone().or_else(|| o.content.clone()),
                    o.viewport.name,
                    o.viewport.content,
                    o.viewport.max
                )
            })
            .collect();
        assert!(
            lost.is_empty(),
            "{} mark(s) no gesture can bring into view:\n  {}",
            lost.len(),
            lost.iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );

        // The board really did outgrow the canvas — otherwise the property
        // above is true of a screen that never needed scrolling, which would
        // make this test pass for the wrong reason.
        assert!(
            state.canvas_scroll.max().1 > 0,
            "the fixture did not make the board overflow: range {:?}",
            state.canvas_scroll.max()
        );

        // And a card below the fold answers a press once scrolled to.
        let below: Vec<(String, (i32, i32))> = out.iter().filter_map(card_to_scroll_to).collect();
        assert!(
            !below.is_empty(),
            "no card was below the fold, so the press half checked nothing"
        );
        let mut wrong = Vec::new();
        for (tag, to) in &below {
            state.canvas_scroll.scroll_to(to.0, to.1);
            let scene = paint();
            let Some(rect) = scene.absolute_rects_by_tag().get(tag).copied() else {
                wrong.push(format!("{tag}: scrolling to {to:?} did not paint it"));
                continue;
            };
            let (px, py) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
            let got = super::hit_word(&super::Hit::at(&state, px, py));
            if got != *tag {
                wrong.push(format!(
                    "{tag}: after scrolling to {to:?}, a press at ({px},{py}) answered {got}"
                ));
            }
        }
        state.canvas_scroll.scroll_to(0, 0);
        assert!(
            wrong.is_empty(),
            "{} of {} published offset(s) did not deliver:\n  {}",
            wrong.len(),
            below.len(),
            wrong.join("\n  ")
        );
    });
}

// ── R1694: what reaches a reader who never sees the drawing ────────────────

/// The accessibility tree of the opening screen, keyed by tag.
fn announced() -> std::collections::BTreeMap<String, pinion_a11y::AccessNode> {
    use pinion_a11y::WidgetA11y;
    Owner::new().run(|| {
        super::AnalyzerShellView::access_node(&ScreenState::default(), None)
            .into_iter()
            .map(|node| (node.tag.clone(), node))
            .collect()
    })
}

#[test]
fn the_voice_table_is_a_partition_with_no_tag_in_both_halves() {
    // ★ A total census is satisfied by declaring everything silent, so the
    // table has to fix the SPLIT — and a tag on both sides would let it claim
    // both answers at once.
    let mut voiced = std::collections::BTreeSet::new();
    for voice in spec::VOICES {
        for member in voice.population.members() {
            let tag = voice.tag.replace("{}", &member);
            assert!(voiced.insert(tag.clone()), "{tag} owes a voice twice");
        }
    }
    for (template, population, _, _) in spec::SILENCES {
        for member in population.members() {
            let tag = template.replace("{}", &member);
            assert!(
                !voiced.contains(&tag),
                "{tag} is declared both spoken and quiet",
            );
        }
    }
}

#[test]
fn the_locked_table_is_derived_from_the_tier_and_the_reservation() {
    // ★ R1695 — sixteen then: nine catalogue entries booked for a later
    // release, FIVE rail destinations this application cannot take you to, and
    // the settings page's two key rows. Derived rather than listed, so a seat
    // that is unlocked leaves the table by being unlocked.
    // ★★★★★ R1724 — **fifteen then, and this is the assertion that shows the
    // derivation works.** The node lab's seat has a page now, mounted, so that
    // rail seat is open — and it left this table without anybody editing this
    // table, which is exactly what "derived rather than listed" was for.
    // ★★ R1728 — **sixteen now**, and the same derivation is why: the rail
    // became the reference's eight seats, so it carries five closed ones rather
    // than four. The total is not written down twice — the three assertions
    // below account for every tag by where it comes from, and the length is
    // their sum.
    let tags: Vec<String> = spec::LOCKED
        .iter()
        .flat_map(|(template, population, _)| {
            population
                .members()
                .into_iter()
                .map(move |member| template.replace("{}", &member))
        })
        .collect();
    assert_eq!(
        tags.iter()
            .filter(|t| t.starts_with("shell.palette."))
            .count(),
        spec::reserved_count(),
    );
    assert_eq!(
        tags.iter().filter(|t| t.starts_with("shell.rail.")).count(),
        spec::destinations().closed().count(),
    );
    assert_eq!(
        tags.iter()
            .filter(|t| t.starts_with("shell.settings."))
            .count(),
        spec::KEY_ROWS.len(),
    );
    // ★ R1728 — and the three account for all of them, so the table has no
    // member from a fourth source that the assertions above cannot see. This is
    // what the hand-written total used to be doing, minus the hand.
    assert_eq!(
        tags.len(),
        spec::reserved_count() + spec::destinations().closed().count() + spec::KEY_ROWS.len(),
        "{tags:?}",
    );
}

/// ★★★★★ R1695 — the census is a partition **per destination**, and every open
/// destination is covered.
///
/// The table used to describe one screen because the application had one, and
/// nothing said so — a page added to this screen would simply not appear in any
/// census, which is `debt-the-voice-gate-judges-only-the-opening-screen` in the
/// small. The roster is the enumeration that was missing.
///
/// ★★★★★ R1724 — **and it is a partition over the pages this screen paints.**
///
/// A destination whose page is a mounted screen has no regions in this table
/// and must not: the regions are that screen's, enumerated by that screen's own
/// specification and judged by its own tests. What this census owes is that no
/// destination falls out of *both* — so a key is covered here, or it has a
/// screen, and the assertion names which.
#[test]
fn r1695_every_open_destination_owns_at_least_one_declared_region() {
    let roster = spec::destinations();
    let screens = super::screen_roster();
    for destination in roster.open() {
        let key = destination.key.as_ref();
        let own = spec::VOICES
            .iter()
            .filter(|voice| matches!(voice.at, spec::Where::At(k) if k == key))
            .count();
        if screens.is_mounted(key) {
            assert_eq!(
                own, 0,
                "the {key} destination's page is a mounted screen, so this \
                 screen's census must not also claim regions there — two \
                 censuses over one page is how they come to disagree",
            );
            continue;
        }
        assert!(
            own > 0,
            "the {key} destination is open, this screen paints it, and the \
             census gives it no region of its own, so arriving there is a page \
             nobody judges",
        );
    }
    // And a row cannot name a destination the rail does not hold — the join
    // that would otherwise rot silently.
    for voice in spec::VOICES {
        if let spec::Where::At(key) = voice.at {
            assert!(
                roster.get(key).is_some(),
                "{} belongs to {key:?}, which is not on the rail",
                voice.tag,
            );
        }
    }
}

#[test]
fn a_locked_seat_is_announced_named_and_keeps_its_place_in_the_set() {
    // ★★★★★ The screen's whole claim, checked where it is cheapest. Measured at
    // 6.11.1 by building and running the same shape: a locked entry in an item
    // view and a locked destination in a tab bar come back `focusable,
    // selectable` and carry no unavailable state at all, so a reader there is
    // invited to activate exactly the seats the screen has closed.
    let tree = announced();
    for (n, entry) in spec::CATALOGUE.iter().enumerate() {
        let node = tree
            .get(&format!("shell.palette.{}", entry.kind))
            .unwrap_or_else(|| panic!("{} is not announced", entry.kind));
        assert_eq!(node.role, pinion_a11y::AriaRole::ListItem);
        assert_eq!(node.name.as_deref(), Some(entry.label));
        assert_eq!(
            node.position_in_set,
            Some(u32::try_from(n + 1).unwrap()),
            "{} keeps its place whether or not it is locked",
            entry.kind,
        );
        assert_eq!(node.size_of_set, Some(13));
    }
    let list = &tree["shell.palette"];
    assert_eq!(
        list.size_of_set,
        Some(u32::try_from(spec::CATALOGUE.len()).unwrap()),
        "the palette counts the locked seats among its entries",
    );
    // The reason itself is NOT restated by the builder: it is declared once on
    // the row's layout style and the assembler relays what the cascade
    // resolved. So the tree built in isolation carries no reason at all, and
    // the demo is what proves the relay end to end.
    assert!(
        tree.values().all(|node| node.unavailable.is_none()),
        "the screen states the reason once, on the scene, not twice",
    );
}

#[test]
fn a_value_that_is_not_knowable_is_announced_as_its_meaning() {
    // The map's unresolved row paints an em dash, which is the typographic
    // stand-in for a value nobody has — and to somebody reading rather than
    // looking it is a punctuation mark with no word in it.
    let tree = announced();
    let card = spec::card_of("keymap").expect("the opening board places the map");
    let last = spec::MAP_COLUMNS.len() - 1;
    assert_eq!(
        tree[&format!("card.{card}.cell.{}_{last}", spec::MAP_UNRESOLVED)]
            .name
            .as_deref(),
        Some("not known"),
    );
    // The negative half: a row that HAS a timestamp announces the timestamp.
    assert_eq!(
        tree[&format!("card.{card}.cell.0_{last}")].name.as_deref(),
        Some(spec::MAP_ROWS[0].2),
    );
}

#[test]
fn a_table_card_counts_its_header_row_in_both_the_count_and_the_indices() {
    // ★ WAI-ARIA counts the header row in `aria-rowcount`, so `aria-rowindex`
    // has to count it too: the header is row one and the last data row is the
    // row count. Held here as well as in the builder because this screen is
    // where the disagreement was found.
    let tree = announced();
    for kind in spec::TABLE_CARDS {
        let card = spec::card_of(kind).expect("the opening board places it");
        let grid = &tree[&format!("card.{card}.grid")];
        let head = &tree[&format!("card.{card}.head")];
        assert_eq!(head.row_index, Some(1), "{kind}: the header is row one");
        let rows = usize::try_from(grid.row_count.unwrap()).unwrap() - 1;
        let suffix = if *kind == "keymap" { "map" } else { "row" };
        let last = &tree[&format!("card.{card}.{suffix}.{}", rows - 1)];
        assert_eq!(
            last.row_index, grid.row_count,
            "{kind}: the last row IS the row count, so none is unreachable",
        );
    }
}

// ── R1721: the saved-filter bar ──────────────────────────────────────────────

/// ★★★★★ R1721 — **the filter card's chips are operable, and at most one is on.**
///
/// Both halves were absent, and the first one is the sharper: measured by driving
/// the running screen, the five chips announced `checked`, a pointer press over
/// every one of them changed nothing, and no keyboard reached any of them. A
/// control announced as operable that does nothing is the defect this tool keeps
/// reporting, in its own shell.
///
/// Pinning the BEHAVIOUR here is what keeps the declaration honest: change
/// `spec::FILTER_ROW` and this fails, rather than the accessibility census
/// quietly agreeing with whatever the rule became.
#[test]
fn r1721_the_filter_cards_chips_are_operable_and_at_most_one_is_on() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let opening = state.filter_chip.get();
        assert_eq!(
            opening,
            Some(0),
            "the card opens with the saved filter the specification lights"
        );
        super::ShellOracle::choose_filter(&state, "filter#3", 3);
        assert_eq!(
            state.filter_chip.get(),
            Some(3),
            "★ a chosen saved filter REPLACES the one that was on"
        );
        super::ShellOracle::choose_filter(&state, "filter#3", 3);
        assert_eq!(
            state.filter_chip.get(),
            None,
            "★ and choosing the one that is on empties the row"
        );
        assert_eq!(
            super::filter_row(&state, "filter#3").choice(),
            pinion_core::widgets::chip_group::Choice::AtMostOne,
            "the behaviour above IS the declared rule, not a coincidence"
        );
    });
}

/// ★★★★★ R1721 — **the accessibility tree reports the saved filter that is on**,
/// on this screen too.
///
/// Its sibling's counterfactual is what asked for this: replacing the live row
/// with an all-off one in the tree builder was caught by nothing in that crate's
/// suite, and the same hole was here. A tree read once and a bar painted live is
/// exactly the drift this round is about, one layer up.
#[test]
fn r1721_the_tree_reports_the_saved_filter_the_card_has_applied() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let selected = || -> Vec<bool> {
            super::filter_nodes(&state, "filter#3")
                .into_iter()
                .filter(|node| node.tag.starts_with("card.filter#3.chip."))
                .map(|node| node.selected == Some(true))
                .collect()
        };
        assert_eq!(
            selected(),
            vec![true, false, false, false, false],
            "the card opens announcing the saved filter the specification lights"
        );
        super::ShellOracle::choose_filter(&state, "filter#3", 2);
        assert_eq!(
            selected(),
            vec![false, false, true, false, false],
            "★ the option announced as selected MOVES with the choice"
        );
    });
}

// ── R1698: the cursor inside each composite ──────────────────────────────────

/// Press a key the way the SHELL does — through `WidgetCore::apply_key`, with
/// the focus manager's tag.
///
/// ★★★★★ Not through `ShellOracle::key_at`, which is what the first draft of
/// these gates drove: four counterfactuals passed against that draft, and every
/// one of them was this round's own headline defect. Deleting the `apply_key`
/// hook entirely — the exact state this screen was in before the round —
/// changed nothing in any of them, because none of them went through the door a
/// person's key comes in by. A gate that drives the layer BELOW the defect is
/// the shape R1693 named and this is its fourth recorded occurrence.
fn press_key(focused: Option<&str>, chord: &str) -> bool {
    let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
    AnalyzerShellView::apply_key(
        &mut scene,
        focused,
        chord,
        pinion_core::input::Modifiers::default(),
    )
}

/// ★★★★★ R1698 — **every composite the ring declares has a cursor its arrows
/// move, and the cursor is published.**
///
/// The half of WAI-ARIA's composite pattern R1696 left open. Measured on this
/// running screen the day the round started: five Tab stops, four arrow keys
/// each, forty-four presses and an active descendant that was `None` at every
/// one of them.
///
/// It drives `ShellOracle::key_at` — the same function the shell's `apply_key`
/// calls with the focus manager's tag — rather than writing a cursor, because a
/// cursor a test moves by hand proves nothing about a key press.
#[test]
fn r1698_every_declared_composite_has_a_cursor_its_arrows_move() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let mut checked = 0;
        for stop in spec::FOCUS_RING {
            let Some(declared) = stop.cursor else {
                continue;
            };
            let roving = state
                .cursor_of(stop.tag)
                .unwrap_or_else(|| panic!("{} declares a cursor and has none", stop.tag));
            assert_eq!(
                roving.spec(),
                declared,
                "{}'s policy is the declared one",
                stop.tag
            );
            assert!(
                roving.members().len() >= 2,
                "{} is a composite, so it holds more than one member",
                stop.tag
            );
            let first = roving
                .cursor_tag()
                .expect("a seated composite has a cursor")
                .to_owned();

            // The advancing key moves it; the retreating key brings it back.
            let keys = declared.axis.keys();
            assert!(press_key(Some(stop.tag), keys[0]));
            let moved = state
                .cursor_of(stop.tag)
                .and_then(|r| r.cursor_tag().map(str::to_owned))
                .expect("still seated");
            assert_ne!(moved, first, "{}: {} moved the cursor", stop.tag, keys[0]);
            let back = keys[keys.len() / 2];
            assert!(press_key(Some(stop.tag), back));
            assert_eq!(
                state
                    .cursor_of(stop.tag)
                    .and_then(|r| r.cursor_tag().map(str::to_owned)),
                Some(first.clone()),
                "{}: {back} brought it back",
                stop.tag
            );

            // Home and End reach the ends — the pair the reference toolkit's
            // tab list does not implement at all.
            assert!(press_key(Some(stop.tag), "End"));
            assert_eq!(
                state.cursor_of(stop.tag).and_then(|r| r.cursor()),
                Some(roving.members().len() - 1),
                "{}: End reaches the last member",
                stop.tag
            );
            assert!(press_key(Some(stop.tag), "Home"));
            assert_eq!(
                state.cursor_of(stop.tag).and_then(|r| r.cursor()),
                Some(0),
                "{}: Home reaches the first",
                stop.tag
            );
            checked += 1;
        }
        assert_eq!(
            checked,
            spec::FOCUS_RING
                .iter()
                .filter(|s| s.cursor.is_some())
                .count(),
            "every declared cursor was driven"
        );
        assert!(
            checked >= 4,
            "the ring declares four composites, not {checked}"
        );
    });
}

/// ★★★★★ R1698 — **an arrow from inside a composite does not reach the board.**
///
/// The defect the cursors created and a measurement caught: a key a composite
/// declines falls through, and what it used to fall through to was a global
/// handler that moved a card on the board the reader had left. Both halves are
/// asserted, because a screen that swallowed every arrow would satisfy the
/// first alone.
#[test]
fn r1698_an_arrow_from_inside_a_composite_does_not_reach_the_board() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let first = state.placed()[0].id().as_str().to_owned();
        state.selected.set(Some(first.clone()));

        // The rail is vertical, so ArrowDown is ITS key and never the board's.
        assert!(press_key(Some("shell.rail"), "ArrowDown"));
        assert_eq!(
            state.selected.get(),
            Some(first.clone()),
            "the rail's own arrow left the board's selection alone"
        );
        // And a key the rail declines does not reach the board either.
        assert!(!press_key(Some("shell.rail"), "ArrowRight"));
        assert_eq!(
            state.selected.get(),
            Some(first.clone()),
            "★ an arrow the rail declines must not move a card on a board the \
             reader is not in — this is what the fall-through used to do"
        );

        // With focus on the board it moves, and so does the wire's own channel.
        assert!(press_key(Some("shell.canvas"), "ArrowRight"));
        assert_ne!(
            state.selected.get(),
            Some(first.clone()),
            "the board's arrow moves the board"
        );
        state.selected.set(Some(first.clone()));
        assert!(press_key(None, "ArrowRight"));
        assert_ne!(
            state.selected.get(),
            Some(first),
            "and an agent driving the wire with nothing focused still reaches it"
        );
    });
}

/// ★★★ R1698 — **the tree publishes the cursor**, and publishes the roster the
/// arrows walk rather than the container's children.
///
/// The two are not the same list and the palette is the proof: its children are
/// three section groups and two status readouts while its cursor walks the
/// thirteen catalogue entries. A gate comparing `navigation` with `children`
/// would pass on a screen that published the wrong one.
#[test]
fn r1698_the_tree_publishes_the_cursor_and_the_roster_it_walks() {
    let owner = Owner::new();
    owner.run(|| {
        let _ = use_shell_state();
        let nodes = AnalyzerShellView::access_node(&ScreenState::default(), None);
        let by_tag: BTreeMap<&str, &pinion_a11y::AccessNode> =
            nodes.iter().map(|n| (n.tag.as_str(), n)).collect();

        for stop in spec::FOCUS_RING {
            let Some(declared) = stop.cursor else {
                continue;
            };
            let Some(node) = by_tag.get(stop.tag) else {
                continue; // not at this destination
            };
            let nav = node
                .navigation
                .as_ref()
                .unwrap_or_else(|| panic!("{} declares a cursor and publishes none", stop.tag));
            assert_eq!(
                nav.spec(),
                declared,
                "{} publishes its declared policy",
                stop.tag
            );
            assert_eq!(
                node.orientation,
                pinion_a11y::Orientation::of(declared.axis),
                "{} publishes the orientation its axis implies",
                stop.tag
            );
            // Every member is a node in the tree, or the active descendant
            // names something no reader can be told about.
            for member in nav.members() {
                assert!(
                    by_tag.contains_key(member.tag.as_str()),
                    "{}'s member {} is not in the tree",
                    stop.tag,
                    member.tag
                );
            }
        }

        // ★ The palette is where children and roster differ, so it is asserted
        // explicitly rather than only in the loop.
        let palette = by_tag["shell.palette"];
        let nav = palette
            .navigation
            .as_ref()
            .expect("the palette has a cursor");
        assert_eq!(
            nav.members().len(),
            spec::CATALOGUE.len(),
            "the cursor walks the catalogue entries"
        );
        assert_ne!(
            nav.members().len(),
            palette.children.len(),
            "★ and NOT the container's children, which are its sections and its \
             two status readouts — the distinction the floor loses"
        );
        // A locked entry stays in the roster: this screen's subject is that a
        // seat booked for a later release is shown rather than hidden.
        let locked = nav.members().iter().filter(|m| !m.enabled).count();
        assert_eq!(
            locked,
            spec::reserved_count(),
            "every reserved entry is reachable by the cursor and says it refuses"
        );
    });
}

/// ★★★★★ R1698 — **the focus target names the member the cursor is on.**
///
/// Demanded by a counterfactual that passed: replacing the composite focus
/// target with an atomic one — telling a reader the rail has focus and never
/// which destination, which is exactly the state this screen was in — left
/// every other gate here green, because none of them read the hook the
/// framework lowers to `aria-activedescendant` and frames the focus ring from.
#[test]
fn r1698_the_focus_target_names_the_member_the_cursor_is_on() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let mut checked = 0;
        for stop in spec::FOCUS_RING {
            let Some(declared) = stop.cursor else {
                continue;
            };
            let target =
                AnalyzerShellView::access_focus_target(&ScreenState::default(), Some(stop.tag))
                    .unwrap_or_else(|| panic!("{} owns the focus and reports no target", stop.tag));
            assert_eq!(
                target.focus_tag, stop.tag,
                "the AT focus stays on the composite"
            );
            let cursor = state
                .cursor_of(stop.tag)
                .and_then(|r| r.cursor_tag().map(str::to_owned));
            assert_eq!(
                target.active_descendant, cursor,
                "★ {} must name the member its arrows are on",
                stop.tag
            );
            assert!(
                target.active_descendant.is_some(),
                "{} has members, so it has a descendant to name",
                stop.tag
            );

            // And it FOLLOWS the arrows rather than being a value read once.
            let before = target.active_descendant.clone();
            assert!(press_key(Some(stop.tag), declared.axis.keys()[0]));
            let after =
                AnalyzerShellView::access_focus_target(&ScreenState::default(), Some(stop.tag))
                    .and_then(|t| t.active_descendant);
            assert_ne!(
                before, after,
                "{}: the descendant moved with the cursor",
                stop.tag
            );
            checked += 1;
        }
        assert!(checked >= 4, "four composites were checked, not {checked}");

        // The board declares no roster and still names the card it is on, so a
        // reader landing there is told which one.
        let first = state.placed()[0].id().as_str().to_owned();
        state.selected.set(Some(first.clone()));
        let target =
            AnalyzerShellView::access_focus_target(&ScreenState::default(), Some("shell.canvas"))
                .expect("the board reports a target");
        assert_eq!(target.active_descendant, Some(format!("card.{first}")));
    });
}

/// ★★★ R1698 — every stop this screen declares chooses **explicitly**.
///
/// The assertion that keeps `Landing::choose` from being a bit nobody reads
/// here: if a stop is ever declared `Follows`, this fails and whoever declared
/// it has to write the arm. The other arm's real consumer is the capture
/// viewer's message list, where the cursor IS the selection.
#[test]
fn r1698_no_stop_on_this_screen_chooses_by_arriving() {
    for stop in spec::FOCUS_RING {
        if let Some(cursor) = stop.cursor {
            assert_eq!(
                cursor.activation,
                spec::Activation::Explicit,
                "{} arrives without choosing",
                stop.tag
            );
        }
    }
}

// ── R1699: the cursor can act, and it can go inside ──────────────────────────

/// Every tag any composite's cursor can rest on, nested rosters included, each
/// paired with whether it is itself a composite.
///
/// Built from the live cursors rather than written down: a member added to a
/// roster joins the gates below on the next paint, which is exactly what a
/// hand-kept list loses (R1687's eighth seat).
fn all_cursor_members(state: &std::rc::Rc<super::ShellState>) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for stop in spec::FOCUS_RING {
        let Some(roving) = state.cursor_of(stop.tag) else {
            continue;
        };
        for member in roving.members() {
            out.push((member.tag.clone(), member.is_composite()));
            if let Some(inner) = member.inner() {
                for nested in inner.members() {
                    out.push((nested.tag.clone(), nested.is_composite()));
                }
            }
        }
    }
    out
}

/// ★★★★★ R1699 — **the tag a cursor names and the tag a press lands on are the
/// same thing.**
///
/// A keyboard activation is semantic — the reader named a member, not a pixel —
/// so this screen resolves it with `Hit::of_tag`, a second address space beside
/// `Hit::at`. Two address spaces drift, so the gate came before the function
/// and checks both directions against a third party: `hit_word` (which the
/// pointer path already had) must round-trip, and the answer must equal what a
/// press at the centre of the tag's **painted** rectangle produces.
///
/// A member that is itself a composite is exempt from the press half and only
/// from that half: the tab list paints nothing — it is anchored by the tabs it
/// composes — so there is no rectangle to press, which is precisely why it is
/// entered rather than chosen.
#[test]
fn r1699_every_cursor_member_resolves_to_the_hit_its_tag_names() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
        let rects = scene.absolute_rects_by_tag();

        let members = all_cursor_members(&state);
        assert!(
            members.len() >= 25,
            "the ring's rosters are smaller than the screen has: {}",
            members.len()
        );
        let mut wrong = Vec::new();
        let mut pressed = 0;
        for (tag, composite) in &members {
            if *composite {
                assert_eq!(
                    super::Hit::of_tag(&state, tag),
                    super::Hit::Nothing,
                    "{tag} is a composite; it is entered, not chosen"
                );
                continue;
            }
            let hit = super::Hit::of_tag(&state, tag);
            let round_trip = super::hit_word(&hit);
            if round_trip != *tag {
                wrong.push(format!("{tag}: of_tag round-trips to {round_trip}"));
                continue;
            }
            let Some(rect) = rects.get(tag).copied() else {
                wrong.push(format!("{tag}: a cursor rests here and nothing paints it"));
                continue;
            };
            let at = super::Hit::at(&state, rect.x + rect.w / 2, rect.y + rect.h / 2);
            if at != hit {
                wrong.push(format!(
                    "{tag}: a press at its centre answers {}, a key answers {}",
                    super::hit_word(&at),
                    round_trip
                ));
            }
            pressed += 1;
        }
        assert!(
            wrong.is_empty(),
            "{} member(s):\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );
        assert!(
            pressed >= 24,
            "only {pressed} member(s) were checked against the paint"
        );
    });
}

/// ★★★★★ R1699 — **`Enter` at every cursor position does something.**
///
/// The measurement that opened the round, as an assertion. Every composite here
/// declares `Activation::Explicit`, whose documented meaning is "arriving only
/// moves the cursor; `Enter` or `Space` chooses" — and until this round nothing
/// anywhere implemented the second half. Driven on the running screen before
/// the fix: four composites, three chords each, twelve presses, the destination
/// unchanged and the toast still reading what the previous *arrow* had put
/// there.
///
/// "Does something" is deliberately the weakest claim that is still false of
/// the old screen, because the strong version is per-member domain knowledge
/// this gate must not restate. What it asserts is that the key is CONSUMED and
/// that **a reader can tell**: either the painted screen changed or the screen
/// said something new.
///
/// ★★★★★ Both halves are load-bearing and the first draft had only the second,
/// which is R1695's own lesson walked into again — a gate that watches a
/// VARIABLE rather than the screen. It reported the layout-preset button as
/// silent: choosing it opens a menu, which paints rows and flips the button's
/// `aria-expanded`, and that IS the announcement WAI-ARIA specifies for opening
/// a menu. A toast there would have been a second, weaker one written to
/// satisfy a test. The other half is equally necessary: a booked palette entry
/// REFUSES, which changes nothing painted at all, and hearing the reason is the
/// whole of what happens.
#[test]
fn r1699_choosing_a_member_from_the_keyboard_does_something() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let screen = || {
            let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
            let mut cache = pinion_runtime::LayoutCache::new();
            pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
            let mut tags: Vec<(String, (u32, u32, u32, u32))> = scene
                .absolute_rects_by_tag()
                .into_iter()
                .map(|(tag, r)| (tag, (r.x, r.y, r.w, r.h)))
                .collect();
            tags.sort();
            (tags, state.toast.get())
        };
        let mut silent = Vec::new();
        let mut checked = 0;
        let mut by_paint = 0;
        let mut by_word = 0;
        for stop in spec::FOCUS_RING {
            // ★ Drive each stop at a destination that SHOWS it. The first draft
            // did not, and the rail's own members are what moved the journey:
            // by the time the loop reached the layout bar the screen was at
            // another destination, where that bar is not painted and choosing
            // its menu could not paint one either. A gate that drives a control
            // the screen is not showing is asking a question with no answer.
            if let spec::Where::At(destination) = stop.at {
                assert!(
                    state.go(destination).is_ok(),
                    "{} lives at {destination}, which the rail must reach",
                    stop.tag
                );
            }
            let Some(roving) = state.cursor_of(stop.tag) else {
                continue;
            };
            for (index, member) in roving.members().iter().enumerate() {
                if member.is_composite() {
                    continue;
                }
                // Walk the cursor onto this member through the keyboard rather
                // than writing it: a cursor a test moves by hand proves nothing
                // about a key press.
                assert!(press_key(Some(stop.tag), "Home"));
                for _ in 0..index {
                    press_key(Some(stop.tag), roving.spec().axis.keys()[0]);
                }
                assert_eq!(
                    state
                        .cursor_of(stop.tag)
                        .and_then(|r| r.active_descendant().map(str::to_owned)),
                    Some(member.tag.clone()),
                    "the walk did not reach {}",
                    member.tag
                );
                let (painted_before, said_before) = screen();
                let consumed = press_key(Some(stop.tag), "Enter");
                let (painted_after, said_after) = screen();
                let repainted = painted_after != painted_before;
                let spoke = said_after != said_before;
                if !consumed || !(repainted || spoke) {
                    silent.push(format!(
                        "{} \u{00B7} {}: consumed={consumed} toast={said_after:?}",
                        stop.tag, member.tag
                    ));
                }
                by_paint += usize::from(repainted);
                by_word += usize::from(spoke && !repainted);
                checked += 1;
            }
        }
        assert!(
            silent.is_empty(),
            "{} member(s) did nothing a reader could tell:\n  {}",
            silent.len(),
            silent.join("\n  ")
        );
        assert!(checked >= 24, "only {checked} member(s) were chosen");
        // Neither half is decoration: if one of these ever reaches zero the gate
        // has quietly become half of itself.
        assert!(by_paint > 0, "no member's choice repainted anything");
        assert!(by_word > 0, "no member's choice was heard rather than seen");
    });
}

/// ★★★★★ R1699 — **the application bar's tab list is entered, walked and left.**
///
/// Measured the day the round opened by driving the running window: the bar's
/// cursor reached `shell.appbar.tabs` in one step (WAI-ARIA's nesting, and
/// correct) and `ArrowDown`, `ArrowUp`, `Enter` and `Space` there all left the
/// active descendant exactly where it was. From a keyboard the two views could
/// not be switched at all.
///
/// The floor, measured by building a probe at 6.11.1 and running it offscreen:
/// the same arrangement is **four** Tab stops rather than one, an arrow from a
/// bar control moves *focus* into the tab bar while the opposite arrow walks
/// straight out of the bar entirely, and `Escape` does nothing anywhere.
#[test]
fn r1699_the_nested_tab_list_is_entered_walked_and_left() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let descendant = || {
            AnalyzerShellView::access_focus_target(&ScreenState::default(), Some("shell.appbar"))
                .and_then(|t| t.active_descendant)
        };
        assert!(press_key(Some("shell.appbar"), "Home"));
        assert_eq!(descendant(), Some(super::APP_BAR_TABS.to_owned()));

        // An off-axis arrow enters, because THIS member is a composite.
        assert!(press_key(Some("shell.appbar"), "ArrowDown"));
        let inside = descendant().expect("the cursor is on a tab");
        assert_eq!(
            inside,
            BarChip::Tab0.tag(),
            "\u{2605} entering lands on the tab list's own cursor"
        );
        assert_ne!(
            inside,
            super::APP_BAR_TABS,
            "and the active descendant is the innermost tag, not the list"
        );

        // The inner axis answers and the outer one does not move.
        assert!(press_key(Some("shell.appbar"), "ArrowRight"));
        assert_eq!(descendant(), Some(BarChip::Tab1.tag().to_owned()));
        assert_eq!(
            state
                .cursor_of("shell.appbar")
                .and_then(|r| r.cursor_tag().map(str::to_owned)),
            Some(super::APP_BAR_TABS.to_owned()),
            "the bar's own cursor stayed on the tab list while the reader was inside"
        );

        // Choosing switches the view — the thing a keyboard could not do.
        let before = state.tab.get();
        assert!(press_key(Some("shell.appbar"), "Enter"));
        let after = state.tab.get();
        assert_ne!(
            before, after,
            "\u{2605} Enter inside the tab list switched the view"
        );
        assert_eq!(after, TABS[1], "to the tab the cursor was on");

        // Escape leaves ONE level: back onto the tab list, still in the bar.
        assert!(press_key(Some("shell.appbar"), "Escape"));
        assert_eq!(descendant(), Some(super::APP_BAR_TABS.to_owned()));
        assert!(
            press_key(Some("shell.appbar"), "ArrowRight"),
            "and the bar's own axis answers again"
        );
        assert_eq!(descendant(), Some(BarChip::Source.tag().to_owned()));

        // ★ The narrowing R1699 had to make to R1698's invariant: the off-axis
        // arrow is consumed ONLY where there is something to enter.
        assert!(
            !press_key(Some("shell.appbar"), "ArrowDown"),
            "the source chip is not a composite, so ArrowDown still falls through"
        );
    });
}

/// ★★★ R1699 — a nested composite publishes its own roster **before** anybody
/// descends into it.
///
/// The point of publishing is to be askable without pressing a key. A roster
/// that appeared only once the cursor was inside would make a client's answer
/// depend on where somebody happened to be standing.
#[test]
fn r1699_the_nested_composite_publishes_its_roster_unentered() {
    let owner = Owner::new();
    owner.run(|| {
        let _ = use_shell_state();
        let nodes = AnalyzerShellView::access_node(&ScreenState::default(), None);
        let by_tag: BTreeMap<&str, &pinion_a11y::AccessNode> =
            nodes.iter().map(|n| (n.tag.as_str(), n)).collect();

        let bar = by_tag["shell.appbar"];
        let bar_nav = bar.navigation.as_ref().expect("the bar has a cursor");
        assert!(
            !bar_nav.entered(),
            "nobody has descended, and the wire says so"
        );
        let nested: Vec<&str> = bar_nav
            .members()
            .iter()
            .filter(|m| m.is_composite())
            .map(|m| m.tag.as_str())
            .collect();
        assert_eq!(
            nested,
            vec![super::APP_BAR_TABS],
            "the bar has exactly one member that is itself a composite"
        );

        let tabs = by_tag[super::APP_BAR_TABS];
        let tabs_nav = tabs
            .navigation
            .as_ref()
            .expect("\u{2605} a nested composite publishes what its own arrows reach");
        assert_eq!(tabs_nav.members().len(), TABS.len());
        assert_eq!(
            tabs.orientation,
            pinion_a11y::Orientation::of(tabs_nav.spec().axis),
            "and the orientation its axis implies"
        );
        for member in tabs_nav.members() {
            assert!(
                by_tag.contains_key(member.tag.as_str()),
                "{} is not a node a reader can be told about",
                member.tag
            );
        }
    });
}

/// Drive the screen the way a pointer does — through `invoke`, which is the door
/// the router's own events come in by.
///
/// ★ R1698's lesson, applied: a gate that calls the inner function proves the
/// inner function. `send` is where a press, a release and a double click all
/// arrive, so that is what this presses.
fn send(oracle: &mut super::ShellOracle, event: &str) {
    use pinion_core::external::{ExternalIntrospect, IntrospectValue};
    oracle
        .invoke("send", IntrospectValue::Text(event.to_owned()))
        .expect("the screen accepts the pointer events its router sends");
}

fn point(oracle: &mut super::ShellOracle, x: u32, y: u32) {
    use pinion_core::external::{ExternalIntrospect, IntrospectValue};
    oracle
        .invoke("point", IntrospectValue::Text(format!("{x},{y}")))
        .expect("the cursor can be put where the paint put a control");
}

/// The middle of the grip a card is dragged by, read out of the PAINT.
///
/// ★★ Not computed from `cell_rect`, which the first draft did and which aimed
/// at the application bar: those rectangles are in the board's own frame and a
/// press is resolved in the window's. The rule this project has already
/// recorded twice — take a press point from the painted scene, never from
/// arithmetic beside the thing under test — is the rule here too.
fn grip_centre(n: usize) -> (String, u32, u32) {
    let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
    let mut cache = pinion_runtime::LayoutCache::new();
    pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
    let rects = scene.absolute_rects_by_tag();
    // The last segment, not a suffix: `ends_with(".grip")` reads to clippy as a
    // file-extension comparison, and a tag is not a path.
    let mut grips: Vec<String> = rects
        .keys()
        .filter(|t| t.starts_with("card.") && t.split('.').next_back() == Some("grip"))
        .cloned()
        .collect();
    grips.sort();
    assert!(
        grips.len() > n,
        "the board paints at least {} card grip(s), it paints {}",
        n + 1,
        grips.len()
    );
    let tag = grips.swap_remove(n);
    let rect = rects[&tag];
    (tag, rect.x + rect.w / 2, rect.y + rect.h / 2)
}

/// What the wire answers for `layout` — the arrangement, as a string two
/// moments can be compared by.
fn board_layout(state: &std::rc::Rc<super::ShellState>) -> String {
    serde_json::to_string(&state.board.get()).expect("a board serialises")
}

/// ★★★★★ R1701 — **two clicks on a card's header toggle it between its size on
/// the board and the whole board, and carry nothing else with them.**
///
/// Reported by a person. The behaviour reference cannot settle it — it is a
/// browser prototype with no window chrome, and its 194,828 bytes of
/// application script contain zero double-click handlers — so the floor does:
/// built and run offscreen at 6.11, an in-application sub-window's title-bar
/// double-click takes it from 300x200 to its parent's full 900x600.
///
/// The second assertion is the one driving found: a grip press OPENS A BOARD
/// DRAG, so before the repair the trailing release committed a move aimed at
/// the board that existed before the card grew — "Decode Inspector moved,
/// displacing Message Stream, Identifier Map, Search & Filter", and a second
/// double-click never came back to the arrangement the screen opened with.
#[test]
fn r1701_a_double_click_on_a_card_header_toggles_maximise_and_moves_nothing() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let mut oracle = super::ShellOracle::new();
        oracle.attach_state(state.clone());

        let opened = board_layout(&state);
        let (tag, ax, ay) = grip_centre(1);
        point(&mut oracle, ax, ay);
        assert!(
            matches!(super::Hit::at(&state, ax, ay), super::Hit::Grip(_)),
            "{tag} at ({ax},{ay}) is the card's header, which is what a title bar is here"
        );

        // The gesture, as the router delivers it: two press/release pairs with
        // the `DoubleClick` the router synthesises on the second press.
        send(&mut oracle, "PointerDown");
        send(&mut oracle, "PointerUp");
        send(&mut oracle, "PointerDown");
        send(&mut oracle, "DoubleClick");
        send(&mut oracle, "PointerUp");

        assert!(
            state.maximized.get().is_some(),
            "\u{2605} a double-click on the header maximises the card"
        );
        let grown = board_layout(&state);

        // And back, which is the half a toggle is only half of — and the half
        // that runs with the board in its maximised arrangement. The aim is
        // read AGAIN, because a maximised card is not where it was.
        let (_, bx, by) = grip_centre(0);
        point(&mut oracle, bx, by);
        send(&mut oracle, "PointerDown");
        send(&mut oracle, "PointerUp");
        send(&mut oracle, "PointerDown");
        send(&mut oracle, "DoubleClick");
        send(&mut oracle, "PointerUp");

        assert!(
            state.maximized.get().is_none(),
            "\u{2605} and a second double-click restores it"
        );
        assert_ne!(
            grown, opened,
            "the maximised board is a different arrangement, or nothing was proven"
        );
        assert_eq!(
            board_layout(&state),
            opened,
            "\u{2605}\u{2605} and the board is EXACTLY the arrangement it opened with \u{2014} \
             the trailing release of a double-click carries no move"
        );
    });
}

/// ★★★★★ R1701 — **a press that carried nothing announces nothing**, on the
/// board arm of the release path.
///
/// R1697 wrote this rule and built it for a detached panel: "nothing checked
/// that a press which moved nothing does not announce a move". The arm ten
/// lines below it, for a card on the board, told the same lie — measured on the
/// running screen, a single click on a header said "Decode Inspector moved"
/// with the layout byte-identical — and worse, `move_to` is a real edit, so a
/// click carrying nothing could still reflow the board.
#[test]
fn r1701_a_click_on_a_header_that_moved_nothing_says_nothing() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let mut oracle = super::ShellOracle::new();
        oracle.attach_state(state.clone());

        let opened = board_layout(&state);
        let said = state.toast.get();
        let (_, ax, ay) = grip_centre(0);

        point(&mut oracle, ax, ay);
        send(&mut oracle, "PointerDown");
        send(&mut oracle, "PointerUp");

        assert_eq!(
            board_layout(&state),
            opened,
            "a click that carried nothing leaves the board alone"
        );
        assert_eq!(
            state.toast.get(),
            said,
            "\u{2605} and says nothing, because there is nothing to say"
        );
    });
}

// --- R1724: the tool is one application --------------------------------------
//
// These are the shell's half of `pinion-screen`'s guarantees. That crate proves
// them against fixtures; these prove that THIS application is wired to them —
// which is a different claim, and the one the integration debt is about.

/// The tags in a scene, so a claim about "what the window shows" is a set
/// rather than a walk written four times.
///
/// ★ Not `painted::Painted::of(..).tags`, and the difference is load-bearing:
/// that one keys tags by their absolute RECTANGLE and therefore skips every
/// node the layout pass has not placed. These tests assert about the scene the
/// view returns, without running layout, because what they are about is which
/// screen composed it. Unifying the two would make this silently empty.
fn painted_tags(scene: &pinion_core::Scene) -> std::collections::BTreeSet<String> {
    let mut tags = std::collections::BTreeSet::new();
    scene.for_each_node(&mut |visit| {
        if let Some(tag) = visit.node.tag() {
            tags.insert(tag.to_owned());
        }
    });
    tags
}

fn lab_tags(tags: &std::collections::BTreeSet<String>) -> Vec<String> {
    tags.iter()
        .filter(|t| t.as_str() == "node_lab" || t.starts_with("lab."))
        .cloned()
        .collect()
}

/// ★★★★★ R1724 — **arriving at the node lab seat shows the node graph lab.**
///
/// The seat said `elsewhere` — *built, shipping, and not here* — for as long as
/// the tool was three executables. It is here now, and this is what that means:
/// the same binding the standalone `hello-node-lab` binary runs paints inside
/// this window's page region.
///
/// ★ R1728 renamed the seat from `catalog` to `lab`: the reference has a node
/// graph section and has no catalogue section, so the page was right and the
/// address was this application's invention.
#[test]
fn r1724_the_lab_destination_is_the_node_lab_itself() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();

        let dashboard = painted_tags(&super::view(
            ScreenState::default(),
            pinion_core::Frame::default(),
        ));
        assert_eq!(
            lab_tags(&dashboard),
            Vec::<String>::new(),
            "no part of the lab is anywhere on the dashboard"
        );

        state
            .go("lab")
            .expect("the node lab is a destination we can reach");
        let catalog = painted_tags(&super::view(
            ScreenState::default(),
            pinion_core::Frame::default(),
        ));
        let lab = lab_tags(&catalog);
        assert!(
            catalog.contains("node_lab"),
            "the lab's own paint root is in the window"
        );
        for pane in ["lab.palette", "lab.canvas", "lab.inspector"] {
            assert!(
                lab.iter().any(|t| t == pane),
                "the lab's {pane} pane is painted"
            );
        }
        assert!(
            lab.len() > 40,
            "a whole screen arrived, not a placeholder: {} region(s)",
            lab.len()
        );
        // And the application is still the application.
        for chrome in ["shell.appbar", "shell.rail", "shell.canvas"] {
            assert!(
                catalog.contains(chrome),
                "the shell's {chrome} is still painted"
            );
        }
    });
}

/// ★★★★★ R1724 — **a press inside the section reaches the section.**
///
/// The property no gate in this tree had, and the one that was false while
/// every other gate was green. Measured the day the lab was first mounted: it
/// painted 139 tagged regions, answered every path on the wire, appeared in
/// the accessibility tree, and `scene/pointer_reach` reported it `routed_by:
/// node_lab` — and not one press anywhere inside it reached anything, the
/// screen or the host.
///
/// Every one of those checks was true and none of them was this one. They ask
/// what a painted rectangle RESOLVES to; this asks whether the hit test
/// descends that far, which is a question one layer below them.
#[test]
fn r1724_a_press_inside_the_mounted_section_resolves_to_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        state.go("lab").expect("the node lab seat is reachable");
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);

        // ★★★★★ The ROUTER's own resolution, not a copy of it. The first draft
        // of this test wrote the four-line walk out here, and then did not see
        // the repair it was written to check — a hit test spelled twice is two
        // hit tests, which is the R47 class in its smallest possible form.
        let resolves_at = |x: u32, y: u32| -> Option<String> {
            pinion_runtime::resolve_pointer_tag(&scene, f64::from(x), f64::from(y))
        };

        // ★ The card's rectangle is stated in the PAGE's frame, not the
        // window's: the pan viewport gives its content its own layout pass, so
        // a rectangle inside it counts from the page's origin. Converting is
        // the caller's job and getting it wrong is how the first draft of this
        // test pressed 22 pixels above the card it named.
        let page = pinion_runtime::rect_for_tag(&scene, "window.pan")
            .expect("the mounted section pans inside its region");
        let card = pinion_runtime::rect_for_tag(&scene, "lab.node.P-02")
            .expect("the mounted lab paints its node cards");
        let inside = (page.x + card.x + card.w / 2, page.y + card.y + card.h / 2);
        let landed = resolves_at(inside.0, inside.1);
        assert_eq!(
            landed.as_deref(),
            Some("node_lab"),
            "a press at ({}, {}) -- the centre of a card the mounted screen \
             painted -- must resolve to that screen's own surface. It is the \
             ONE external the lab registers (R1655), so this is what carries \
             every gesture the section has",
            inside.0,
            inside.1,
        );

        // And the host still owns its own chrome, so the two do not fight.
        let seat = pinion_runtime::rect_for_tag(&scene, "shell.rail.dashboard")
            .expect("the rail is painted");
        assert_eq!(
            resolves_at(seat.x + seat.w / 2, seat.y + seat.h / 2).as_deref(),
            Some(super::VIEW_TAG),
            "a press on the shell's rail is the shell's, with a whole other \
             screen showing beside it",
        );
    });
}

/// ★★★★★ R1724 — **the surfaces are the showing screen's, and nobody else's.**
///
/// Measured at 6.11.1 by building a probe and running it: a page of the
/// reference toolkit's paged container that is not showing, sent a press, a key
/// and a wheel, counted all three. Here the externals of a screen the journey
/// is not at are not in the set the shell hands the framework, so there is
/// nothing for the router to resolve a press to and no slot for the wire.
#[test]
fn r1724_only_the_showing_section_has_surfaces() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let tags = || {
            super::AnalyzerShellView::create_extra_externals()
                .iter()
                .map(|e| e.tag.clone().into_owned())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            tags(),
            Vec::<String>::new(),
            "the dashboard is this application's own page, so it mounts nothing"
        );

        state.go("lab").expect("the node lab seat is reachable");
        let live = tags();
        assert!(
            live.iter().any(|t| t == "node_lab"),
            "the lab's own surface is live while the lab is showing: {live:?}"
        );

        state.go("dashboard").expect("the dashboard is reachable");
        assert_eq!(
            tags(),
            Vec::<String>::new(),
            "and it is gone the moment the rail takes you elsewhere"
        );
    });
}

/// ★★★★★ R1726 — **the card you are dragging is on top, and the slot it would
/// land in is underneath.**
///
/// The owner's report, driven: *"while dragging, the widget's interior is just
/// grey"*. It was not — the widget kept every row it had. The snap preview was
/// filled opaque and pushed AFTER the cards, so it covered the card whole.
///
/// Both halves are asserted because either alone leaves the symptom: a preview
/// drawn behind a card that is not itself lifted still ends up under its
/// neighbours, and a lifted card under an opaque preview is still hidden.
/// Asserted over the paint ORDER, since that is the z-order here.
#[test]
fn r1726_the_dragged_card_is_above_the_slot_it_would_land_in() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let board = state.board.get();
        let dragged = state
            .placed()
            .first()
            .map(|c| c.id().clone())
            .expect("the board opens with cards");
        let tile = board.tile(&dragged).expect("the card is on the board");
        state.drag.set(Some(super::Drag {
            id: dragged.clone(),
            dx: 0,
            dy: 0,
            snap: (tile.col, tile.row),
        }));

        let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let tags = scene.tags();
        let at = |want: &str| tags.iter().position(|t| t == want);

        let slot = at("shell.dropslot").expect("the snap preview is painted while dragging");
        let card = at(&format!("card.{dragged}")).expect("the dragged card is painted");
        let others: Vec<usize> = state
            .placed()
            .iter()
            .filter(|c| *c.id() != dragged)
            .filter_map(|c| at(&format!("card.{}", c.id())))
            .collect();

        // ★★★★★ THREE layers, and each boundary was a measured defect.
        //
        // The preview above the resting cards: below them it hides behind
        // whatever already occupies the target cell, which is every drag ONTO
        // another widget — a destination you cannot see reads as the thing you
        // are holding having vanished. Measured with a real pointer: the slot
        // landed at (518,114) squarely under a neighbour and was invisible.
        assert!(
            others.iter().all(|other| *other < slot),
            "the snap preview paints ABOVE the resting cards, or the \
             destination disappears under whichever card already sits there: \
             slot {slot} against {others:?}"
        );
        // And the held card above the preview: below it, the opaque slot covers
        // the widget whole, which is what was reported as its interior going
        // grey.
        assert!(
            slot < card,
            "the card being dragged paints above the preview, or the opaque \
             slot covers its whole body: slot {slot}, card {card}"
        );
    });
}

/// ★★★★★ R1726 — **a press after scrolling lands on the cell it looks like.**
///
/// Reported from the running board: scroll down, press a widget, and the drop
/// position is not where the widget is. `Hit::at` has folded the board's scroll
/// offset since R1662 — which is why the press still *selects* the right card —
/// and the two places that turned a press into a CELL did not, so the grab and
/// the destination were computed in the unscrolled frame.
///
/// Driven as a difference rather than as an absolute: the same window point,
/// scrolled and unscrolled, must name cells that differ by the scroll. An
/// assertion on one cell number would pass for a function that ignores the
/// offset whenever the offset happens to be zero.
#[test]
fn r1726_a_press_after_scrolling_names_the_cell_under_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let canvas = super::canvas_rect();
        let point = (canvas.x + 40, canvas.y + super::ROW_H * 3);

        let unscrolled = super::cell_at_window(&state, point.0, point.1);
        // The offset is clamped into `[0, max]`, and a board whose content fits
        // has a max of zero — so the range has to exist before the board can
        // slide. Without this the scroll silently stays at 0 and the test
        // passes for a function that ignores it.
        let row_h = i32::try_from(super::ROW_H).unwrap_or(1);
        state.canvas_scroll.set_max(0, row_h * 4);
        state.canvas_scroll.scroll_by(0, row_h * 2);
        assert_eq!(
            state.canvas_scroll.offset().1,
            row_h * 2,
            "the board really did slide, or the rest of this proves nothing"
        );

        let scrolled = super::cell_at_window(&state, point.0, point.1);
        assert_ne!(
            unscrolled, scrolled,
            "the same window point names a different cell once the board has \
             slid under it; equal means the scroll was not folded in, which is \
             the defect -- the press selects one card and the drag computes \
             another"
        );
        assert_eq!(
            scrolled.1,
            unscrolled.1 + 2,
            "and it differs BY the scroll: two rows further down the board"
        );
    });
}

/// ★★★★★ R1726 — **the drop preview is a mark, not a surface.**
///
/// An opaque preview has no correct layer and both were driven with a real
/// pointer: under the cards it hides behind whatever occupies the destination
/// (so the drag has no visible target), over them it hides the widget standing
/// there (reported as "the widget goes grey"). Translucent, it can sit above
/// the board and cover nothing.
#[test]
fn r1726_the_drop_preview_covers_nothing() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let dragged = state
            .placed()
            .first()
            .map(|c| c.id().clone())
            .expect("the board opens with cards");
        let board = state.board.get();
        let tile = board.tile(&dragged).expect("on the board");
        state.drag.set(Some(super::Drag {
            id: dragged,
            dx: 0,
            dy: 0,
            snap: (tile.col, tile.row),
        }));

        let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let fill = find_fill(&scene, "shell.dropslot").expect("the preview is painted");
        assert!(
            fill.a < 0x80,
            "the preview must be translucent so what is under it stays \
             readable; alpha {} is a surface, not a mark",
            fill.a
        );
        assert!(fill.a > 0, "and it is a mark rather than nothing at all");
    });
}

/// ★★★★★ R1726 — **the cursor carries the name of what it is carrying.**
///
/// The behaviour reference keeps three things during a board drag: the widget
/// stays put, a snap mark shows the destination, and a chip rides the cursor
/// with the widget's NAME. This tree had the first two, so the gesture never
/// said what was being carried.
#[test]
fn r1726_the_cursor_carries_the_name_of_what_it_holds() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        assert!(
            !scene.tags().iter().any(|t| t == "shell.carried"),
            "nothing is carried when nothing is being dragged"
        );

        let dragged = state
            .placed()
            .first()
            .map(|c| c.id().clone())
            .expect("the board opens with cards");
        let board = state.board.get();
        let tile = board.tile(&dragged).expect("on the board");
        state.cursor.set((640, 400));
        state.drag.set(Some(super::Drag {
            id: dragged.clone(),
            dx: 0,
            dy: 0,
            snap: (tile.col, tile.row),
        }));

        let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let tags = scene.tags();
        assert!(
            tags.iter().any(|t| t == "shell.carried"),
            "a card being carried puts its name on the cursor"
        );
        // ★ Above the BOARD and everything on it — not last in the window,
        // which the palette owns. The first draft asserted last-in-the-scene
        // and failed against `shell.palette.reserved`, which is a true fact
        // about a different plane: this chip's place is the pointer's within
        // the page, and the page is not the whole window.
        let carried = tags
            .iter()
            .position(|t| t == "shell.carried")
            .expect("the chip is painted");
        let board_things: Vec<usize> = tags
            .iter()
            .enumerate()
            .filter(|(_, t)| t.starts_with("card.") || *t == "shell.dropslot")
            .map(|(i, _)| i)
            .collect();
        assert!(
            board_things.iter().all(|at| *at < carried),
            "the chip paints after every card and after the drop preview: \
             {carried} against {board_things:?}"
        );
    });
}

/// The fill colour of the first node carrying `tag`.
fn find_fill(scene: &pinion_core::Scene, tag: &str) -> Option<pinion_core::style::Color> {
    let mut found = None;
    scene.for_each_node(&mut |visit| {
        if found.is_none() && visit.node.tag() == Some(tag) {
            found = visit.node.box_style().map(|s| s.fill);
        }
    });
    found
}

/// ★★★★★ R1725 — **one application, one navigation.**
///
/// The defect this pins was visible the moment the first screen was mounted and
/// no gate could see it: at Catalog the shell's rail ran x=0..52 and the mounted
/// screen painted its own at x=52..106 — two rails side by side — and the
/// accessibility tree published **both**, `role=navigation`, named *Destinations*
/// and *sections*.
///
/// Measured at 6.11.1 by building a probe and running it: a complete
/// application window placed inside another application's page container keeps
/// its menu bar (23 px of it), its tool bar and its status bar, and its tree
/// carries **2 of each**. There is no property, method or event by which the
/// placed window could learn what its container already provides — the nearest
/// signal is `window()`, which answers the *host's* window, so a guest can
/// learn that it is embedded and nothing about what that place has.
///
/// Both halves are asserted because they fail differently: paint-only would
/// leave a landmark a screen reader walks to and a pointer cannot reach.
#[test]
fn r1725_one_application_has_one_navigation() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        state.go("lab").expect("the node lab seat is reachable");

        // --- the tree ---------------------------------------------------
        let nodes = super::AnalyzerShellView::access_node(&ScreenState::default(), None);
        let navigations: Vec<&str> = nodes
            .iter()
            .filter(|n| n.role == pinion_a11y::AriaRole::Navigation)
            .map(|n| n.tag.as_str())
            .collect();
        assert_eq!(
            navigations,
            vec!["shell.rail"],
            "a reader must be told this application has ONE navigation. Before \
             R1725 the mounted screen contributed a second one named `sections` \
             beside the shell's `Destinations`, which is what the reference \
             toolkit does by construction (2 menu bars, 2 tool bars and 2 \
             status bars in its tree, measured at 6.11.1)"
        );

        // --- the picture ------------------------------------------------
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
        // ★★★★★ Asked of the scene's TAGS and not of a rectangle, and a
        // counterfactual is what said so. `rect_for_tag` answers `None` both
        // for a tag no node carries AND for a node whose rect does not resolve
        // — so the first draft of this assertion could not tell "not built"
        // from "built at zero width", which is exactly the distinction this
        // round's design turns on. Restoring the unconditional rail was caught
        // by NOTHING, because the width still came from the same predicate and
        // the node was painted zero-wide: the mechanism was half broken and the
        // gate could not see it. Presence is the property; geometry is a
        // consequence.
        let tags = scene.tags();
        assert!(
            tags.iter().any(|t| t == "shell.rail"),
            "the host's own rail is still painted"
        );
        assert!(
            !tags.iter().any(|t| t == "lab.rail"),
            "and the guest's is not built AT ALL -- not painted-and-hidden and \
             not zero-width, because a hidden node is still a node in the tree \
             and a zero-width one is still a node a census counts"
        );
        assert!(
            !tags.iter().any(|t| t.starts_with("lab.rail.")),
            "nor any of its seats"
        );

        // --- and the room it left is USED, not left blank ----------------
        // The guest's panes shift left by the rail it no longer draws, which is
        // the difference between omitting a pane and merely not drawing it.
        let region = pinion_runtime::rect_for_tag(&scene, "shell.canvas")
            .expect("the page region is painted");
        let pan = pinion_runtime::rect_for_tag(&scene, "window.pan");
        let palette = pinion_runtime::rect_for_tag(&scene, "lab.palette")
            .expect("the mounted screen paints its palette");
        let page_x = pan.map_or(region.x, |p| p.x);
        assert_eq!(
            palette.x, page_x,
            "the palette starts at the page's own left edge, where the guest's \
             rail used to be"
        );
    });
}

/// ★★★★★ R1724 — **the accessibility tree follows the rail into the section.**
///
/// The row the reference toolkit fails twice over: measured at 6.11.1, its
/// non-current page is reachable as an accessible child with its text field
/// under it, marked `invisible` and nothing more.
#[test]
fn r1724_the_tree_holds_the_showing_section_and_only_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let announced = || {
            super::AnalyzerShellView::access_node(&ScreenState::default(), None)
                .into_iter()
                .map(|n| n.tag)
                .collect::<Vec<_>>()
        };
        let lab_nodes = |nodes: &[String]| {
            nodes
                .iter()
                .filter(|t| t.as_str() == "node_lab" || t.starts_with("lab."))
                .count()
        };

        assert_eq!(
            lab_nodes(&announced()),
            0,
            "nothing of the lab on the dashboard"
        );

        state.go("lab").expect("the node lab seat is reachable");
        let nodes = super::AnalyzerShellView::access_node(&ScreenState::default(), None);
        let region = nodes
            .iter()
            .find(|n| n.tag == "shell.canvas")
            .expect("the page region is in the tree");
        assert!(
            region.children.iter().any(|c| c == "node_lab"),
            "the lab's root hangs under the region it is painted in: {:?}",
            region.children
        );
        let tags: Vec<String> = nodes.into_iter().map(|n| n.tag).collect();
        assert!(
            lab_nodes(&tags) > 20,
            "and the lab announces its own screen: {} node(s)",
            lab_nodes(&tags)
        );
    });
}

/// ★★★★★ R1724 — **the host's state notices the section it is showing.**
///
/// This shell declared `State = ()` for as long as every page was its own, and
/// that was true then: its pages are signals. A mounted screen's projection
/// comes out of the state scene, so a host whose state never differs would
/// paint that screen's first frame and no other.
#[test]
fn r1724_the_hosts_state_moves_when_the_rail_does() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let empty =
            pinion_core::Scene::Container(pinion_core::scene::ContainerNode::new(Vec::new()));
        let here = super::AnalyzerShellView::read_state(&empty);
        state.go("lab").expect("the node lab seat is reachable");
        let there = super::AnalyzerShellView::read_state(&empty);
        assert_ne!(
            here, there,
            "arriving somewhere is a change the framework can see"
        );
        assert_eq!(
            there,
            super::AnalyzerShellView::read_state(&empty),
            "and standing still is not"
        );
    });
}
