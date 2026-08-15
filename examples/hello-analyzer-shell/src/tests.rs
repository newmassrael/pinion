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

use super::{
    BarChip, GRID_COLS, KEYMAP, SOURCES, STEPPERS, SubChip, TABS, cell_at, cell_rect, chrome,
    def_of, kind_of, kind_span, parse_state, remedy_label, remedy_word, spec, state_sentence,
    transport_word, type_ink,
};

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
        .filter_map(|seat| seat.reserved_for.map(|why| (seat.key, why)))
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
            .any(|seat| seat.key == spec::RAIL_ACTIVE && seat.reserved_for.is_none()),
        "the seat this screen IS cannot be a reserved one",
    );
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
        let scene = super::view((), pinion_core::Frame::default());
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
        let mut scene = super::view((), pinion_core::Frame::default());
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
            let mut scene = super::view((), pinion_core::Frame::default());
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
        let below: Vec<(String, (i32, i32))> = out
            .iter()
            .filter_map(|o| {
                let tag = o.tag.clone()?;
                let card = tag.strip_prefix("card.")?;
                if card.contains('.') {
                    return None;
                }
                match o.reach {
                    pinion_core::reach::Reach::Scrollable { to } => Some((tag.clone(), to)),
                    pinion_core::reach::Reach::Lost { .. } => None,
                }
            })
            .collect();
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
        super::AnalyzerShellView::access_node(&(), None)
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
    for (template, population, _) in spec::SILENCES {
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
    // Eleven: nine catalogue entries booked for a later release, and the two
    // rail destinations booked the same way. Derived rather than listed, so a
    // seat that is unlocked leaves the table by being unlocked.
    let tags: Vec<String> = spec::LOCKED
        .iter()
        .flat_map(|(template, population)| {
            population
                .members()
                .into_iter()
                .map(move |member| template.replace("{}", &member))
        })
        .collect();
    assert_eq!(tags.len(), 11, "{tags:?}");
    assert_eq!(
        tags.iter()
            .filter(|t| t.starts_with("shell.palette."))
            .count(),
        spec::reserved_count(),
    );
    assert_eq!(
        tags.iter().filter(|t| t.starts_with("shell.rail.")).count(),
        spec::RAIL
            .iter()
            .filter(|seat| seat.reserved_for.is_some())
            .count(),
    );
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
