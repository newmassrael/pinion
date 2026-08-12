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
    distinct("rail", RAIL.iter().map(|(k, _)| *k).collect());
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
            super::ShellOracle::add(&state, DEFS[0].kind).expect("the palette offers it");
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
