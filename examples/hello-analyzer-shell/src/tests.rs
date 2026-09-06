//! R1648/R1649 §5.21 — the shell's pure parts.
//!
//! The demo (`tools/demos/r1648_the_analyzer_shell_is_assembled.py`) drives the
//! live window over RPC and is where every wire claim is checked. These pin the
//! functions where a unit test is the sharper instrument — in particular the
//! ones that must be **total over a vocabulary**, which a demo can only sample.

use pinion_core::reactive::Owner;
use pinion_core::scene::Rect;
use pinion_core::widgets::card::{CardAffordance, CardState, Remedy};
use pinion_core::widgets::destination::{Divergence, Required};
use pinion_core::widgets::tile_grid::{Tile, TileDrag};
use pinion_core::widgets::transport::TransportStatus;
use pinion_screen::ScreenState;

use std::collections::BTreeMap;

use super::{
    AnalyzerShellView, BarChip, GRID_COLS, KEYMAP, SOURCES, STEPPERS, SubChip, cell_at, cell_rect,
    chrome, def_of, kind_of, kind_span, parse_state, remedy_label, remedy_word, spec,
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
        faded: next(),
    }
}

/// R1668 — the catalogue is thirteen entries in two tiers, each in a listed
/// section. R1797 — **five** placeable and eight reserved, and a section's
/// heading is read off its entries rather than checked against a second copy.
///
/// The palette's footer states **both** counts, so an entry that moved tier
/// without the footer moving would put two numbers on the screen that disagree
/// with the list under them.
///
/// ★★★★★ R1797 — what this test used to assert, and why it stopped. It read
/// `section.2 == def.tier`: every entry of a section had to carry the section's
/// own tier column. That is one fact stored twice and kept in step by this
/// assertion — the exact shape `section_heading`'s doc comment argues against
/// two paragraphs from where the column stood. It surfaced the moment a single
/// widget was promoted out of a group: the promotion was legal and the screen
/// was right, and a gate failed because the model had no way to hold a mixed
/// section. The tier column is gone; the heading derives.
#[test]
fn r1668_the_catalogue_partitions_into_placed_and_reserved() {
    assert_eq!(spec::CATALOGUE.len(), 13);
    // ★ R1843 — written as a PARTITION rather than as two literals. A
    // promotion moves one seat from the right-hand count to the left, and a
    // pair of hardcoded numbers turns that legal move into two edits in a test
    // whose subject is not the release structure at all.
    assert_eq!(
        spec::placeable_count() + spec::reserved_count(),
        spec::CATALOGUE.len(),
        "the palette's footer counts every entry exactly once",
    );
    assert_eq!(spec::placeable_count(), 7, "the palette's footer says this");

    let mut seen = std::collections::BTreeSet::new();
    let mut codes = std::collections::BTreeSet::new();
    for def in spec::CATALOGUE {
        assert!(seen.insert(def.kind), "{} appears twice", def.kind);
        assert!(codes.insert(def.code), "{} shares its code", def.kind);
        assert!(
            spec::SECTIONS.iter().any(|(key, _)| *key == def.section),
            "{} is in an unlisted section",
            def.kind
        );
        let (placeable, reserved) = spec::section_tiers(def.section);
        assert!(
            match def.tier {
                spec::Tier::Placeable => placeable,
                spec::Tier::Reserved => reserved,
            },
            "{}'s own tier {:?} is not among the releases its section reports",
            def.kind,
            def.tier
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

/// ★★★★★ R1946 — **every navigable surface on this screen is derived from a
/// declaration, and the application bar's view tabs were the one that was not.**
///
/// Until this round the bar's tabs were a two-element constant array of titles
/// in `main.rs`, with their tags spelled a second time inside `BarChip`'s two
/// hand-written tab arms. The rail has been derived since R1695 — `spec::destinations()`
/// builds the roster from `spec::RAIL`, `rail_scene` paints from the same
/// table, `ScreenRoster::new` refuses a mount at a seat the rail closed — and
/// nothing said the bar had to be. Built without a declaration is the mirror of
/// declared without being built, and this is the assertion for that direction.
///
/// ⚠ The two surfaces are NOT the same list and must not be derived from each
/// other: the rail is eight sections, the bar is a view switcher over two. The
/// debt that sent this round here read one against the other; see
/// `spec::ViewTabSpec` for the measurement.
#[test]
fn r1946_every_view_tab_the_bar_presses_is_one_the_specification_declares() {
    // Forward: the declaration has a chip, and that chip's tag is derived from
    // the declared key rather than written beside it.
    for (n, tab) in spec::VIEW_TABS.iter().enumerate() {
        assert_eq!(
            BarChip::Tab(n).tag(),
            format!("shell.appbar.tab.{}", tab.key),
            "the {:?} tab's chip does not carry its declared key",
            tab.title
        );
    }
    // Backward, and this is the half a constant length could not make: the bar
    // presses EXACTLY the declared tabs. A third entry in the specification
    // with no chip behind it, or a chip with no entry, fails here.
    let pressed: Vec<String> = BarChip::all()
        .into_iter()
        .filter_map(|chip| match chip {
            BarChip::Tab(n) => Some(BarChip::Tab(n).tag()),
            _ => None,
        })
        .collect();
    let declared: Vec<String> = spec::VIEW_TABS
        .iter()
        .map(|tab| format!("shell.appbar.tab.{}", tab.key))
        .collect();
    assert_eq!(
        pressed, declared,
        "the bar's tab chips and the specification's tabs are not the same list",
    );
    // ★ And the population cannot be empty — an equality over two empty lists
    // is the shape that passes after the thing it guards has been deleted.
    assert!(
        !declared.is_empty(),
        "the specification declares no view tab, so this assertion compares nothing",
    );
}

/// ★★★★★ R1946 — **the north star's condition (A), as a predicate over the
/// declaration rather than as prose.**
///
/// *Every declared destination is reached and paints something.* Measured here
/// over `spec::RAIL`, which is the population the reproduction command greps:
/// a seat is either open — and then the roster carries a way to paint it, a
/// mounted screen or this shell's own judged page — or it is closed, and then
/// it names the requirement that books it. There is no third state, and a seat
/// that reached neither would fail both arms.
///
/// ⚠ What this does NOT assert is what the page paints: that is
/// `painted::r1695_each_destination_paints_the_regions_the_specification_gives_it`,
/// which sweeps the real pipeline. This one is the *roster* half — that no
/// declared seat is a dead end — and the two are deliberately separate
/// instruments over the same population.
#[test]
fn r1946_every_declared_seat_is_open_with_a_page_or_closed_with_a_reason() {
    let roster = spec::destinations();
    let mut open = 0_usize;
    let mut closed = 0_usize;
    for seat in spec::RAIL {
        let destination = roster
            .get(seat.key)
            .unwrap_or_else(|| panic!("{} is declared and the roster lacks it", seat.key));
        match destination.standing.why() {
            None => {
                open += 1;
                assert!(
                    seat.reserved_for().is_none(),
                    "{} is open on the roster and reserved on the rail",
                    seat.key
                );
            }
            Some(why) => {
                closed += 1;
                let reason = why.sentence();
                assert!(
                    reason.contains("requirement"),
                    "{} is closed and its reason {reason:?} names no requirement — \
                     a reader is told the section is missing and not why",
                    seat.key
                );
            }
        }
    }
    assert_eq!(
        open + closed,
        spec::RAIL.len(),
        "every declared seat is accounted for by exactly one arm",
    );
    // ★ Rule: can this reach zero? `closed` can — the reference deferring
    // nothing would open every seat — and `open` cannot, because
    // `Destinations::new` refuses a roster that opens nothing. Asserting the
    // floor rather than a number keeps the check honest as the rail grows.
    assert!(
        open > 0,
        "no seat is open, which the roster's own constructor should have refused",
    );
}

/// ★★★★★ R1946 — **this build's distance from the BEHAVIOUR reference is a
/// list, and the list is derived rather than remembered.**
///
/// `spec::owed()` measures the distance from the *scope* reference and is
/// empty; the first-stage reproduction is complete. Nothing measured the other
/// reference at all, and the two disagree on exactly two seats — which is how
/// this screen came to report full marks on every instrument it had while two
/// sections a person went looking for did not exist.
///
/// The pin now carries a second standing per seat, and this is its ratchet. It
/// runs in **both** directions on purpose: a seat that quietly stops being
/// reproduced fails, and so does one that gets built without the pin being
/// told. A one-directional check is what lets a remainder drift.
///
/// ⚠ This is NOT a first-stage gap. Both seats are locked by the scope mockup
/// and locked by this build under the same requirement, so the reproduction is
/// faithful. They are second-stage work — reproduce, then improve past it.
#[test]
fn r1946_the_distance_from_the_behaviour_reference_is_exactly_what_the_pin_declares() {
    let derived = spec::second_phase_owed();
    let entries = spec::second_phase_owed_declared();
    let declared: Vec<String> = entries.iter().map(|entry| entry.key.clone()).collect();
    assert_eq!(
        derived, declared,
        "the seats this build closes that the behaviour reference builds are not \
         the seats `docs/analyzer-rail-spec.json` says they are",
    );
    // Each entry's reason is the reference's own doing rather than a restated
    // key — a remainder whose reason is its own name tells a reader nothing.
    for entry in &entries {
        assert!(
            entry.reason.len() > entry.key.len(),
            "{}'s reason says no more than its key",
            entry.key
        );
    }
    // ★ Rule: can this reach zero? Yes — by BUILDING the sections, which is the
    // only thing that removes a key from the derived side. So the equality is
    // allowed to compare two empty lists one day, and the assertion below is
    // what keeps that day from arriving by a parse going quiet instead.
    assert_eq!(
        spec::behaviour_built().len(),
        spec::RAIL.len(),
        "the behaviour reference builds every seat on this rail — a smaller \
         number here means the pin stopped being read, not that a seat closed",
    );
    // Each entry is a seat that exists and is closed HERE. A key naming no seat,
    // or naming an open one, would make the remainder describe a screen nobody
    // has.
    for key in &derived {
        let seat = spec::RAIL
            .iter()
            .find(|s| s.key == key)
            .unwrap_or_else(|| panic!("{key} is owed and the rail has no such seat"));
        assert!(
            seat.reserved_for().is_some(),
            "{key} is owed against the behaviour reference and open here",
        );
    }
}

/// ★★★★★ R1948 — **every seat says where it goes, and none says it is
/// missing.**
///
/// ⚠ This REPLACES `r1946_a_seat_this_build_is_behind_the_reference_on_says_so`,
/// on that gate's own instruction. It asserted that a seat owed against the
/// behaviour reference carries a clause saying so, and it opened by refusing an
/// empty population *in those words*: **"build the sections and delete this
/// gate, do not let it pass by having nothing to judge."** R1947 and R1948 built
/// them, `second_phase_owed` is empty, and the gate did exactly what it was
/// written to do — it failed rather than passing over nothing.
///
/// What replaces it is the claim that survives: every seat is a place a reader
/// can go, and no seat's sentence still tells them a section is absent. The
/// second half is what catches the residue of a round like this one — a clause
/// left behind in a register after the thing it described stopped being true.
#[test]
fn r1948_every_rail_seat_says_where_it_goes_and_none_pleads_absence() {
    const BEHIND: &str = "the reference draws it and this build does not yet";
    const ABSENT: &str = "is not in this release";
    let described = super::chrome_descriptions();
    let mut checked = 0_usize;
    for seat in spec::RAIL {
        let sentence = described
            .of(&format!("shell.rail.{}", seat.key))
            .unwrap_or_else(|| panic!("{} carries no description", seat.key));
        assert!(
            !sentence.contains(BEHIND),
            "{}'s sentence still says the reference has something this build does \
             not, and `second_phase_owed` is empty — sentence: {sentence:?}",
            seat.key,
        );
        assert!(
            !sentence.contains(ABSENT),
            "{}'s sentence still tells a reader the section is missing — {sentence:?}",
            seat.key,
        );
        assert!(
            sentence.starts_with("Go to "),
            "{}'s sentence does not say where pressing it goes — {sentence:?}",
            seat.key,
        );
        checked += 1;
    }
    // ★ Rule: can this pass by judging nothing? The count is asserted against
    // the rail, so an empty register fails here rather than reading as success.
    assert_eq!(checked, spec::RAIL.len(), "not every seat was judged");
    assert!(
        spec::second_phase_owed().is_empty(),
        "a seat is owed against the behaviour reference and every sentence above \
         says otherwise",
    );
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
    assert_eq!(
        reserved.len(),
        spec::CATALOGUE.len() - spec::placeable_count(),
        "the reserved seats are what the catalogue has left after the placed ones",
    );
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
    // ★★★★★ R1947 — **this counts what THIS RAIL locks, and it is no longer the
    // same number as what the reference locks.**
    //
    // It read `assert_eq!(locked.len(), 2, "the reference locks two rail
    // seats")` — a count of one thing under a sentence about another, true only
    // while the two agreed. R1947 opened `topology`, which the scope mockup
    // locks and the behaviour reference builds, so they do not agree any more.
    //
    // The relation is what is asserted now, and it is the honest one: every
    // seat this rail locks is one the reference locks too, and the reference
    // locking MORE is a declared divergence rather than a defect. A count would
    // have to be edited every time either side moves; a subset holds.
    let specified_locked: Vec<String> = spec::canon()
        .all()
        .iter()
        .filter(|seat| seat.standing.why().is_some())
        .map(|seat| seat.key.to_string())
        .collect();
    assert!(
        !specified_locked.is_empty(),
        "the reference locks nothing, so the comparison below judges an empty set",
    );
    for (key, _) in &locked {
        assert!(
            specified_locked.iter().any(|s| s == key),
            "this rail locks {key}, which the reference does not",
        );
    }
    // ★★★★★ R1953 — **which seats this build opens that the reference locks is
    // derived, and it is judged against the list that MEANS that.**
    //
    // This read `spec::owed()` — the list meaning *the reference has this and
    // this build has not written it* — and asked it to excuse the opposite
    // claim. It passed only while the two happened to share an array, and when
    // R1953 split them by meaning this gate went red for the honest reason: the
    // entries it was reading had moved to `ahead`, where they always belonged.
    //
    // It also computed the live difference for itself, by walking the rail's
    // bookings against the specification's standings — a second spelling of
    // what `r1728` below derives through the framework's roster diff. Both read
    // `spec::rail_divergence()` now, so the two gates cannot come apart.
    let declared = spec::ahead();
    let opened: Vec<String> = spec::rail_divergence()
        .into_iter()
        .filter_map(|difference| match difference {
            Divergence::Standing {
                key,
                specified: Required::Closed(_),
                found: Required::Open,
            } => Some(key),
            _ => None,
        })
        .collect();
    for key in &opened {
        assert!(
            declared.owed().iter().any(|entry| &entry.key == key),
            "this rail opens {key}, which the reference locks, and no entry in \
             `docs/analyzer-rail-spec.json`'s `ahead` declares it",
        );
    }
    // ⚠ And the derivation has to agree with the standings this test read for
    // itself, or the filter above is quietly selecting nothing and every
    // assertion in the loop is vacuous.
    let by_standing: Vec<String> = specified_locked
        .iter()
        .filter(|key| !locked.iter().any(|(k, _)| *k == key.as_str()))
        .cloned()
        .collect();
    assert_eq!(
        opened, by_standing,
        "the derived difference and the bookings disagree about which seats \
         this rail opens that the reference locks",
    );
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
    // ★★★★★ R1731 — **and the same thing happened again, one arm along.**
    // R1730 built `keys` and R1731 built `logs`, so nothing constructs
    // `Seat::Unbuilt` either and the compiler said so. This rail now has TWO
    // arms — a page and a booked seat — and cannot spell either *built,
    // shipping, and not here* or *specified and not built*. What is left shut
    // is shut because the reference itself defers it, which is the only kind of
    // shut a faithful reproduction of this rail can have.
    //
    // What is asserted instead is the property that survives: every seat that
    // is not a page names the requirement it is booked under.
    let booked: Vec<_> = spec::RAIL
        .iter()
        .filter_map(|seat| seat.reserved_for().map(|why| (seat.key, why)))
        .collect();
    // ★★★★★ R1948 — **the rail books NOTHING now, and the floor R1947 put here
    // is gone rather than lowered.**
    //
    // R1947 replaced a hard `2` with `!booked.is_empty()`, which was right at
    // the time and rested on the same assumption one layer down: that this rail
    // always refuses something. Building the sessions section made that false —
    // `spec::Seat` lost its last arm to the compiler in the same edit — so the
    // floor failed, correctly, on the round that emptied the population.
    //
    // ⚠ What replaces it is NOT a weaker check. The property below is still
    // asserted over whatever is booked, and the emptiness itself is now a
    // CLAIM: a seat that starts being refused again has to come with a reason
    // that names a requirement, and `r1728` holds the rail to the pin either
    // way. A test that judged an empty set silently is what this comment exists
    // to prevent, and the assertion under the loop is that guard.
    for (key, why) in &booked {
        assert!(
            why.starts_with("requirement ") && why.len() > 12,
            "the {key} seat cites {why:?}, which names no requirement",
        );
    }
    // ★ The emptiness is a claim rather than a silence: this rail refuses
    // nothing, and the reference's own locks live in the pin's `owed`. If that
    // stops being true the reason is a rail seat that grew a requirement back,
    // and the loop above is what judges it.
    assert_eq!(
        booked.len(),
        spec::RAIL.len() - spec::destinations().all().len(),
        "a seat is booked on the rail and open on the roster, or the other way",
    );
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
    let found = spec::rail_divergence();
    // ★★★★★ R1953 — **both directions, because the specification declares
    // both.** This read `spec::owed()` alone while two entries meaning *the
    // reference locks this and we open it* sat in that array; the equality then
    // held for the wrong reason, and split them by meaning and it fails.
    // `spec::divergences()` is the roster for this question and the ONLY one
    // that answers it — a seat declared in both directions cannot even load.
    let declared = spec::divergences();
    let unreconciled: Vec<String> = declared
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
    let reproduced = spec::canon_spec().len() - declared.len();
    assert_eq!(
        reproduced + declared.len(),
        built.len(),
        "every seat is either reproduced or declared as a difference, and none \
         is both",
    );
    // ⚠ The population is not allowed to be empty: an equality against an empty
    // ledger judging an empty diff passes while saying nothing, and this rail
    // has declared differences in exactly one direction since R1947.
    assert!(
        !declared.owed().is_empty(),
        "the specification declares no difference at all, so the equality above \
         is comparing two empty lists",
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

/// ★★★★★ R1965 — **every way the rail can differ from its specification is a
/// way this gate NAMES**, and a sixth way would not compile.
///
/// # The class this closes
///
/// `r1728_the_rail_reproduces_the_reference_or_says_where_it_does_not` compares
/// `spec::RAIL` against the reviewed `docs/analyzer-rail-spec.json` and refuses
/// any difference the pin does not declare. What it never said is *which kinds
/// of difference it can see* — so a reader had two choices: believe it, or find
/// out by hand.
///
/// R1965 found out by hand, and it took four mutations of the real table: a
/// reorder is caught (`` `logs` is specified at seat 3 and sits at seat 7 ``), a
/// retitle is caught (`` `settings` is specified as "Settings" and reads
/// "Preferences" ``), closing a seat is caught, and deleting one is caught — the
/// last by `screen_roster()` rather than by the comparison, which is a
/// different guarantee wearing the same green. That work is this test now,
/// because a coverage a session has to rediscover by mutating a shipped
/// constant is a coverage nobody will check twice.
///
/// # Why the omission is a COMPILE error and not an assertion
///
/// The population is [`Divergence`]'s own arms. A hand-written list of five
/// would leave a sixth silently uncovered — the escape-hatch shape this
/// workspace refuses — so each produced divergence is classified through an
/// EXHAUSTIVE match, and a variant added upstream stops the build here until
/// somebody says whether the rail can diverge that way.
///
/// ⚠ Driven over a SYNTHETIC pair, deliberately. Asking the real rail would
/// answer *the two `ahead` entries* and nothing else, because the real rail
/// conforms — a gate whose population is the passing case cannot show what it
/// would catch.
#[test]
fn r1965_every_way_the_rail_can_diverge_is_one_this_gate_names() {
    use pinion_core::widgets::destination::{Destination, Destinations, RosterSpec, SeatSpec};

    // A specification of four seats, and a rail that differs from it in every
    // way the model can express at once.
    let specified = RosterSpec::new(vec![
        SeatSpec::open("alpha", "Alpha"),
        SeatSpec::open("beta", "Beta"),
        SeatSpec::open("gamma", "Gamma"),
        SeatSpec::open("delta", "Delta"),
    ])
    .expect("a written specification of four open seats");
    let built = Destinations::new(vec![
        // `alpha` keeps its place and its name, so something conforms — a diff
        // in which everything differs cannot show that agreement is possible.
        Destination::open("alpha", "Alpha"),
        // `gamma` and `beta` change places.
        Destination::open("gamma", "Gamma"),
        Destination::open("beta", "Bravo"),
        // `delta` is present and cannot be arrived at.
        Destination::closed(
            "delta",
            "Delta",
            pinion_core::availability::Unavailable::reserved("requirement 1"),
        ),
        // And a seat no specification declares.
        Destination::open("epsilon", "Epsilon"),
    ])
    .expect("a rail of five seats");

    let found = specified.diff(&built);

    let (mut absent, mut unspecified, mut out_of_order, mut retitled, mut standing) =
        (0_usize, 0_usize, 0_usize, 0_usize, 0_usize);
    for divergence in &found {
        // ★ EXHAUSTIVE on purpose. A sixth arm upstream is a compile error
        // here, which is the only way an uncovered kind stops being silent.
        match divergence {
            Divergence::Absent { .. } => absent += 1,
            Divergence::Unspecified { .. } => unspecified += 1,
            Divergence::OutOfOrder { .. } => out_of_order += 1,
            Divergence::Retitled { .. } => retitled += 1,
            Divergence::Standing { .. } => standing += 1,
        }
    }

    // Every kind is REACHED. A zero here is a kind the comparison cannot see,
    // which is a difference this application could ship without being told.
    for (kind, count) in [
        ("Unspecified", unspecified),
        ("OutOfOrder", out_of_order),
        ("Retitled", retitled),
        ("Standing", standing),
    ] {
        assert!(
            count > 0,
            "the diff produced no {kind} divergence over a pair built to \
             contain one, so a rail differing that way from its specification \
             would pass unremarked: {found:?}",
        );
    }
    // ⚠ `Absent` is the one kind this pair cannot produce, and saying so is the
    // point rather than an omission: every specified seat IS on the rail here.
    // It is exercised by the shell's own roster instead — deleting a seat from
    // `spec::RAIL` fails at `screen_roster()` with `` `settings` is an open
    // destination with no screen mounted at it ``, measured at R1965 — which is
    // a DIFFERENT guarantee, and one that would not fire for a seat the shell
    // paints itself. Asserted as zero so that a later change making the pair
    // produce one is a red that sends a reader back to this sentence.
    assert_eq!(
        absent, 0,
        "this pair declares every specified seat on the rail, so an Absent \
         divergence means the fixture no longer says what this comment says",
    );

    // And the sentences name the seat and both sides, because a divergence a
    // person cannot act on is a divergence they will not act on.
    let said: Vec<String> = found.iter().map(Divergence::sentence).collect();
    for want in [
        "`beta` is specified as \"Beta\" and reads \"Bravo\"",
        "seat 4 `epsilon` is on the rail and no specification declares it",
    ] {
        assert!(
            said.iter().any(|s| s == want),
            "no divergence reads {want:?}; the gate said {said:?}",
        );
    }
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
    let bar_chips = BarChip::all();
    for (i, one) in bar_chips.iter().enumerate() {
        for other in &bar_chips[i + 1..] {
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
    for chip in BarChip::all() {
        assert!(seen.insert(chip.tag()), "{} is used twice", chip.tag());
    }
    for chip in SubChip::ALL {
        assert!(
            seen.insert(chip.tag().to_owned()),
            "{} is used twice",
            chip.tag()
        );
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
    distinct(
        "tabs",
        spec::VIEW_TABS.iter().map(|tab| tab.title).collect(),
    );
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
/// ★★★★★ R1971 — **nothing this screen paints carries a name and no box.**
///
/// The class [`pinion_core::reach::Reach::Unplaced`] names, asked of THIS
/// screen, because R1971 measured that asking it of the node lab alone was not
/// enough: with a placement removed from this screen's own icon strokes, the
/// whole in-process suite here stayed GREEN and only a demo caught it. A gate
/// that lives on one screen is a gate the next screen does not have.
///
/// ⚠ It found three the moment it could see: bare paths `5`, `6` and `8` under
/// the root, which nothing had ever reported because `walk_marks` returned on a
/// zero box before this round.
#[test]
fn r1971_this_screen_paints_nothing_it_cannot_place() {
    let owner = Owner::new();
    owner.run(|| {
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
        let window = (super::WIN_W, super::WIN_H);
        let out = pinion_core::reach::out_of_sight(
            &scene,
            window,
            &mut pinion_core::test_fixtures::screen_ink::stand_in_ink,
            // R2035 — a stand-in measurer judging placement: this gate asks
            // nothing about the process's faces, and the arm that excuses
            // nothing is what keeps it strict.
            pinion_core::reach::Faces::Unproven,
        );
        let unplaced: Vec<String> = out
            .iter()
            .filter(|o| o.reach.is_unplaced())
            .map(|o| {
                format!(
                    "{} (rect {:?})",
                    o.tag.clone().unwrap_or_else(|| o.path.join("/")),
                    o.rect,
                )
            })
            .collect();
        assert!(
            unplaced.is_empty(),
            "{} mark(s) carry a name and NO BOX: {unplaced:?}. A primitive whose \
             own `rect` holds its geometry must be placed with `absolute(rect)`; \
             put in flow, the layout pass overwrites that rect with the flow box \
             and every index built from `absolute_rect` drops it.",
            unplaced.len(),
        );
        // ★ The walk reached this screen at all — without a floor an empty
        // report reads as clean on a scene nothing painted.
        let mut marks = 0_usize;
        scene.for_each_node(&mut |_| marks += 1);
        assert!(marks > 100, "the sweep examined {marks} node(s)");
    });
}

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
        // ★ R1971 — and a card with NO BOX has nothing to press towards for a
        // different reason: there is no rectangle to bring anywhere. Answered
        // `None` here because this function's question is only *what do I
        // scroll to*; that it is a defect is reported where the class is
        // judged, and the caller's own population floor below refuses a sweep
        // that found no card to scroll to at all.
        // ★ R2025 — and the arm the walk declined to judge answers `None` for
        // the same reason the two above it do: it has no rectangle either.
        pinion_core::reach::Reach::Clipped { .. }
        | pinion_core::reach::Reach::Lost { .. }
        | pinion_core::reach::Reach::Unplaced
        | pinion_core::reach::Reach::Unjudged { .. } => None,
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
        let out = pinion_core::reach::out_of_sight(
            &scene,
            (super::WIN_W, super::WIN_H),
            ink,
            pinion_core::reach::Faces::Unproven,
        );
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

/// ★★★★★ R1867 — **no destination paints a region nobody decided about**, and
/// this asks it where a round can hear the answer.
///
/// # The hole this closes, measured — and it is the HOST's, not the census's
///
/// A region that entered this screen with no declaration was published first
/// and reported later, by the demo sweep, which runs after a push. It happened
/// **twice, three rounds apart, neither round aware of the other**:
/// `shell.status` at R1864 and `shell.status.slot` at R1865. Two rounds finding
/// the same hole independently is a structure, not a slip, and the repair for a
/// structure is a gate rather than a note.
///
/// ⚠ **The obvious reading of that is wrong, and measuring said so.** A comment
/// left at R1846 gives the reason as *"a voice census needs a running screen and
/// a demo is not run by `cargo test`"*. Counted here: **four sibling screens
/// already run this census in `cargo test`** — `hello-packet-view`,
/// `hello-node-lab`, `hello-key-patterns` and `hello-log-view` each build their
/// scene, enrich the names from it and assert the defect arms are empty; the
/// node lab does it at two window sizes and over four states. So the census
/// never needed a running screen. What had no such gate was the **assembled
/// shell** — the host that composes those four — which is exactly where the two
/// undeclared regions went in.
///
/// ⇒ the recipe below is a fifth hand-rolled copy of one mechanism, and that is
/// registered rather than hidden: `debt-five-screens-hand-roll-one-voice-gate`.
///
/// # Both occupancies, because the slot has two
///
/// A `layout` silence promises that the subtree speaks, and the status band's
/// slot holds a toast for 2.6 seconds and the gesture sentence the rest of the
/// time. Checking one state would leave the other free to be `hollow`, which is
/// precisely how the gesture sentence came to be inaudible: it was the state
/// nobody looked at.
#[test]
fn r1867_no_destination_paints_a_region_with_no_declared_voice() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = spec::destinations();
        let mut wrong: Vec<String> = Vec::new();
        let mut judged = 0_usize;
        for holding in [true, false] {
            if holding {
                state.say(pinion_core::utterance::Utterance::done(
                    "a sentence the slot is holding",
                ));
            } else {
                // Drive the toast's whole life forward rather than clearing it
                // by hand: what this gate is judging is the state the screen
                // actually reaches, and `Saying::life` is the number that says
                // when it reaches it. ⚠ Seconds, not milliseconds (R1783).
                owner.tick_animations(state.toast.life() + 1.0);
            }
            assert_eq!(
                state.toast.showing().is_some(),
                holding,
                "the slot's occupancy is what this loop varies",
            );
            for destination in roster.open() {
                let key = destination.key.as_ref();
                assert!(state.go(key).is_ok(), "the rail must reach {key}");
                let census = census_of_the_open_destination();
                judged += census.nodes.len();
                for row in &census.nodes {
                    if row.voice.is_defect() {
                        wrong.push(format!(
                            "{key} ({}): {} is {} (name {:?}, fault {:?})",
                            if holding {
                                "a toast in the slot"
                            } else {
                                "the gesture sentence in the slot"
                            },
                            row.tag,
                            row.voice.name(),
                            row.name,
                            row.fault,
                        ));
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} region(s) of {judged} judged are not classified — every one is a \
             reader who is not told something the screen paints, and the repair \
             is a row in `spec::VOICES` or `spec::SILENCES` rather than a wider \
             budget here:\n  {}",
            wrong.len(),
            wrong.join("\n  "),
        );
        // ★ And the gate has to be able to FAIL: a census over nothing reports
        // nothing wrong, which is the shape every vacuous green takes.
        assert!(
            judged > 0,
            "the census judged no region at all, so its verdict is vacuous",
        );
    });
}

/// The four-step recipe that turns whatever destination the rail is standing at
/// into a census of what it says.
///
/// ★★★★★ The third step is the one this gate would have been WRONG without, and
/// it is the difference between a tree and the tree a reader gets: a widget's
/// `access_node` returns role, state and value and deliberately NOT the name for
/// anything named from its paint (`grid_table_nodes` says so at the site: *"NO
/// `with_name`: the name is derived from the painted header"*). The runtime
/// fills those in after layout. Skipping it reported **90 `mumbled` regions
/// across four destinations** that a running window does not have — a gate
/// accusing the screen of a defect the gate had created.
///
/// ⚠ Written once here because this screen now has TWO gates over it (R1868),
/// and a recipe copied twice in one file is the copy that drifts. The
/// cross-screen lift — four sibling screens carry this by hand — stays
/// registered as `debt-five-screens-hand-roll-one-voice-gate`, because each
/// screen's SECOND axis differs and only the construction is common.
fn census_of_the_open_destination() -> pinion_core::voice::VoiceCensus {
    let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
    let mut cache = pinion_runtime::LayoutCache::new();
    pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
    let mut nodes = AnalyzerShellView::access_node(&ScreenState::default(), None);
    pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
    let announced = pinion_a11y::announcements(&nodes);
    let referenced = pinion_a11y::referenced_tags(&nodes);
    pinion_core::voice::voice_census(&scene, &announced, &referenced)
}

/// Every region this screen PAINTS is one it PUBLISHES, and the other way round.
///
/// ★★★★★ R1868 — the half of R1867's rule that was still guarded only by a
/// demo, measured by counterfactual before a line of this was written: deleting
/// `shell.status`'s row from [`spec::SILENCES`] left `cargo test -p
/// hello-analyzer-shell` **green** and was caught by
/// `tools/demos/r1694_a_locked_seat_is_heard.py` alone. Demos run in CI's sweep,
/// *after* the push — the wrong side of publishing for a rule about what may be
/// published, and the reason R1864's band and R1865's slot each reached a
/// release with no published declaration.
///
/// A screen's voice is written down twice: by the painter, as the silence a
/// scene node carries, and by the specification the screen publishes. R1867's
/// gate judges the first alone. This one judges that the two records are the
/// same record.
///
/// # The population, stated rather than assumed
///
/// The census is the UNION over the status slot's two occupancies, because a
/// region painted in one of them is one this screen has; and the declarations
/// are filtered to the destination walked, because the table describes an
/// application of many pages and comparing all of it against one page would
/// demand every other page's regions here. Both obligations are
/// [`pinion_core::voice::reconcile`]'s, stated in its own documentation, and
/// this is where they are met.
#[test]
fn r1868_what_a_destination_paints_is_what_it_publishes_about_itself() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = spec::destinations();
        let screens = super::screen_roster();
        let mut wrong: Vec<String> = Vec::new();
        let mut compared = 0_usize;
        let mut destinations = 0_usize;
        for destination in roster.open() {
            let key = destination.key.as_ref();
            // ⚠ A destination whose page is a MOUNTED SCREEN is not this
            // screen's to reconcile, and the first draft of this gate did not
            // know that: it reported **728 disagreements**, every one of them a
            // guest's own region (`pv.*` at `packets`), because the host's table
            // names none of them and must not —
            // `r1695_every_open_destination_owns_at_least_one_declared_region`
            // states that partition and asserts it. What this gate judges is the
            // host: its chrome, and the pages it paints itself.
            //
            // ★ The residue that leaves, stated rather than hidden: a mounted
            // destination's regions are reconciled against the GUEST's table by
            // the guest's own tests, and nothing reconciles the two tables
            // TOGETHER — the composed screen has no single published
            // description. That is a spec question, not a gate one, and it is
            // registered rather than papered over here.
            if screens.is_mounted(key) {
                continue;
            }
            destinations += 1;
            assert!(state.go(key).is_ok(), "the rail must reach {key}");
            let mut nodes = Vec::new();
            for holding in [true, false] {
                if holding {
                    state.say(pinion_core::utterance::Utterance::done(
                        "a sentence the slot is holding",
                    ));
                } else {
                    owner.tick_animations(state.toast.life() + 1.0);
                }
                assert_eq!(
                    state.toast.showing().is_some(),
                    holding,
                    "the slot's occupancy is what this loop varies",
                );
                nodes.extend(census_of_the_open_destination().nodes);
            }
            let census = pinion_core::voice::VoiceCensus { nodes };
            let declared_voices: std::collections::BTreeSet<String> = spec::VOICES
                .iter()
                .filter(|voice| voice.at.shows_at(key))
                .flat_map(|voice| {
                    voice
                        .population
                        .members()
                        .into_iter()
                        .map(|member| voice.tag.replace("{}", &member))
                })
                .collect();
            let declared_silences: std::collections::BTreeMap<String, String> = spec::SILENCES
                .iter()
                .filter(|(_, _, _, at)| at.shows_at(key))
                .flat_map(|(tag, population, kind, _)| {
                    population
                        .members()
                        .into_iter()
                        .map(move |member| (tag.replace("{}", &member), (*kind).to_owned()))
                })
                .collect();
            compared += declared_voices.len() + declared_silences.len();
            for disagreement in
                pinion_core::voice::reconcile(&census, &declared_voices, &declared_silences)
            {
                wrong.push(format!(
                    "{key}: {} {}",
                    disagreement.tag,
                    disagreement.mismatch.sentence(),
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} region(s) where what this screen paints and what it publishes \
             disagree — a client reading the specification is told something the \
             window does not do:\n  {}",
            wrong.len(),
            wrong.join("\n  "),
        );
        // ★ The premise, both halves: a reconciliation over two empty tables
        // agrees with everything, and a loop that skipped every destination
        // compares nothing at all. Either is the shape a vacuous green takes.
        assert!(
            destinations > 0,
            "every open destination was skipped as mounted, so this gate judged \
             no page of this screen's own",
        );
        assert!(
            compared > 0,
            "no declaration was compared at all, so the verdict is vacuous",
        );
    });
}

/// ★★★★★ R2002 — **a borrowed name says the words that were borrowed**, on the
/// assembled application and on every page of it.
///
/// A caption declaring [`pinion_core::voice::Silence::name_of`] says *my ink is
/// that node's NAME*. WAI-ARIA calls the obligation label-in-name: a
/// speech-input user says the visible label out loud, and a sighted helper reads
/// it to somebody who cannot, so a control painted one phrase and announced
/// another is reachable by neither. It is the one place on these screens where
/// some ink and some announcement are DECLARED to be the same words — which is
/// what makes this a comparison rather than a guess about how a screen chose to
/// name things.
///
/// # Why the gate is here and not only in a walk
///
/// `tools/demos/r1692` has compared these since R1692, on **one screen**, in
/// CI's sweep — after publishing, and 97 minutes into a job. Measured when the
/// rule moved into `voice_census` this round, the assembled application had
/// **four** violations and the walk could see one of them: the node lab's turn
/// seat (`turn` / *make this wire run the other way*), the config form's defect
/// badge (`out_of_range` / *…is outside 1..=8192*), the lab's run button while
/// running (`running 3/3` / *stop*), and this shell's own health tile
/// (`Rate /s` / *Rate*). Three were on pages no walk asked the question about.
///
/// ⚠ The population is the point, so it is asserted. A screen that lent no
/// names at all would pass this by having nothing to judge, which is the shape
/// every vacuous green takes — and the floor is a MEASUREMENT (`cargo test`,
/// this round), not a wish.
#[test]
fn r2002_every_page_of_the_tool_says_the_words_its_captions_lend_out() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let roster = spec::destinations();
        let mut lent = 0_usize;
        let mut pages = 0_usize;
        let mut wrong: Vec<String> = Vec::new();
        for destination in roster.open() {
            let key = destination.key.as_ref();
            assert!(state.go(key).is_ok(), "the rail must reach {key}");
            pages += 1;
            let census = census_of_the_open_destination();
            for row in &census.nodes {
                // ★ Rows that MADE the promise, not rows an ancestor's promise
                // reached: the obligation is the declaring node's, and that is
                // also the only row the census can report against.
                if row.self_declared
                    && row.silence.as_ref().map(pinion_core::voice::Silence::kind)
                        == Some(pinion_core::voice::SilenceKind::NameOf)
                {
                    lent += 1;
                }
                if row.voice == pinion_core::voice::Voice::Misquoted {
                    wrong.push(format!(
                        "{key}: {} lends its ink to {:?}, which says something else",
                        row.tag,
                        row.silence
                            .as_ref()
                            .map(pinion_core::voice::Silence::detail),
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} caption(s) lend their words to a name that does not carry them — \
             a person reading the label aloud reaches nothing:\n  {}",
            wrong.len(),
            wrong.join("\n  "),
        );
        assert!(
            pages > 0,
            "no page was walked at all, so the verdict is vacuous",
        );
        // ★ 89 across 8 pages, measured this round by raising this floor until
        // it reported the number. A floor and not an equality: a round that
        // adds a caption is not a regression, and what this catches is the
        // walk quietly stopping short of the pages — which is how a gate over a
        // population goes vacuous without saying so.
        assert!(
            lent >= 89,
            "only {lent} caption(s) across {pages} page(s) lend a name, and 89 \
             across 8 was the measurement — a count this low means the walk \
             stopped reaching the pages rather than that the screens changed",
        );
    });
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
            // ★ R2022 — the body rectangle the card actually has, so the roster
            // is the one the painter placed. The card is on the board at its
            // opening size here, which is wide enough for every chip.
            super::filter_nodes(
                &state,
                "filter#3",
                state
                    .card("filter#3")
                    .and_then(|card| super::card_body_rect(&state, &card))
                    .expect("the filter card is on the board and ready"),
            )
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
            let Some(declared) = stop.interior.roster() else {
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
                .filter(|s| s.interior.roster().is_some())
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
            let Some(declared) = stop.interior.roster() else {
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
            let Some(declared) = stop.interior.roster() else {
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
        if let Some(cursor) = stop.interior.roster() {
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
            (tags, state.toast.showing())
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
            BarChip::Tab(0).tag(),
            "\u{2605} entering lands on the tab list's own cursor"
        );
        assert_ne!(
            inside,
            super::APP_BAR_TABS,
            "and the active descendant is the innermost tag, not the list"
        );

        // The inner axis answers and the outer one does not move.
        assert!(press_key(Some("shell.appbar"), "ArrowRight"));
        assert_eq!(descendant(), Some(BarChip::Tab(1).tag()));
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
        assert_eq!(
            after,
            spec::VIEW_TABS[1].title,
            "to the tab the cursor was on"
        );

        // Escape leaves ONE level: back onto the tab list, still in the bar.
        assert!(press_key(Some("shell.appbar"), "Escape"));
        assert_eq!(descendant(), Some(super::APP_BAR_TABS.to_owned()));
        assert!(
            press_key(Some("shell.appbar"), "ArrowRight"),
            "and the bar's own axis answers again"
        );
        assert_eq!(descendant(), Some(BarChip::Source.tag()));

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
        assert_eq!(tabs_nav.members().len(), spec::VIEW_TABS.len());
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
        let said = state.toast.showing();
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
            state.toast.showing(),
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
        // ★★★★★ R1791 — the pan viewport is GONE, and that is the fix rather
        // than a regression. A mounted section pans only when it declares a
        // width the page cannot give it; the lab declared 1625 against 1388, so
        // the shell wrapped it in one. The toolbar can give a group up now, the
        // lab declares a width that fits, and there is nothing to pan — so the
        // card's rectangle is in the window's own frame. Read either way rather
        // than assuming: this test is about where a press LANDS, and it must go
        // on being true whichever of the two the layout is in.
        let page = pinion_runtime::rect_for_tag(&scene, "window.pan")
            .unwrap_or(pinion_core::scene::Rect::new(0, 0, 0, 0));
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
        state.drag.set(Some(
            TileDrag::grip(&board, &dragged, tile.col, tile.row).expect("the card is on the board"),
        ));

        let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let tags = scene.tags();
        let at = |want: &str| tags.iter().position(|t| t == want);

        let slot = at("shell.carry.slot").expect("the snap preview is painted while dragging");
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
        state.drag.set(Some(
            TileDrag::grip(&board, &dragged, tile.col, tile.row).expect("on the board"),
        ));

        let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let fill = find_fill(&scene, "shell.carry.slot").expect("the preview is painted");
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
            !scene.tags().iter().any(|t| t == "shell.carry.chip"),
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
        state.drag.set(Some(
            TileDrag::grip(&board, &dragged, tile.col, tile.row).expect("on the board"),
        ));

        let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let tags = scene.tags();
        assert!(
            tags.iter().any(|t| t == "shell.carry.chip"),
            "a card being carried puts its name on the cursor"
        );
        // ★ Above the BOARD and everything on it — not last in the window,
        // which the palette owns. The first draft asserted last-in-the-scene
        // and failed against `shell.palette.reserved`, which is a true fact
        // about a different plane: this chip's place is the pointer's within
        // the page, and the page is not the whole window.
        let carried = tags
            .iter()
            .position(|t| t == "shell.carry.chip")
            .expect("the chip is painted");
        let board_things: Vec<usize> = tags
            .iter()
            .enumerate()
            .filter(|(_, t)| t.starts_with("card.") || *t == "shell.carry.slot")
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

/// ★★★★★ R1761 — **screen C has two written specifications and they must not
/// name the same surface.**
///
/// The split is by EVIDENCE and not by screen: `analyzer-board-spec.json` holds
/// surfaces that are a declaration, or that exist only while a placement is in
/// flight, judged by the driven sweep in `crate::painted`;
/// `analyzer-dashboard-spec.json` holds the ones readable from a frame at any
/// moment, judged by `crate::judge` and published to the host.
///
/// One word with two documents behind it is the defect R1747 spent a round on —
/// a screen's own `context` table collided with its pin's `context` surface, and
/// a lookup by name could not say which it had found. Two documents about one
/// screen are safe only while nothing has to choose between them, so this is the
/// condition that makes them safe, asserted rather than remembered.
#[test]
fn r1761_the_two_specifications_of_this_screen_name_disjoint_surfaces() {
    let board = spec::board_document();
    let dashboard = spec::dashboard_document();
    let named: std::collections::BTreeSet<String> = board.surfaces().map(str::to_owned).collect();
    let both: Vec<&str> = dashboard
        .surfaces()
        .filter(|surface| named.contains(*surface))
        .collect();
    assert!(
        both.is_empty(),
        "both documents name {both:?}, so a reader asking this screen about that \
         surface would get whichever document was looked in first"
    );
    assert!(
        !named.is_empty() && dashboard.surfaces().count() > 0,
        "both documents have surfaces, so the disjointness above is a claim \
         rather than one empty set meeting another"
    );
}

// --- The latency card (R1797) ------------------------------------------------

/// ★★★★★ R1797 — **the reference's two halves disagree, and this says so.**
///
/// The reference's latency card publishes eight bucket counts AND three stat
/// tiles, independently. Its counts total 1,672 samples of which 93.9% are at
/// or below 16 ms, so the 95th percentile of the bars it draws falls in the
/// `16-32` bucket — while the tile beside them says 11.4 ms, which is in
/// `8-16`. Nothing in a mockup can notice that.
///
/// This is the test that would have failed on the reference, and it passes here
/// because every number on this card comes out of one record.
#[test]
fn r1797_the_cards_percentile_is_consistent_with_its_own_bars() {
    let binned = pinion_chart::Binned::over(
        spec::LATENCY_SAMPLES,
        spec::LATENCY_LADDER,
        pinion_chart::BinEnds::Open,
    )
    .expect("the specification's record is binnable");
    let quantiles =
        pinion_chart::Quantiles::of(spec::LATENCY_SAMPLES, pinion_chart::QuantileMethod::Linear)
            .expect("the record is finite");

    let p95 = quantiles.at(0.95).expect("linear defines p95");
    // Which bucket does the 95th percentile of the DRAWN BARS fall in? Walk the
    // cumulative counts, the way the check on the reference was done.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a hundred samples; f64 is exact well past that"
    )]
    let cut = 0.95 * quantiles.n() as f64;
    let mut cumulative = 0_u32;
    let mut bucket = binned.bins() - 1;
    for (k, count) in binned.counts().iter().enumerate() {
        cumulative += count;
        if f64::from(cumulative) >= cut {
            bucket = k;
            break;
        }
    }
    let (lo, hi) = binned.extent(bucket).expect("a bin the walk reached");
    assert!(
        lo.is_none_or(|lo| p95 >= lo) && hi.is_none_or(|hi| p95 <= hi),
        "★ the p95 tile says {p95} but the bars put the 95th percentile in \
         bucket {} ({lo:?}..{hi:?}) — which is exactly the inconsistency the \
         reference publishes",
        binned.label(bucket)
    );
}

/// R1797 — the derivation lands on the reference's three published landmarks.
///
/// The figures are the oracle, not the output: the record was chosen so that
/// deriving p50, p95 and the maximum from it reproduces what the reference
/// states, which is what makes "the structure is reproduced" checkable rather
/// than asserted.
#[test]
fn r1797_the_derived_tiles_are_the_references_published_figures() {
    let quantiles =
        pinion_chart::Quantiles::of(spec::LATENCY_SAMPLES, pinion_chart::QuantileMethod::Linear)
            .expect("the record is finite");
    let p50 = quantiles.at(0.50).expect("p50");
    let p95 = quantiles.at(0.95).expect("p95");
    assert!(
        (p50 - spec::LATENCY_P50).abs() < 1e-9,
        "p50 {p50} vs the reference's {}",
        spec::LATENCY_P50
    );
    assert!(
        (p95 - spec::LATENCY_P95).abs() < 1e-9,
        "p95 {p95} vs the reference's {}",
        spec::LATENCY_P95
    );
    assert!((quantiles.max() - spec::LATENCY_MAX).abs() < 1e-9);
    assert_eq!(quantiles.n(), spec::LATENCY_SAMPLES.len());
    assert_eq!(
        spec::LATENCY_STAT_KEYS.len(),
        3,
        "and the card lays out one tile per landmark"
    );
}

/// ★★★★★ R1797 — the default quantile method **cannot answer this card**, and
/// the card is what makes that concrete.
///
/// `QuantileMethod::Tukey` is the crate's default because it is what Tukey
/// defined the box plot on, and a hinge exists only at the quartiles. The card
/// needs p95. Before this round the crate had no way to ask for one at all;
/// now it has one that refuses under the wrong method rather than interpolating
/// a number that definition never defined.
#[test]
fn r1797_the_default_method_refuses_the_percentile_this_card_needs() {
    let tukey = pinion_chart::Quantiles::of(
        spec::LATENCY_SAMPLES,
        pinion_chart::QuantileMethod::default(),
    )
    .expect("the record is finite");
    assert_eq!(tukey.method(), pinion_chart::QuantileMethod::Tukey);
    assert_eq!(
        tukey.at(0.95),
        Err(pinion_chart::QuantileError::HingesOnly(0.95)),
        "and it refuses by name instead of inventing a hinge"
    );
    assert!(
        tukey.at(0.5).is_ok(),
        "while the median, which IS a hinge, is answerable under either"
    );
}

/// R1797 — the emphasised bars are the ones at or above the p95 tile.
///
/// Both halves come from the same samples, so the caption's claim — *tail above
/// p95* — is checkable against the tile printed two rows up. The reference
/// writes `i >= 6` and its caption cannot be checked against anything.
#[test]
fn r1797_the_emphasised_bars_are_the_ones_the_tile_names() {
    let binned = pinion_chart::Binned::over(
        spec::LATENCY_SAMPLES,
        spec::LATENCY_LADDER,
        pinion_chart::BinEnds::Open,
    )
    .expect("binnable");
    let tail = binned.tail_from(spec::LATENCY_P95);
    assert!(!tail.is_empty(), "this record's tail is not empty");
    assert_eq!(tail.end, binned.bins(), "and it runs to the last bin");
    for k in tail.clone() {
        let (lo, _) = binned.extent(k).expect("a bin in range");
        assert!(
            lo.is_some_and(|lo| lo >= spec::LATENCY_P95),
            "bin {} is emphasised but starts below the tile's {}",
            binned.label(k),
            spec::LATENCY_P95
        );
    }
    let below = tail.start - 1;
    let (lo, _) = binned.extent(below).expect("the bin under the tail");
    assert!(
        lo.is_some_and(|lo| lo < spec::LATENCY_P95),
        "and the bin below it is not emphasised"
    );
    assert!(
        spec::LATENCY_CAPTION.contains("p95"),
        "so the caption can say WHY a bar is amber: {:?}",
        spec::LATENCY_CAPTION
    );
}

/// ★ R1797 — the record's slowest sample is **drawn**, not dropped.
///
/// The ladder stops at 64 ms and the slowest reply is 72. Under a closed ladder
/// that sample falls out, and the `max` tile would then report a measurement no
/// bar accounts for — a card contradicting itself in the other direction. The
/// open end is what makes the tile and the bars the same distribution.
#[test]
fn r1797_the_slowest_reply_is_in_a_bin_rather_than_dropped() {
    let open = pinion_chart::Binned::over(
        spec::LATENCY_SAMPLES,
        spec::LATENCY_LADDER,
        pinion_chart::BinEnds::Open,
    )
    .expect("binnable");
    assert_eq!(
        open.counts().iter().sum::<u32>() as usize,
        spec::LATENCY_SAMPLES.len(),
        "every measurement is on the chart"
    );
    assert_eq!(open.outside(), (0, 0));

    let closed = pinion_chart::Binned::over(
        spec::LATENCY_SAMPLES,
        spec::LATENCY_LADDER,
        pinion_chart::BinEnds::Closed,
    )
    .expect("binnable");
    let (below, above) = closed.outside();
    assert!(
        above > 0 && below > 0,
        "★ and a closed ladder really would drop some: {below} below, {above} above"
    );
    assert!(
        spec::LATENCY_MAX > spec::LATENCY_LADDER[spec::LATENCY_LADDER.len() - 1],
        "the maximum tile is past the last boundary, which is why this matters"
    );
}

/// R1797 — the card paints its tiles, its bins and its caption.
///
/// The body is driven through the real painter at the size the board gives it,
/// so a card that computed everything correctly and drew none of it fails here.
#[test]
fn r1797_the_latency_body_paints_its_three_parts() {
    let palette = probe_palette();
    let rect = Rect::new(0, 0, 330, 210);
    let scenes = super::latency_body("latency#4", rect, palette);
    // `Scene::tags` is the framework's own depth-first walk (R1650), so this
    // cannot disagree with the walk the census and the wire use.
    let tags: Vec<String> = scenes
        .iter()
        .flat_map(pinion_core::scene::Scene::tags)
        .collect();
    for expected in [
        "card.latency#4.stat.0",
        "card.latency#4.stat.1",
        "card.latency#4.stat.2",
        "card.latency#4.bins",
        "card.latency#4.caption",
    ] {
        assert!(
            tags.iter().any(|tag| tag == expected),
            "{expected} is not painted; the body drew {tags:?}"
        );
    }
}

/// ★★★★★ R1797 — the distribution the card draws stays inside the box the card
/// gives it, **at every size the board can produce**.
///
/// This isolates the FRAMEWORK question the screen sweep could only report as a
/// symptom: does a bar chart keep its marks inside the rect it was handed. It
/// found that the answer was no — `plot_area` clamped a plot's size and not its
/// origin, so a rect narrower than the label gutters produced an axis outside
/// its own rectangle and every mark aligned to it followed. The repair is in
/// `pinion_chart::window`; this is the consumer-side check that the card never
/// asks for a size the chart cannot honour.
///
/// ★ It walks the chart directly rather than the body's scenes. A body scene is
/// a container laid out absolutely with children in ITS frame, so walking one
/// in isolation answers in container-local coordinates — the first draft
/// compared those against the body's absolute rect and reported 156 findings
/// that were an arithmetic mistake in the test. Where the body's own marks sit
/// on a real screen is `crate::painted`'s question, and it has the regions to
/// answer it.
#[test]
fn r1797_the_cards_distribution_stays_inside_the_box_it_is_given() {
    let (binned, _) = super::latency_binned().expect("the specification's record bins");
    let bars = binned.bars();
    let style = pinion_chart::ChartStyle::default();
    let mut escaping = Vec::new();
    let mut drawn = 0_u32;
    let mut refused = 0_u32;
    // The board's own range and past both ends of it: a card is one to twelve
    // columns wide and one to six rows tall, and the chart gets what the tiles
    // and the caption leave. The sizes the card REFUSES are swept too, so the
    // count below shows the guard is doing work rather than the sweep having
    // missed the small end.
    for w in [1_u32, 8, 24, 40, 69, 80, 120, 200, 330, 640, 1080] {
        for h in [1_u32, 8, 24, 40, 57, 60, 100, 140, 210, 400, 760] {
            // ★ The card's OWN predicate, not a second copy of it. What this
            // asserts is exactly "the card never asks for a size the chart
            // cannot honour", and a test that decided for itself which sizes
            // were reasonable would be asserting a different sentence.
            let Some(box_) = super::distribution_box(Rect::new(0, 0, w, h), 0, bars.len(), &style)
            else {
                refused += 1;
                continue;
            };
            drawn += 1;
            let scene = pinion_chart::BarChart::new(bars.clone())
                .with_tag_prefix("probe")
                .build(Rect::new(0, 0, box_.w, box_.h), &style);
            scene.for_each_node(&mut |visit| {
                let Some(at) = visit.absolute_rect() else {
                    return;
                };
                if at.x + at.w > box_.w || at.y + at.h > box_.h {
                    escaping.push((
                        box_.w,
                        box_.h,
                        visit.node.tag().unwrap_or("<untagged>").to_owned(),
                        at,
                    ));
                }
            });
        }
    }
    assert!(
        escaping.is_empty(),
        "{} mark(s) painted outside the box the chart was given: {:?}",
        escaping.len(),
        &escaping[..escaping.len().min(8)]
    );
    // ★★ Both counts, because an empty finding list is also what a sweep that
    // drew nothing reports. The guard has to have admitted sizes AND refused
    // some, or this test passes for a reason that has nothing to do with
    // containment.
    assert!(
        drawn > 0 && refused > 0,
        "the sweep drew {drawn} and refused {refused}; it has to do both for the \
         empty finding list above to mean anything"
    );
}

/// ★ R1797 — the silence census's populations are the chart's own numbers.
///
/// Two counts have to agree with the painter or the census demands regions
/// nothing paints: how many buckets the ladder produces, and how many value
/// ticks the chart draws. Both are written in `spec` because a population needs
/// a number; this is what stops them being a second, drifting copy.
#[test]
fn r1797_the_silence_populations_match_what_the_chart_paints() {
    let (binned, _) = super::latency_binned().expect("the specification's record bins");
    assert_eq!(
        binned.bins(),
        spec::LATENCY_LADDER.len() + 1,
        "the ladder's boundaries plus two open ends is what `LatencyBins` counts"
    );
    assert_eq!(
        pinion_chart::ChartStyle::default().y_ticks,
        spec::LATENCY_Y_TICKS,
        "★ the tick count the census declares is the one the painter asks for"
    );
}

/// ★ R1797 — the card is on the board, and the board still places every
/// placeable kind.
#[test]
fn r1797_the_promoted_card_is_placed_and_the_palette_has_nothing_left() {
    let def = def_of("latency").expect("the catalogue still has it");
    assert_eq!(def.tier, spec::Tier::Placeable);
    assert!(
        def.reserved_for.is_empty(),
        "a promoted entry states no booking"
    );
    assert!(
        spec::BOARD.iter().any(|tile| tile.kind == "latency"),
        "and the opening board places it"
    );
    assert_eq!(spec::BOARD.len(), spec::placeable_count());
    // ★ Its section now holds both releases, which is the thing the tier column
    // this round removed could not represent.
    assert_eq!(spec::section_tiers("visual"), (true, true));
    assert!(
        spec::section_heading("visual", "VISUALIZATION").contains("1 + 2"),
        "and the heading a reader scans says so: {:?}",
        spec::section_heading("visual", "VISUALIZATION")
    );
}

// ── R1806 — click to cross-filter every linked view ──────────────────────────
//
// The census row `dashboard.t2.4`. The word that had no referent in this tree
// was **every**: a cross-filter was an imperative `.select_x_range(..)` written
// once per chart, so the set it reached was whatever somebody had remembered to
// write, and a card added and forgotten rendered unfiltered in silence.
//
// These assert the reach as a **set**, not as a count. A count of two is
// equally true of the right two cards and the wrong two.

/// The kinds the opening board actually places, which is the population the
/// link declaration must cover.
fn placed_kinds() -> std::collections::BTreeSet<String> {
    spec::BOARD
        .iter()
        .map(|tile| tile.kind.to_string())
        .collect()
}

#[test]
fn r1806_a_saved_filter_reaches_the_declared_set_of_linked_views() {
    let group = spec::dashboard_links();
    let reach = group.publish(&pinion_chart::Selection::Category("units only".into()));

    assert_eq!(
        reach
            .reached()
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["filter", "packet"]),
        "★ a saved filter reaches the SET of category-speaking views — and the \
         stream is a DIFFERENT card from the one the chip lives on, which is the \
         whole of the census sentence"
    );
    assert_eq!(
        reach
            .refused()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["alarms", "decode", "health", "keymap", "latency"]),
        "and the ones it does not reach are named too, rather than merely absent"
    );

    // ★★★★★ The accounting identity. Before this round a card could fall out of
    // a cross-filter without appearing anywhere; here every declared view is in
    // exactly one of the two halves, so "every linked view" is checkable.
    assert_eq!(
        reach.accounted(),
        group.declared(),
        "every declared view is accounted for"
    );
    for name in reach.reached() {
        assert!(
            !reach.refused().contains_key(name),
            "{name} is both reached and refused"
        );
    }
}

#[test]
fn r1806_the_same_board_reaches_a_different_set_in_a_different_domain() {
    // The latency card is not un-filterable — it is not filterable BY CATEGORY.
    // A millisecond window reaches it and leaves the filter card behind, which
    // is what makes the refusal above a statement about domains rather than a
    // ranking of cards.
    let reach =
        spec::dashboard_links().publish(&pinion_chart::Selection::XRange { lo: 8.0, hi: 16.0 });
    assert_eq!(
        reach
            .reached()
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["latency", "packet"]),
    );
}

#[test]
fn r1806_a_refusal_says_why_and_the_two_kinds_are_distinguishable() {
    let reach =
        spec::dashboard_links().publish(&pinion_chart::Selection::Category("units only".into()));

    let latency = reach
        .reason("latency")
        .expect("a refused view has a reason");
    assert!(
        latency.contains("x-range") && latency.contains("category"),
        "★ the refusal names BOTH sides — what the card selects by, and what was \
         published: {latency:?}"
    );
    assert!(
        matches!(
            reach.refused().get("latency"),
            Some(pinion_chart::Refusal::Domain { .. })
        ),
        "the latency card is a real view over the capture that speaks another domain"
    );

    let keymap = reach
        .reason("keymap")
        .expect("an inert view has a reason too");
    assert_eq!(keymap, "a key legend, not capture data");
    assert!(
        matches!(
            reach.refused().get("keymap"),
            Some(pinion_chart::Refusal::Inert { .. })
        ),
        "★ and 'cannot answer THIS question' is kept distinct from 'is not part \
         of this population' — before this round both were the same silence"
    );
}

/// ★★★★★ The gate the per-view call could never have: the declaration compared
/// against the board that was actually **placed**.
///
/// A sixth card added to `spec::BOARD` and forgotten in
/// `spec::dashboard_links` would render unfiltered forever, and nothing in this
/// tree would have said so. This is the sentence instead of the silence.
#[test]
fn r1806_the_link_declaration_covers_the_placed_board() {
    let audit = spec::dashboard_links().audit(&placed_kinds());
    assert!(
        audit.agrees(),
        "the link declaration and the placed board disagree: {}",
        audit.fault().unwrap_or_default()
    );
    // And the gate is not vacuous — the board really does place cards.
    assert_eq!(placed_kinds().len(), spec::BOARD.len());
}

#[test]
fn r1806_every_saved_filter_has_exactly_one_rule() {
    assert_eq!(
        spec::FILTER_CHIP_RULES.len(),
        spec::FILTER_CHIPS.len(),
        "★ one rule per chip, asserted rather than trusted — a chip with no rule \
         is a chip whose click means nothing to any other card"
    );
    for n in 0..spec::FILTER_CHIPS.len() {
        assert!(spec::chip_rule(n).is_some(), "chip {n} has no rule");
    }
    assert!(spec::chip_rule(spec::FILTER_CHIPS.len()).is_none());
}

/// ★★★★★ The painted half: choosing a saved filter **fades the stream rows it
/// does not select** — on a card the chip does not live on.
///
/// Asserted as the two SETS of row indices, derived from the chip's own stated
/// rule, so the test cannot agree with a painter that faded the wrong rows or
/// all of them.
#[test]
fn r1806_choosing_a_saved_filter_fades_the_rows_it_does_not_select() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let packet = state
            .cards
            .get()
            .into_iter()
            .find(|c| kind_of(c.id().as_str()) == "packet")
            .expect("the opening board places the stream card");
        let id = packet.id().as_str().to_string();

        // Which rows a given chip's rule selects, straight from the specification.
        let selected_by = |n: usize| -> std::collections::BTreeSet<usize> {
            let rule = spec::chip_rule(n).expect("the chip has a rule");
            spec::STREAM_ROWS
                .iter()
                .enumerate()
                .filter(|(_, (_, kind, name, _))| rule.selects(kind, name))
                .map(|(i, _)| i)
                .collect()
        };

        // Which rows the SCENE actually faded, read off the painted ink.
        let faded_rows = || -> std::collections::BTreeSet<usize> {
            let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
            let faded = probe_faded_ink();
            let mut out = std::collections::BTreeSet::new();
            scene.for_each_node(&mut |visit| {
                let Some(tag) = visit.node.tag() else { return };
                let Some(rest) = tag.strip_prefix(&format!("card.{id}.cell.")) else {
                    return;
                };
                let Some((row, _)) = rest.split_once('_') else {
                    return;
                };
                let Ok(row) = row.parse::<usize>() else {
                    return;
                };
                if let pinion_core::Scene::Text(text) = visit.node
                    && text.style.fg_color == faded
                {
                    out.insert(row);
                }
            });
            out
        };

        // The opening state lights chip 0 ("units only"), so the board opens
        // ALREADY cross-filtered — which is the honest reading of a saved filter
        // that the specification says is on.
        assert_eq!(state.filter_chip.get(), Some(0));
        let painted_rows: std::collections::BTreeSet<usize> =
            (0..spec::STREAM_ROWS.len()).collect();
        assert_eq!(
            faded_rows(),
            painted_rows
                .difference(&selected_by(0))
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            "★ exactly the rows the lit filter does NOT select are faded"
        );

        // A different filter fades a different set — so the painter is reading
        // the rule and not a constant.
        super::ShellOracle::choose_filter(&state, &id_of_filter_card(&state), 4);
        assert_eq!(state.filter_chip.get(), Some(4));
        let declares_only = selected_by(4);
        assert!(
            !declares_only.is_empty() && declares_only.len() < spec::STREAM_ROWS.len(),
            "the fixture chip must split the rows, or this proves nothing"
        );
        assert_eq!(
            faded_rows(),
            painted_rows
                .difference(&declares_only)
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            "★ a second filter fades a DIFFERENT set of rows"
        );

        // And turning the filter off restores every row to full strength: the
        // crossfilter convention that no selection is not an empty one.
        super::ShellOracle::choose_filter(&state, &id_of_filter_card(&state), 4);
        assert_eq!(state.filter_chip.get(), None);
        assert!(
            faded_rows().is_empty(),
            "with no saved filter applied nothing is outside it"
        );
    });
}

/// ★★★★★ R1824 — **the cross-filter reaches a CHART on this screen, and the
/// chart dims.**
///
/// R1806 made "every linked view" a set that can be named, and R1806's own
/// painted proof (above) is about *rows*. The board's one chart that a saved
/// filter reaches — the filter card's matched-count trend — kept drawing at full
/// strength under every filter, because no chart kind but bar / line / scatter
/// had any way to be told about a selection at all. Measured before the repair,
/// by building every kind this framework ships and reading the fill alphas of
/// its marks: three of ten dimmed anything.
///
/// The trend is of the WHOLE capture ([`MATCH_SERIES_OF`](super::MATCH_SERIES_OF)
/// names the measure), so under any saved filter it describes something other
/// than what the reader is looking at. Dimming it is the honest rendering of
/// that, and it is applied through the one API every kind now answers
/// (`pinion_chart::Mute`) rather than by a call written for this card.
///
/// Read off the PAINTED ink, like its sibling: a test that asked the chart
/// whether it had been told would pass against a chart that stores a selection
/// and paints without it, which is the exact defect.
#[test]
fn r1824_choosing_a_saved_filter_dims_the_trend_that_is_not_of_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let trend_alpha = || -> u8 {
            let scene = super::view(ScreenState::default(), pinion_core::Frame::default());
            let mut found = None;
            scene.for_each_node(&mut |visit| {
                if visit.node.tag() == Some("match.spark.line")
                    && let pinion_core::Scene::Path(path) = visit.node
                    && let Some(stroke) = path.style.stroke.as_ref()
                {
                    found = Some(stroke.color.a);
                }
            });
            found.expect("the filter card paints its trend")
        };

        // The board opens with a saved filter already lit, so it opens with the
        // trend already reading as context.
        assert_eq!(state.filter_chip.get(), Some(0));
        let under_filter = trend_alpha();

        // Turning the filter off is what makes the trend the answer again.
        super::ShellOracle::choose_filter(&state, &id_of_filter_card(&state), 0);
        assert_eq!(state.filter_chip.get(), None);
        let unfiltered = trend_alpha();

        assert!(
            under_filter < unfiltered,
            "★ a trend of the whole capture must read as context while a saved \
             filter is on: {under_filter} under the filter vs {unfiltered} with \
             none"
        );

        // And the reach is what drove it — not the chip signal read directly.
        super::ShellOracle::choose_filter(&state, &id_of_filter_card(&state), 2);
        let reach = spec::dashboard_links().publish(&pinion_chart::Selection::Category(
            spec::FILTER_CHIPS[2].0.to_string(),
        ));
        assert!(
            reach.reaches("filter"),
            "the filter card is a declared view of the board's link group"
        );
        assert!(
            trend_alpha() < unfiltered,
            "a different saved filter reaches it the same way"
        );
    });
}

/// The filter card's id on the opening board — the chip row's address.
fn id_of_filter_card(state: &std::rc::Rc<super::ShellState>) -> String {
    state
        .cards
        .get()
        .into_iter()
        .find(|c| kind_of(c.id().as_str()) == "filter")
        .expect("the opening board places the filter card")
        .id()
        .as_str()
        .to_string()
}

/// The ink a faded row takes, resolved the same way the painter resolves it.
fn probe_faded_ink() -> pinion_core::style::Color {
    let theme = pinion_core::theme::use_theme(super::THEME_TAG).theme_animated();
    theme.resolve(pinion_core::theme::ColorRole::Outline)
}

// ── R1808 — the whole application, walked past its own specification ─────────
//
// ★★★★★ The mechanism for this has been complete since R1767: a walk records
// each departing frame's verdict, folds the live section in, counts a section
// nobody visited as unreproduced, and refuses the application while any is
// unanswered. `ScreenRoster::journey_conformance` is that report, and this
// application PUBLISHES it on the wire (`sections_json`).
//
// Nothing asserted it. Measured at R1808, every `conforms()` assertion in the
// tree lived in `pinion-screen`'s own test file, over SYNTHETIC specifications
// — three fixtures named `inspector`, `stream` and `board`. The three real
// canonical screens have their own judges, their own specification tables and
// their own per-screen tests, and had never once been driven through the walk
// as ONE application. The framework could answer and nobody asked, which is
// this tree's oldest recurring shape and the reason R1794 exists.

/// The whole run, walked once: navigate, paint, record, hand the stop back.
///
/// ★ The two populations are the ROSTER'S, through `Tour` — where to go, and
/// which surfaces the frame must record. The second is the one that fails
/// quietly: a mounted screen whose paint-root tag is missing from the recording
/// is painted, judged, and reported as reproducing nothing, for a reason that
/// has nothing to do with the screen.
/// ★★★★★ R1909 — open every pane the arrived section is showing as folded,
/// through the verb that section publishes.
///
/// What a person does on finding a panel put away. See the call site in
/// [`walk_the_application`] for why a walk has to do it and why it is derived
/// from each screen's own `spec` rather than written for one of them.
///
/// ⚠ A refusal is NOT swallowed. A pane the surface reports folded is one its
/// own policy admitted a fold on, so `unfold` must be admitted too — a refusal
/// here would mean a panel a reader can see put away and cannot bring back,
/// which is precisely the `hide`/`fold` confusion this campaign is about.
pub(crate) fn open_whatever_arrived_folded(state: &std::rc::Rc<super::ShellState>) {
    use pinion_core::external::IntrospectValue;

    let mut externals = state.screens.externals(&state.journey.get());
    for external in &mut externals {
        let Some(surface) = external.handle.introspect_mut() else {
            continue;
        };
        let Ok(spec) = surface.query("spec") else {
            continue;
        };
        let Some(panes) = as_json(spec)["panes"].as_array().cloned() else {
            continue;
        };
        let folded: Vec<String> = panes
            .iter()
            .filter(|pane| pane["at"]["folded"] == serde_json::Value::Bool(true))
            .filter_map(|pane| pane["name"].as_str().map(ToOwned::to_owned))
            .collect();
        for name in folded {
            surface
                .invoke("place", IntrospectValue::Text(format!("{name},unfold")))
                .unwrap_or_else(|why| {
                    panic!(
                        "a pane this surface reports FOLDED refused to unfold: \
                         {name} — {why:?}. A fold a reader cannot undo is a hide"
                    )
                });
        }
    }
}

pub(crate) fn walk_the_application(
    state: &std::rc::Rc<super::ShellState>,
) -> pinion_screen::TourReport {
    let tour = pinion_screen::Tour::of(&state.screens).also_recording(super::VIEW_TAG);
    let surfaces = tour.surfaces();
    tour.walk(
        // Navigate only. Painting here would destroy the departing frame the
        // latch is about to read — see `Tour::walk`.
        |key| {
            state.go(key).expect("an open destination is reachable");
            let journey = state.journey.get();
            assert_eq!(journey.at(), key, "the walk arrived where it was sent");
            journey
        },
        // Paint the arrived section, in whichever pose the section asked for,
        // and record its surfaces. How many times this runs is the roster's
        // answer, not a number written here.
        |key, _pose| {
            // ★★★★★ R1909 — **a walk opens what the section arrived folded.**
            //
            // A person who reaches a screen and finds a panel put away opens
            // it; a walk that did not would report every surface inside that
            // panel as never having stood, which is a statement about the walk
            // rather than about the application. R1909 is the round that makes
            // this reachable at all — the node lab's inspector opens folded now
            // — and it took the three surfaces that pane draws to `stood: none`
            // and the whole walk to non-conforming.
            //
            // ⚠ DERIVED, not written for the lab. Each arrived external is
            // asked which of its panes are folded and told to unfold those, so
            // a section that grows a folded pane later joins this without the
            // walk being edited — and a section with none is untouched.
            open_whatever_arrived_folded(state);
            let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
            let mut cache = pinion_runtime::LayoutCache::new();
            pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
            // The cascade the window runs after layout, so a seat's inertness
            // is resolved before anything judges it (why `painted.rs` runs it).
            pinion_core::scene_disabled::resolve_disabled(&mut scene);

            let refs: Vec<&str> = surfaces.iter().map(String::as_str).collect();
            let recorded = pinion_runtime::record_painted_surfaces(&scene, &refs);
            assert!(
                recorded > 0,
                "the frame at `{key}` recorded none of {refs:?}, so every \
                 verdict below would be asked of a store it never filled"
            );
            scene
        },
    )
}

/// ★★★★★ **The three canonical screens, judged as one application.**
///
/// North-star condition (4). Screen A is the node lab (`lab`, mounted), screen
/// B the capture viewer (`packets`, mounted) and screen C the dashboard
/// (`dashboard`, a page this shell paints itself and judges with `BoardJudge`).
/// They reach this assertion by the same route as every other open section,
/// because the itinerary is the roster's and not a list written here.
#[test]
fn r1808_the_application_reproduces_every_section_over_one_walk() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
    });
}

/// The walk really did stand in all three canonical screens — asserted as a
/// SUBSET relation over the itinerary, so the test above cannot pass by
/// covering an application that no longer has them.
#[test]
fn r1808_the_walk_covers_the_three_canonical_screens() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let report = walk_the_application(&state);

        let visited: std::collections::BTreeSet<&str> =
            report.visited().iter().map(String::as_str).collect();
        for key in ["lab", "packets", "dashboard"] {
            assert!(
                visited.contains(key),
                "the walk never stood in `{key}`; it visited {visited:?}"
            );
        }
        assert!(report.covered(), "{}", report.why().unwrap_or_default());
        assert!(report.missed().is_empty() && report.strayed().is_empty());
    });
}

/// ★★★★★ The populations are DERIVED, and this is what says so.
///
/// The itinerary must equal the roster's own open destinations and the surface
/// list must hold every mounted screen's paint-root tag. A list written by hand
/// beside the application is a list a section falls off, and the failure that
/// follows is silent for the surfaces half.
#[test]
fn r1808_the_tour_takes_both_populations_from_the_roster() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let tour = pinion_screen::Tour::of(&state.screens).also_recording(super::VIEW_TAG);

        let open: Vec<String> = state
            .screens
            .destinations()
            .open()
            .map(|destination| destination.key.to_string())
            .collect();
        assert_eq!(tour.itinerary(), open, "the itinerary is the roster's");
        assert!(
            open.len() >= 3,
            "the fixture must have sections to walk, or every assertion here is \
             vacuous: {open:?}"
        );

        let surfaces = tour.surfaces();
        for key in state.screens.mounted_keys() {
            let tag = state
                .screens
                .tag_of(key)
                .expect("a mounted key has a paint root");
            assert!(
                surfaces.iter().any(|held| held == tag),
                "`{key}`'s paint root `{tag}` is not in the recording list, so \
                 its section would judge a store nobody filled"
            );
        }
        assert!(
            surfaces.iter().any(|held| held == super::VIEW_TAG),
            "and the host's own page surface is recorded too"
        );

        // ★ The third population: how many frames each section needs. The node
        // lab's specification names a value row and that row's OPEN roster, and
        // the roster is the row's open state — mutually exclusive, so one frame
        // cannot carry both. The roster answers this; nothing here counts it.
        assert_eq!(
            state.screens.poses_of("lab"),
            2,
            "the node lab declares the two states its specification describes"
        );
        assert_eq!(
            state.screens.poses_of("packets"),
            1,
            "a section whose surfaces coexist asks for one frame"
        );
        assert_eq!(
            state.screens.poses_of("dashboard"),
            1,
            "a page the host paints itself has no screen to ask, and gets one"
        );
    });
}

// ── R1812 — every caption in the application, over the roster's own walk ─────
//
// ★★★★★ The debt R1792 left open said the escape check *"runs on one screen of
// five"*, and that arming it elsewhere was one line. Both halves were true and
// the conclusion was not: armed over the assembled application it reports SEVEN
// escapes, and measured one by one at R1812 **every one is a false positive** —
// four pair a run of this shell's own chrome with a box inside a mounted screen,
// two treat a tagged text node as a box and pair it with an open dropdown's
// option label. A gate that cries seven times is a gate somebody switches off.
//
// So the repair was not to arm the old check four more times. It was to give the
// pairing a rule about the TREE (`caption::Survey`), and then to arm the result
// ONCE, here, where the population is the roster's.

/// How many caption/box pairs in this application **say where they sit** — a
/// ratchet, and it may only ever go UP.
///
/// ★★★★★ This is the number the debt carried as prose, and prose does not
/// ratchet: it said *225 of 230* at R1792 and nothing re-measured it for twenty
/// rounds. A caption that declares nothing is not wrong — it is *unanswerable*,
/// and `off-centre` and `deliberately left` are the same picture while it stays
/// that way. Raising this constant is what adopting `caption::Caption::align`
/// at a site buys.
///
/// ★★★★★ **R1813 — it was `SILENT_CAPTIONS`, a ceiling on the complement, and
/// the instrument caught its own defect within one round of being written.**
/// Bonding the two frame captions raised `silent` from 152 to 154: they had
/// FALLEN OUT of the watched population (R1812's own repair narrowed their runs
/// past the scale rule), and bonding RETURNED them — as pairs that declare no
/// alignment. So a repair that is unambiguously progress raised the number a
/// ratchet had sworn could only fall.
///
/// The complement is not monotone because it is measured over a population the
/// work itself widens. This is, in both directions: declaring at a site raises
/// it, and widening the population never lowers it. Ratchet what the work
/// controls, and state it as what you HAVE rather than what you lack.
///
/// ⚠ **Its ceiling is not the pair count, and that is a property of the
/// instrument.** `TextAlign::Start` is the framework default, so a caption that
/// is at the start *on purpose* — the role chips' name-over-gist rows, every
/// left-aligned list cell — cannot say so: its declaration is spelled exactly
/// like silence. This number can therefore only ever rise to the count of
/// captions that are centred or end-aligned, and nothing here knows what that
/// count is. [`ADJACENT_CAPTIONS`] is the ratchet whose bound really is zero.
const CLAIMING_CAPTIONS: usize = 16;

/// How many caption/box pairs are held together by **geometry alone** — a
/// second ratchet, and the one whose floor really is zero.
///
/// ★★★★★ Where [`CLAIMING_CAPTIONS`] is bounded by what `TextAlign` can
/// express, this one is not: every caption in this application *could* be a
/// child of the box a reader sees around it, and each one that becomes one moves
/// from a guess to a fact. It is also the number that keeps the gate honest in
/// the other direction — a `Bond::Declared` caption cannot escape, so as this
/// falls the escape check has less and less to look at, and a `0 escapes`
/// verdict quietly stops meaning anything unless the split is on the record.
///
/// ⚠ **R1812 set it at 152 after watching it fall from 154, and the fall was
/// not a repayment.** Deriving the frame caption's width from its box made the
/// run its own word's width instead of the whole seat, and a narrower run no
/// longer passes the scale rule against a wide frame — so the caption became
/// correct by construction and stopped being *watched* in the same edit.
///
/// ★★★★★ **R1813 closed that by bonding them, and the arithmetic is worth
/// reading**: measured over the assembled application, `bound` rose 16 -> 22
/// while this fell only 152 -> 148, because two of the six had not been in the
/// population at all. A repair can *return* a caption to being watched, and
/// returning one is invisible here — it is visible in `pairs`, 168 -> 170,
/// which is why the gate prints both.
///
/// 🟥 **This paragraph said `+4 / -2` and this constant said 150 until the
/// closing audit re-ran the gate.** Both were true when written — of a tree in
/// which only the two frame tabs had been bonded — and the same round went on
/// to bond the determinism switch, which moved them again. So neither was
/// stale in the usual sense: they were a mid-round snapshot that reads like a
/// result, and the ratchet passed anyway, because `<=` cannot notice a ceiling
/// set two above the floor. ⇒ **a ratchet's constant is a MEASUREMENT, and has
/// to be re-taken at the END of the round that moves it** — the same lesson
/// this round's own module header applied by deleting its figures, applied
/// there and missed here, three files apart, in one commit.
const ADJACENT_CAPTIONS: usize = 148;

/// ★★★★★ **No caption in this application escapes its box or sits somewhere
/// other than where it says it does** — asked of every destination the roster
/// holds, in every pose those destinations ask for.
///
/// This is the debt's carry item 2, and the shape of the answer is the point:
/// the itinerary is `Tour`'s, so a screen added to the roster tomorrow is
/// covered without this file changing, and a screen removed takes its captions
/// out of the population rather than leaving a check that quietly passes.
/// Survey every caption the application paints, over the roster's own walk.
///
/// Split out so the assertions below read as assertions; the populations are
/// still `Tour`'s, which is the property that matters.
fn survey_the_application(
    state: &std::rc::Rc<super::ShellState>,
) -> (
    pinion_widget_paint::caption::Survey,
    usize,
    pinion_screen::TourReport,
) {
    let mut all = pinion_widget_paint::caption::Survey::default();
    let mut stops = 0usize;
    let tour = pinion_screen::Tour::of(&state.screens).also_recording(super::VIEW_TAG);
    let surfaces = tour.surfaces();
    let report = tour.walk(
        |key| {
            state.go(key).expect("an open destination is reachable");
            state.journey.get()
        },
        |_key, _pose| {
            // ★ R1909 — the same opening the conformance walk does, and for the
            // same reason: this survey counts the captions the APPLICATION has,
            // and a pane put away carries its captions out of the count. The
            // ratchet fell 16 -> 10 the moment one pane opened folded, reporting
            // a repair as having been undone.
            open_whatever_arrived_folded(state);
            let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
            let mut cache = pinion_runtime::LayoutCache::new();
            pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
            pinion_core::scene_disabled::resolve_disabled(&mut scene);
            let refs: Vec<&str> = surfaces.iter().map(String::as_str).collect();
            let _ = pinion_runtime::record_painted_surfaces(&scene, &refs);
            all.absorb(pinion_widget_paint::caption::Survey::of(&scene));
            stops += 1;
            scene
        },
    );
    (all, stops, report)
}

#[test]
fn r1812_no_caption_in_the_application_escapes_or_breaks_its_claim() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let (all, stops, report) = survey_the_application(&state);

        // The denominators first (R1800): a green verdict below is worth
        // nothing without them, and this check's population SHRINKS as the
        // repair spreads — every site that adopts `captioned` moves a pair from
        // `adjacent` to `bound`, so "0 escapes" could come to mean "0 looked at".
        assert!(
            stops >= report.itinerary().len(),
            "the walk stood in every destination: {stops} stops for {:?}",
            report.itinerary()
        );
        let mut tags: Vec<&str> = all
            .placements()
            .iter()
            .map(pinion_widget_paint::caption::Placement::box_tag)
            .collect();
        tags.sort_unstable();
        tags.dedup();
        assert!(
            all.pairs() >= 150 && all.runs() > 900 && tags.len() >= 80,
            "the sweep has a population to judge: {} pairs over {} distinct \
             boxes, of {} runs in {} boxes over {stops} stops",
            all.pairs(),
            tags.len(),
            all.runs(),
            all.boxes()
        );

        // ★★★★★ The anti-vacuity assertion that matters more than the counts:
        // the two boxes a READER actually reported must be in what this gate
        // judges. A pairing rule strict enough to be quiet is a pairing rule
        // that can be strict enough to be blind, and the way to tell those
        // apart is to name the cases the check exists for. `lab.palette.
        // protocol.tcp` is the chip whose word hung 3px off the right edge;
        // `lab.palette.discovery` is the panel whose caption sat flush against
        // its own border.
        for reported in ["lab.palette.protocol.tcp", "lab.palette.discovery"] {
            assert!(
                tags.contains(&reported),
                "`{reported}` is a box a reader reported a caption defect in, \
                 and this gate is not looking at it. Judged: {tags:?}"
            );
        }
        assert_eq!(
            all.bound() + all.adjacent(),
            all.pairs(),
            "every pair is related by the scene or by geometry, and the split \
             is reported rather than averaged: bound={} adjacent={}",
            all.bound(),
            all.adjacent()
        );

        let escaped = all.escaped();
        assert!(
            escaped.is_empty(),
            "{} caption(s) are drawn OUTSIDE the box a reader sees around them: \
             {:?}",
            escaped.len(),
            escaped
                .iter()
                .map(|p| {
                    format!(
                        "{:?} past {} by {:?} -- run {:?} in holder {:?}, bond {:?}",
                        p.text(),
                        p.box_tag(),
                        p.past(),
                        p.run(),
                        p.holder(),
                        p.bond()
                    )
                })
                .collect::<Vec<_>>()
        );

        let broken = all.broken();
        assert!(
            broken.is_empty(),
            "{} caption(s) declare an alignment they are not sitting at — the \
             one form of this defect that can be PROVED rather than suspected, \
             because the scene carries both the claim and the placement: {:?}",
            broken.len(),
            broken
                .iter()
                .map(|p| format!(
                    "{:?} in {} claims {:?} but sits {:?}",
                    p.text(),
                    p.box_tag(),
                    p.claim(),
                    p.room()
                ))
                .collect::<Vec<_>>()
        );

        println!(
            "R1812 pairs={} bound={} adjacent={} silent={} claiming={} runs={} boxes={} tags={} stops={stops}",
            all.pairs(),
            all.bound(),
            all.adjacent(),
            all.silent(),
            all.claiming(),
            all.runs(),
            all.boxes(),
            tags.len()
        );
        assert_the_ratchets(&all);
    });
}

/// The two ratchets, split out so each reads as what it is.
///
/// ★ They move in OPPOSITE directions and that is the point: one counts what
/// the application HAS ([`CLAIMING_CAPTIONS`], a floor), the other what it
/// still lacks ([`ADJACENT_CAPTIONS`], a ceiling). Only the second can honestly
/// reach its bound; the first is capped by what `TextAlign` can express.
fn assert_the_ratchets(all: &pinion_widget_paint::caption::Survey) {
    assert!(
        all.claiming() >= CLAIMING_CAPTIONS,
        "only {} caption(s) say where they sit, DOWN from the {} this \
         application had when the ratchet was set — {} of {} pairs still cannot \
         be told apart from a caption whose author never considered the \
         question. `caption::Caption::align` at a site is what raises it.",
        all.claiming(),
        CLAIMING_CAPTIONS,
        all.silent(),
        all.pairs()
    );
    assert!(
        all.adjacent() <= ADJACENT_CAPTIONS,
        "{} caption(s) are paired with their box by nothing but where they \
         landed, up from the {} this application had when the ratchet was set. \
         `caption::captioned` and `caption::inside` are what turn one into a \
         fact the scene carries.",
        all.adjacent(),
        ADJACENT_CAPTIONS
    );
}

// ── R1973: what the reference gives a reader who cannot see the screen ───────

/// The reference's accessibility surface, pinned. See
/// `docs/analyzer-voice-spec.json`.
///
/// `include_str!` rather than a read at run time, the rule every other pinned
/// reference document here follows: a document that goes missing must break the
/// build rather than let a gate pass by finding no file.
fn voice_pin() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../docs/analyzer-voice-spec.json"))
        .expect("the reference's voice surface is readable JSON")
}

/// ★★★★★ R1973 — **every role the reference declares is reproduced, and what we
/// have beyond it is SURPLUS rather than drift.**
///
/// # Why this is not the gate the round was asked for
///
/// The milestone said: measure `spec::VOICES` against the reference. Measured
/// first, as this project's standing order requires, the premise does not hold —
/// **the reference has almost no accessibility surface to measure against.**
/// Its scope document and its behaviour document declare ZERO role attributes
/// and ZERO `aria-*` attributes between them, and the newest integrated
/// document declares FIVE in thirty-one megabytes: one role, four states.
/// Against that, `spec::VOICES` carries dozens of rows across twenty-odd role
/// kinds. Comparing the two would be measuring a surface against nothing, and
/// the round that tried it would have deleted the difference or invented a
/// reference for it.
///
/// ⇒ so what is gated is the pair of things that ARE true and were not asked of
/// anything before: the one role the reference does declare is reproduced *by
/// name in the tree a reader actually gets*, and our surface is a strict
/// superset. The second half is what the standing order needs and nothing had —
/// *do not delete what the reference lacks and we have* is unenforceable while
/// nobody can tell surplus from divergence, and a round that quietly removed a
/// voice would have closed the distance with no gate noticing.
///
/// ⚠ The reference's four STATE attributes are deliberately not asserted here.
/// A state is a property of a control at a moment (`selected`, `pressed`), so
/// asking it of a boot-time tree would pin whichever tab happens to open —
/// that is `r1696`'s axis and the keyboard walk's, not this one's.
#[test]
fn r1973_every_role_the_reference_declares_is_reproduced_and_the_rest_is_surplus() {
    let pin = voice_pin();
    let canon_roles: Vec<String> = pin["canon"]["roles"]
        .as_array()
        .expect("the pin lists the reference's roles")
        .iter()
        .map(|r| r.as_str().expect("a role is a string").to_owned())
        .collect();
    assert!(
        !canon_roles.is_empty(),
        "the pin declares no reference role, so this gate compares nothing — an \
         empty reference list is how a comparison stops happening silently",
    );

    // What the tree a reader gets actually announces, by role, at boot.
    let owner = Owner::new();
    owner.run(|| {
        let nodes = AnalyzerShellView::access_node(&ScreenState::default(), None);
        // ★★★★★ R2027 — `aria_name`, not the Debug spelling, and the
        // difference is not cosmetic: `spec::VOICES` and the pin are written in
        // W3C words, and a role's Rust name is not one — `AriaRole::TextInput`
        // is `textbox` and `ListboxOption` is `option`. This line read
        // `format!("{:?}", …)` and passed only because `TabList` happens to
        // lowercase to its own W3C word. Measured at R2027: over the whole
        // application the Debug spelling disagrees with the declaration 24
        // times and `aria_name` zero times, so the gate below was comparing two
        // vocabularies and would have gone red the day the reference declared
        // any role whose two spellings differ.
        let announced: std::collections::BTreeSet<String> = nodes
            .iter()
            .map(|node| node.role.aria_name().to_owned())
            .collect();
        assert!(
            !announced.is_empty(),
            "the screen announces no accessibility node at all, so every \
             assertion below is about an empty tree",
        );
        for role in &canon_roles {
            assert!(
                announced.contains(role),
                "the reference declares {role:?} and this screen's \
                 accessibility tree announces {announced:?} — the one \
                 obligation the reference's own accessibility surface carries \
                 is not reproduced",
            );
        }

        // ★★ The surplus, from the SAME source the reproduction arm reads.
        //
        // ⚠ A first draft took this from `spec::VOICES` while the arm above
        // read the tree, and a mutation caught it: two populations for one
        // question, which is the duplication rule applied to a gate rather than
        // to a screen. The tree is the right one — a role a reader is never
        // handed is not a surface we have, whatever a table says — and it also
        // makes the two arms falsifiable by the same edit.
        let surplus: Vec<&String> = announced
            .iter()
            .filter(|role| !canon_roles.contains(role))
            .collect();
        assert!(
            pin["ours"]["surplus_must_be_positive"]
                .as_bool()
                .expect("the pin states whether a surplus is required"),
            "the pin stopped requiring a surplus, which is the only thing \
             keeping a deletion from reading as parity",
        );
        assert!(
            !surplus.is_empty(),
            "this screen announces {} role kind(s) and the reference declares \
             {}: nothing is left over, so either the reference grew or a round \
             deleted a voice — and the standing order is that what the \
             reference lacks and we have is NOT removed",
            announced.len(),
            canon_roles.len(),
        );
    });
}

/// ★★★★★ R2027 — **the specification's `role` column is checked**, which is
/// what makes the surplus a thing that cannot silently shrink.
///
/// # What was unguarded, measured rather than assumed
///
/// `debt-the-surplus-ratchet-says-non-empty-not-non-shrinking` recorded that
/// R1973's gate asserts only *the surplus is non-empty* — floor one — so 21
/// role kinds falling to 2 would pass. It asked for a ratchet and warned that
/// writing our own count into the pin would be the same fact in two files, the
/// class this repository has dozens of open debts about.
///
/// The round's first act was to ask WHO READS `VoiceSpec::role`. **One site:**
/// `main.rs` publishes it on the wire as `"role": voice.role`. Nothing compared
/// it with the tree, so a column of the specification was a published claim
/// that no gate could falsify — and a round that changed a region's role would
/// shrink the surplus with `r1695` (which checks tags) staying green.
///
/// ⇒ so the ratchet is not a number. It is this: **every region the
/// specification declares is announced with the role it declares.** A voice
/// kind can then only leave by an edit to `spec::VOICES`, which is the same
/// visibility every other column of that table has, and no count lives in two
/// places. R2020's rule, applied one axis over: what a derived gate must pin is
/// the RULE, not the table's own numbers.
///
/// ⚠★★★★★ AND THE VOCABULARIES WERE DIFFERENT, which is why nobody could have
/// compared them by accident. `spec::VOICES` writes W3C words; a role's Rust
/// name is not one (`AriaRole::TextInput` is `textbox`, `ListboxOption` is
/// `option`). Probed at the open, comparing the Debug spelling reported **24
/// mismatches** across 8 destinations; comparing `AriaRole::aria_name` — the
/// name this framework actually publishes — reports **zero**. R1973 compares
/// the Debug spelling and passes only because `TabList` happens to lowercase to
/// its own W3C word; see the note this round left on it.
#[test]
fn r2027_every_declared_region_is_announced_with_the_role_it_declares() {
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_shell_state();
        let canon: std::collections::BTreeSet<String> = voice_pin()["canon"]["roles"]
            .as_array()
            .expect("the pin lists the reference's roles")
            .iter()
            .map(|r| r.as_str().expect("a role is a string").to_owned())
            .collect();
        let declared: std::collections::BTreeSet<String> =
            spec::VOICES.iter().map(|v| v.role.to_owned()).collect();
        let mut announced: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::default();
        let mut wrong: Vec<String> = Vec::new();
        let mut checked = 0_usize;

        let destinations = spec::destinations();
        for here in destinations.open() {
            let key = here.key.as_ref();
            state.go(key).expect("an open destination is reachable");
            // ★ Both occupancies of the status slot, the reason `r1695` reads
            // them: the tree carries whichever of the slot's two occupants is
            // painted, so a reading taken in one state reports the other's
            // region as unannounced. Measured — without this, eight rows of
            // `shell.status.gesture` come back MISSING and none of them is.
            let mut by_tag: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::default();
            for _ in 0..2 {
                for node in AnalyzerShellView::access_node(&ScreenState::default(), None) {
                    by_tag.insert(node.tag.clone(), node.role.aria_name().to_owned());
                }
                pinion_core::reactive::Owner::current()
                    .expect("a pose is taken inside an Owner scope")
                    .tick_animations(state.toast.life() + 1.0);
            }
            announced.extend(by_tag.values().cloned());
            for voice in spec::VOICES {
                if !voice.at.shows_at(key) {
                    continue;
                }
                for member in voice.population.members() {
                    let tag = voice.tag.replace("{}", &member);
                    checked += 1;
                    match by_tag.get(&tag) {
                        Some(got) if got == voice.role => {}
                        Some(got) => wrong.push(format!(
                            "{key}: {tag} is declared {:?} and announced {got:?}",
                            voice.role
                        )),
                        // `r1695` owns *is it announced at all*; this row is
                        // here so a missing tag cannot pass THIS gate by having
                        // no role to disagree with.
                        None => wrong.push(format!(
                            "{key}: {tag} is declared {:?} and not announced",
                            voice.role
                        )),
                    }
                }
            }
        }
        // ★ The floor first: an empty comparison reads as agreement.
        assert!(
            checked > 100,
            "{checked} declared region(s) were compared, which is too few to \
             have covered this specification",
        );
        assert!(
            wrong.is_empty(),
            "★★★★★ {} declared region(s) do not carry the role the \
             specification gives them, of {checked} compared: {wrong:?}. The \
             `role` column is published on the wire (`\"role\": voice.role`), so \
             a region whose tree disagrees with it makes this application say \
             two different things about one region.",
            wrong.len(),
        );

        // ★★★★★ AND THAT IS THE RATCHET. Every role kind the specification
        // declares is announced, so a surplus over the reference cannot shrink
        // without an edit to `spec::VOICES` — no count is written anywhere, and
        // the floor is the declaration rather than a remembered number.
        let unannounced: Vec<&String> = declared.difference(&announced).collect();
        assert!(
            unannounced.is_empty(),
            "the specification declares {unannounced:?} and no destination \
             announces them, so the surplus this application claims over the \
             reference is larger than the one a reader is handed",
        );
        let surplus: Vec<&String> = declared.difference(&canon).collect();
        assert!(
            surplus.len() > 1,
            "the specification declares {} role kind(s) and the reference {}: a \
             surplus of {} is the floor R1973 could already assert, and this \
             gate exists because that floor is one — {surplus:?}",
            declared.len(),
            canon.len(),
            surplus.len(),
        );
        // ⚠★★★★★ AND THE REFERENCE'S OWN ROLE IS NOT ONE OF THOSE — asked, and
        // the answer is a fact about the two tables rather than a gap.
        //
        // A first draft asserted `canon ⊆ declared` and added a `tablist` row
        // for `shell.appbar.tabs`. Three gates refused it in one run: this
        // table is of regions the screen PAINTS (`r1695`: *the specification
        // gives this destination X and the screen does not paint it*), and the
        // strip has no box of its own — it is a grouping node whose bounds come
        // from the tabs inside it. So the reference's single obligation is
        // carried by a node `spec::VOICES` cannot describe, the two
        // comparisons join on the TREE, and that is where `r1973` asks it.
        //
        // Asserted the other way round, which is the claim that IS true here:
        // no role the reference declares may be one this application announces
        // NOWHERE.
        assert!(
            canon.iter().all(|role| announced.contains(role)),
            "the reference declares {canon:?} and this application announces \
             {announced:?} across every destination: a reference role nothing \
             announces is the reproduction obligation unmet",
        );
    });
}

// ── R1832: every locked seat cites a requirement the register books ──────────

/// The deferred register, parsed. See `docs/analyzer-reserved-spec.json`.
fn reserved_pin() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../docs/analyzer-reserved-spec.json"))
        .expect("the deferred register is readable JSON")
}

/// The requirement number a `reserved_for` string cites.
///
/// The tree spells it `requirement N` in neutral words and the register keys on
/// the number. Parsing rather than string-matching is what lets the two use
/// different vocabulary for one fact, which is the point of the neutralisation.
fn cited(reserved_for: &str) -> Option<u64> {
    reserved_for
        .strip_prefix("requirement ")?
        .trim()
        .parse()
        .ok()
}

/// ★★★★★ **Every locked seat cites a requirement the register books, and books
/// to that seat.**
///
/// The defect this ends is a false statement to a reader: a locked seat's
/// requirement number is the only thing the screen says about what will fill
/// it, and until this gate nothing compared one against anything. The rail's
/// two carried their numbers in a `$note` — prose — and the palette's carried
/// theirs only in the Rust table this asserts against.
///
/// ★★★★★ THE DEBT THAT ASKED FOR THIS HAD ITS DIAGNOSIS BACKWARDS, which the
/// entry re-measurement found: it recorded that the reference defers SIX
/// requirements and that `admin`'s citation of 14 was therefore wrong. The
/// reference defers FIFTEEN and says so in one sentence; the six are what the
/// palette caption happens to enumerate. 14 books the admin query, so the
/// citation it called a defect is correct — as this gate now proves for every
/// one of them, rather than for the one somebody happened to look at.
#[test]
fn r1832_every_locked_seat_cites_a_requirement_the_register_books_to_it() {
    let pin = reserved_pin();
    let deferred = pin["deferred"]
        .as_array()
        .expect("the register declares a deferred array");

    // requirement -> the seat the register books it to, where it books one.
    let booked: std::collections::BTreeMap<u64, &str> = deferred
        .iter()
        .filter_map(|row| Some((row["requirement"].as_u64()?, row.get("seat")?.as_str()?)))
        .collect();

    let mut checked = 0usize;
    for widget in spec::CATALOGUE
        .iter()
        .filter(|w| !w.reserved_for.is_empty())
    {
        let n = cited(widget.reserved_for).unwrap_or_else(|| {
            panic!(
                "{:?} cites {:?}, which is not a requirement number",
                widget.kind, widget.reserved_for
            )
        });
        let seat = booked.get(&n).unwrap_or_else(|| {
            panic!(
                "★ the palette's {:?} cites requirement {n}, which the register books to no \
                 seat — either the citation is wrong or the register is missing a row",
                widget.kind
            )
        });
        assert_eq!(
            *seat, widget.kind,
            "★ the palette's {:?} cites requirement {n}, which the register books to {seat:?} \
             — a locked seat naming another seat's requirement tells a reader the wrong thing \
             is coming",
            widget.kind,
        );
        checked += 1;
    }

    for row in pin["rail"]
        .as_array()
        .expect("the register declares a rail array")
    {
        let n = row["requirement"]
            .as_u64()
            .expect("a rail row names a requirement");
        let seat = row["seat"].as_str().expect("a rail row names a seat");
        let on_rail = spec::RAIL
            .iter()
            .find(|s| s.key == seat)
            .unwrap_or_else(|| panic!("the register books {seat:?}, which is not a rail seat"));
        // ★★★★★ R1947 — **a row may say the section behind it was BUILT, and
        // then the seat must be open rather than locked.**
        //
        // Before this round every rail row was a locked seat and the lookup
        // below could assume one. R1947 built `topology`, so the assumption
        // stopped holding — and the honest repair is not to skip the row but to
        // make the register carry the fact and check BOTH directions: a row
        // with `built` whose seat is still locked is a section we said we
        // finished and did not, and a row without it whose seat is open is one
        // that arrived with nobody told.
        match row["built"].as_str() {
            Some(round) => {
                assert!(
                    !round.is_empty(),
                    "the {seat:?} row is marked built by nothing",
                );
                assert!(
                    on_rail.reserved_for().is_none(),
                    "the register says {seat:?} was built at {round}, and the rail still \
                     locks it",
                );
                checked += 1;
                continue;
            }
            None => assert!(
                on_rail.reserved_for().is_some(),
                "the rail opens {seat:?} and the register still books it as deferred — \
                 add `built` to its row, naming the round",
            ),
        }
        let cite = on_rail
            .reserved_for()
            .expect("the arm above established this seat is locked");
        assert_eq!(
            cited(cite),
            Some(n),
            "★ the rail's {seat:?} cites {cite:?} where the register books requirement {n}",
        );
        // ★★★★★ NOT an equality with the palette's seat, and this gate is what
        // taught that: requirement 18 is booked to the palette's `health` AND
        // to the rail's `sessions`, because the reference exposes that one
        // capability on BOTH surfaces — a rail destination and a widget seat.
        // The first draft asserted the two agreed and failed here, correctly.
        // What is true across surfaces is that the number is one the register
        // defers at all; which seat carries it is per-surface.
        assert!(
            deferred
                .iter()
                .any(|row| row["requirement"].as_u64() == Some(n)),
            "the rail's {seat:?} cites requirement {n}, which the register does not defer",
        );
        checked += 1;
    }

    // ★ The denominator, because a loop over an empty set passes for the wrong
    // reason.
    //
    // ★★★★★ R1843 RE-JUDGED this floor rather than bumping it. It read
    // `checked >= 10`, written when the build had eight palette seats and two
    // rail ones — a floor set at exactly the population of the day, with a
    // comment saying `>=` was "on purpose" so a NEW locked seat would be
    // covered without editing the gate. That reasoning is sound in one
    // direction only: seats also move the other way, and promoting one is a
    // legitimate move this screen has now made twice. The floor turned every
    // promotion into a failing gate whose message blamed the population.
    //
    // What the floor was for is that the two loops above must not run over
    // nothing. So the assertion is now that the count MATCHES the locked
    // population this build declares — non-vacuous at any size, and no longer
    // satisfiable by a loop that silently skipped a seat, which `>=` was.
    let locked = spec::CATALOGUE
        .iter()
        .filter(|w| w.tier == spec::Tier::Reserved)
        .count()
        + pin["rail"].as_array().map_or(0, Vec::len);
    assert!(
        locked > 0,
        "this build locks no seat at all — the register would then describe a \
         screen that does not exist",
    );
    assert_eq!(
        checked, locked,
        "the gate checked {checked} locked seat(s) where this build declares \
         {locked} — a seat the loops above walked past is exactly what this \
         denominator exists to catch",
    );
}

/// ★★★★★ **(R2042) Every key row is booked by the register, or by nothing —
/// and the register says which.**
///
/// These two rows are the settings page's only locked seats and they were the
/// one surface `r1832` above could not cover: both cited a requirement, and
/// neither citation was theirs. 22 books an export of a report and 23 books
/// capture and replay, so the screen told a reader — in the only sentence it
/// has about what is coming — that two other capabilities would arrive here.
///
/// Repaired by MEASUREMENT rather than by choosing a number that looked free,
/// which is what the debt forbade in as many words. Read across the whole
/// register: decoding a payload in a format the application supplied is booked,
/// and that is the end-to-end key row; a key log that decrypts the links
/// themselves is booked by nothing at all, so that row now says so.
///
/// Both arms are checked here, and the second is the one that matters: a row
/// with no number must be one the register NAMES as unbooked, or "we do not
/// know" becomes a place to put anything.
#[test]
fn r2042_every_key_row_is_booked_by_the_register_or_by_nothing() {
    let pin = reserved_pin();
    let deferred = pin["deferred"]
        .as_array()
        .expect("the register declares a deferred array");
    let unbooked: std::collections::BTreeSet<&str> = pin["unbooked"]
        .as_array()
        .expect("the register declares an unbooked array")
        .iter()
        .filter_map(|row| row["seat"].as_str())
        .collect();

    // ⚠ No "the table is non-empty" assertion here, and that absence is the
    // rule rather than an oversight: `KEY_ROWS` is a const, so clippy proves
    // such a check can never fail and this repository deletes an assertion with
    // no failing path. What keeps the loop below from passing over nothing is
    // the two-armed count at the end, which no constant can satisfy.
    let mut sourced = 0usize;
    let mut declared_unbooked = 0usize;
    for row in spec::KEY_ROWS {
        if let Some(cite) = row.reserved_for {
            {
                let n = cited(cite).unwrap_or_else(|| {
                    panic!(
                        "the {:?} key row cites {cite:?}, which is not a requirement",
                        row.key
                    )
                });
                let booked = deferred
                    .iter()
                    .find(|d| d["requirement"].as_u64() == Some(n))
                    .unwrap_or_else(|| {
                        panic!(
                            "★ the {:?} key row cites requirement {n}, which the register \
                             does not defer at all",
                            row.key
                        )
                    });
                // ★ A key row is foreshadowed INSIDE this screen, so the
                // register must place it there. A number booked to a palette
                // seat or a separate console would be the same class of wrong
                // answer the citation itself was.
                assert_eq!(
                    booked["where"].as_str(),
                    Some("in-place"),
                    "★ the {:?} key row cites requirement {n}, which the register books \
                     somewhere other than inside a screen this build already has",
                    row.key,
                );
                assert!(
                    !unbooked.contains(row.key),
                    "the {:?} key row cites a requirement AND is listed as unbooked",
                    row.key,
                );
                sourced += 1;
            }
        } else {
            assert!(
                unbooked.contains(row.key),
                "★ the {:?} key row cites nothing, and the register does not name it \
                 as unbooked — a seat with no number must say why, or `None` becomes \
                 a place to put anything",
                row.key,
            );
            declared_unbooked += 1;
        }
    }
    assert_eq!(
        sourced + declared_unbooked,
        spec::KEY_ROWS.len(),
        "every key row is one or the other",
    );
    // ★ Both arms have a member, so neither branch above is dead. If this build
    // ever books the second row, this is the assertion that asks whether the
    // register was told.
    assert!(
        sourced > 0 && declared_unbooked > 0,
        "both arms are exercised"
    );
    assert!(
        pin["owed"].as_array().is_some_and(Vec::is_empty),
        "the register still owes a citation it cannot source, and this gate \
         claims that population is empty",
    );
}

/// ★★★ **Every palette requirement the register books is either reserved here
/// or declared BUILT** — so a seat we finished and a seat we forgot cannot look
/// the same.
///
/// The reference draws nine locked widget seats; this build has BUILT some of
/// them (a latency card, R1797; a health card, R1843) and reserves the rest.
/// Without the `built` list that is indistinguishable from a seat nobody
/// noticed was missing, and the honest difference between *shipped early* and
/// *overlooked* is what a register is for.
///
/// ⚠ The split is deliberately NOT written here as two numbers. R1843 moved a
/// seat across it and found this comment saying "reserves eight and has BUILT
/// the ninth" — correct when written, false the moment a promotion lands, and
/// sitting in a doc comment no gate reads. The assertions below hold the
/// invariant instead.
#[test]
fn r1832_a_palette_requirement_is_reserved_here_or_declared_built() {
    let pin = reserved_pin();
    let built: std::collections::BTreeSet<u64> = pin["built"]
        .as_array()
        .expect("the register declares a built array")
        .iter()
        .filter_map(|row| row["requirement"].as_u64())
        .collect();
    let reserved: std::collections::BTreeSet<u64> = spec::CATALOGUE
        .iter()
        .filter(|w| !w.reserved_for.is_empty())
        .filter_map(|w| cited(w.reserved_for))
        .collect();

    let mut palette = 0usize;
    for row in pin["deferred"].as_array().expect("a deferred array") {
        if row["where"].as_str() != Some("palette") {
            continue;
        }
        palette += 1;
        let n = row["requirement"].as_u64().expect("a requirement number");
        assert!(
            reserved.contains(&n) || built.contains(&n),
            "★ requirement {n} is a locked palette seat of the reference and this \
             build neither reserves it nor declares it built — which is what a \
             forgotten seat looks like",
        );
        assert!(
            !(reserved.contains(&n) && built.contains(&n)),
            "requirement {n} is both reserved and declared built, so the register \
             and the tree disagree about whether it exists",
        );
    }
    assert_eq!(
        palette, 9,
        "the reference's palette defers nine requirements"
    );
    assert_eq!(
        reserved.len() + built.len(),
        palette,
        "the reserved seats and the built ones do not account for the palette",
    );
}

/// ★★★★★ R1843 — **every placed card is at least as wide in place as it is
/// torn off**, and the numbers that say so are EMITTED rather than reasoned.
///
/// This exists because of how R1843 nearly went wrong. Fitting a sixth card on
/// the board, the round concluded twice — once by inference and once by what it
/// called arithmetic — that six cards *could not* fit, and reverted working
/// code on the strength of it. Both conclusions were false, and both for the
/// same reason: the round measured card ROWS exhaustively (four cards proved to
/// need their second row, each by a named failing gate) and never once measured
/// card WIDTHS. The rule it had not read is the one below, and reading it
/// turned "cannot fit" into a layout that fits exactly.
///
/// So the geometry is a gate now, and it PRINTS. Run it and the numbers that
/// decided this board come out where a person can check them:
///
/// ```text
/// cargo test -p hello-analyzer-shell r1843_the_board -- --nocapture
/// ```
///
/// ⚠ The floor is not a number chosen here. [`FLOAT_MIN_W`](super::FLOAT_MIN_W)
/// is what a card clamps to once it is torn off, and a card legible detached
/// and illegible in place would be this shell disagreeing with itself about one
/// thing. That is also why `board_canvas_floor` — and through it the shipping
/// window gate `r1781` — is driven by the NARROWEST span on the board: shrink
/// any card below this and the whole dashboard demands a wider window.
#[test]
fn r1843_the_board_is_wide_enough_for_every_card_it_places() {
    let canvas_w = spec::WIN_W - spec::RAIL_W - spec::PALETTE_W;
    let pitch = (canvas_w - super::GAP) / spec::GRID_COLS;
    println!(
        "board geometry at the opening size: window {} - rail {} - palette {} = canvas {canvas_w}; \
         pitch = (canvas - gap {}) / {} cols = {pitch}; a card's float floor is {}",
        spec::WIN_W,
        spec::RAIL_W,
        spec::PALETTE_W,
        super::GAP,
        spec::GRID_COLS,
        super::FLOAT_MIN_W,
    );

    let mut narrowest = spec::GRID_COLS;
    for tile in spec::BOARD {
        let width = tile.cols * pitch - super::GAP;
        println!(
            "  {:9} {} col(s) x {} row(s) at ({}, {}) -> {width}px",
            tile.kind, tile.cols, tile.rows, tile.col, tile.row,
        );
        assert!(
            width >= super::FLOAT_MIN_W,
            "★ {} spans {} column(s) = {width}px, under the {}px a card clamps to when torn \
             off — so this card is legible detached and illegible in place, and the board's \
             own floor rises with it",
            tile.kind,
            tile.cols,
            super::FLOAT_MIN_W,
        );
        narrowest = narrowest.min(tile.cols);
    }

    // The consequence, printed beside its cause: the dashboard's declared floor
    // is derived from the narrowest span, and the shipping window is what has
    // to satisfy it. `r1781` is the gate that fails when it does not.
    let floor =
        (super::FLOAT_MIN_W + super::GAP).div_ceil(narrowest) * spec::GRID_COLS + super::GAP;
    println!(
        "narrowest span {narrowest} col(s) -> the dashboard declares a floor of {floor}px, \
         and the shipping window leaves it {canvas_w}px"
    );
    assert!(
        floor <= canvas_w,
        "★ the board's narrowest card is {narrowest} column(s), which makes the dashboard \
         declare a {floor}px floor where the shipping window leaves {canvas_w}px — widening \
         the narrowest card is the repair, not widening the window",
    );
}

// ── The alarm feed (R1851) ──────────────────────────────────────────────────

/// ★★★★★ R1851 — **the board's seventh card is what COUNTING left, and the count
/// is printed.**
///
/// R1843's lesson, applied to the axis it did not cover. That round nearly
/// reverted working code twice by concluding a card "could not fit" — once by
/// inference and once by what it called arithmetic — because it measured rows
/// exhaustively and never measured widths. This is the row half of the same
/// discipline: which cards can give one up is a question every earlier round has
/// already answered, and the answer is arithmetic rather than judgement.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1851_the_board -- --nocapture
/// ```
#[test]
fn r1851_the_board_is_exactly_full_and_the_alarm_card_is_what_is_left() {
    let cells: u32 = spec::BOARD.iter().map(|p| p.cols * p.rows).sum();
    let grid = spec::GRID_COLS
        * spec::BOARD
            .iter()
            .map(|p| p.row + p.rows)
            .max()
            .unwrap_or(0);
    for placed in spec::BOARD {
        println!(
            "  {:9} {} col(s) x {} row(s) at ({}, {})",
            placed.kind, placed.cols, placed.rows, placed.col, placed.row,
        );
    }
    println!("  {cells} cell(s) placed in a {grid}-cell grid");
    assert_eq!(cells, grid, "the board is exactly full — no cell is idle");
    assert_eq!(grid, 48, "twelve columns by four rows");

    // No two placements overlap, which a cell count alone cannot say: seven
    // cards summing to 48 could still be stacked.
    let mut taken = std::collections::BTreeSet::new();
    for placed in spec::BOARD {
        for c in placed.col..placed.col + placed.cols {
            for r in placed.row..placed.row + placed.rows {
                assert!(
                    taken.insert((c, r)),
                    "{} overlaps at ({c}, {r})",
                    placed.kind
                );
            }
        }
    }
    assert_eq!(super::u(taken.len()), grid, "and it covers every cell");

    // The alarm card is the only one-row card besides `health`, and `health`'s
    // single row is the reference's own footprint for that seat. So the alarm
    // card's four cells are exactly what the other six left.
    let alarms = spec::BOARD
        .iter()
        .find(|p| p.kind == "alarms")
        .expect("the board places the alarm card");
    let others: u32 = spec::BOARD
        .iter()
        .filter(|p| p.kind != "alarms")
        .map(|p| p.cols * p.rows)
        .sum();
    println!("  the other six take {others}, leaving {}", grid - others);
    assert_eq!(alarms.cols * alarms.rows, grid - others);
    assert_eq!(
        (alarms.cols, alarms.rows),
        (4, 1),
        "four cells is four columns by one row — the board's narrowest legal card"
    );
    assert_eq!(
        spec::BOARD.last().map(|p| p.kind),
        Some("alarms"),
        "★ a new placement goes LAST: a card's id is `kind#index`, so inserting one \
         in the middle renames every card after it (R1843 measured that as six \
         gates reporting cards that had vanished)",
    );
}

/// ★★★★★ R1851 — **the feed constructs the window it shows, and both numbers are
/// printed.**
///
/// [`spec::ALARM_ROWS_SHOWN`] is a PIN, for `HEALTH_TILES_SHOWN`'s reason: the
/// rule that produces it needs the card's pixel height, which lives in the
/// painter and is not reachable from a `const` table. This is what makes the pin
/// honest — and it asserts the thing the whole row is about, which is that a feed
/// of eighteen alarms builds four rows.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1851_the_feed -- --nocapture
/// ```
#[test]
fn r1851_the_feed_builds_only_the_window_it_shows() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let rect = super::alarm_body_rect(&state);
        let columns = super::alarm_columns(rect.w).expect("the opening body holds the columns");
        let feed = super::alarm_feed("probe", rect, &columns, spec::ALARMS.len());
        let viewport = feed.body_viewport();
        let window = feed.window(0);
        println!(
            "alarm body {}x{}; header {} leaves {}; pitch {} -> {} whole row(s); \
             window {}..{} of {} alarm(s)",
            rect.w,
            rect.h,
            spec::ALARM_HEAD_H,
            viewport.h,
            spec::ALARM_ROW_H,
            feed.rows_in_view(),
            window.first,
            window.first + window.count,
            spec::ALARMS.len(),
        );
        assert_eq!(
            window.count,
            spec::ALARM_ROWS_SHOWN,
            "the pin and the window disagree"
        );
        // ★★★★★ THE CLAIM. A virtualised feed pays for what it shows, and this is
        // the number the reference toolkit has no way to report at all: probed at
        // 6.11.1, a tabular view over ten thousand rows answers ten thousand
        // through its public surface, and asking which rows it built does not
        // compile.
        assert!(
            window.count < spec::ALARMS.len(),
            "a feed that builds every row it holds is not virtualised: {} of {}",
            window.count,
            spec::ALARMS.len(),
        );
        // And the body is a whole number of rows, so no row is drawn half.
        assert_eq!(viewport.h % spec::ALARM_ROW_H, 0);
        assert_eq!(viewport.h / spec::ALARM_ROW_H, super::u(window.count));
        // The columns fill the body exactly — the one declared `0` takes the rest.
        let spanned: u32 = columns.iter().map(|c| c.size).sum();
        assert_eq!(spanned, rect.w, "the headings span the body exactly");

        // ★★★★★ AND A READER IS TOLD ABOUT EXACTLY THOSE ROWS. This is the claim
        // the paint-side ghost gate cannot make about this card — it reads the
        // frame after the canvas's clip, and a scrolled-away row is a row the
        // frame does not record — so it is made here, against the window the
        // assembly actually built. R1843 shipped the opposite on the health
        // strip (three tiles painted, five announced) and R1846 had to repair it.
        let card = state
            .card(&spec::card_of("alarms").expect("the board places the alarm card"))
            .expect("and the shell holds it");
        // ⚠ The bare ROWS, not their cells. `contains(".feed.row.")` matched
        // both and reported sixteen — a row announces three cells, so the
        // predicate has to say which of the two families it means.
        // ★ R2022 — through the one derivation the painter is handed, so this
        // claim is about the rectangle the card is actually drawn in.
        let announced: Vec<String> = super::alarms_nodes(
            &state,
            &card,
            super::card_body_rect(&state, &card).expect("the alarm card is on the board"),
        )
        .into_iter()
        .map(|node| node.tag)
        .filter(|tag| {
            tag.rsplit_once(".feed.row.")
                .is_some_and(|(_, rest)| !rest.contains('.'))
        })
        .collect();
        assert_eq!(
            announced.len(),
            window.count,
            "the feed announces {} row(s) and built {}: {announced:?}",
            announced.len(),
            window.count,
        );
        for slot in 0..window.count {
            assert!(
                announced
                    .iter()
                    .any(|tag| tag.ends_with(&format!(".feed.row.{slot}"))),
                "slot {slot} was built and is not announced: {announced:?}"
            );
        }
        // A body too narrow for the feed announces NOTHING, which is the same
        // refusal the painter makes — asserted rather than assumed, because the
        // two are separate functions and this is the pair that must agree.
        assert!(
            super::alarm_columns(spec::ALARM_EVENT_FLOOR).is_none(),
            "a body at the reading column's own floor cannot also hold the other two"
        );
    });
}

/// ★★★★★ R1851 — **every vocabulary this feed DECLARES on the wire is its own
/// definition**, and not a second list that agrees today.
///
/// R1642's rule: a declaration admitting a call the surface refuses is worse than
/// silence, because a client acts on it. The three closed sets `sort_alarms` and
/// `filter_alarms` publish are `const`s, so they are exactly the kind of
/// hand-written census R1630's ratchet exists to refuse — this is that ratchet,
/// for this surface.
#[test]
fn r1851_the_declared_vocabularies_are_their_definitions() {
    // The columns a client may sort by ARE the feed's columns.
    assert_eq!(
        super::ALARM_COLUMN_KEYS,
        spec::ALARM_COLUMNS
            .iter()
            .map(|(label, _)| label.to_lowercase())
            .collect::<Vec<_>>(),
        "★ a column added to the feed and not to the verb's domain is a column a \
         client cannot reach; the reverse is a domain admitting a call that is refused"
    );
    // The directions are the framework's own, read through its parser rather
    // than compared with a literal spelled twice.
    for word in super::ALARM_DIRECTIONS {
        let parsed = pinion_core::widgets::view_order::sort_dir_from_str(word);
        assert_eq!(
            pinion_core::widgets::view_order::sort_dir_str(parsed),
            *word,
            "★ {word:?} is declared and does not survive the framework's own \
             round trip, so the domain and the parser disagree"
        );
    }
    assert!(
        super::ALARM_DIRECTIONS.contains(&"none"),
        "unsorted has to be reachable, or a client can start a cycle and not end it"
    );
    // The floors are the severity vocabulary, plus the word for *no floor*.
    assert_eq!(
        &super::ALARM_FLOORS[1..],
        spec::SEVERITY.levels(),
        "★ the declared floors and the scale's own words must be one list"
    );
    assert_eq!(
        super::ALARM_FLOORS.first(),
        Some(&"all"),
        "and *all* is a different statement from the least severe level"
    );
    // Every alarm carries a word the scale holds — which is what lets the feed
    // grade itself at all, and is exactly the property the behaviour prototype
    // does NOT have (its control offers `error` over rows spelled `err`).
    for alarm in spec::ALARMS {
        assert!(
            spec::SEVERITY.rank(alarm.severity).is_some(),
            "{:?} is graded {:?}, which the vocabulary does not hold",
            alarm.message,
            alarm.severity,
        );
    }
    // The first rows ARE the prototype's, which is the claim `ALARMS_IN_REFERENCE`
    // makes; the rest are this build's and the constant says which is which.
    assert!(spec::ALARMS_IN_REFERENCE < spec::ALARMS.len());
    assert!(
        spec::ALARMS.len() > spec::ALARM_ROWS_SHOWN * 2,
        "a feed only twice its own viewport barely exercises a window"
    );
}

/// ★★★★★ R1852 — **the capture section this application mounts builds a topology
/// out of its own hops, and the walk reaches it.**
///
/// Rule (7)'s form: the capability is not a separate binary's — it is a section
/// of THIS application, reached over the same walk every other section is, and
/// the derivation it publishes is asserted from here rather than from the
/// standalone screen's own suite. Screen B is mounted through
/// `pinion_screen::Mount<PacketView>`, so *the shell can reach it* and *the
/// capture derives a topology* are two claims and this holds both.
///
/// ⚠ Asserted from what the section PAINTS and nothing else. The shell mounts
/// that screen; it does not read its specification module, and it must not — a
/// host reaching into a mounted screen's internals is a host that can pass while
/// the screen it hosts is broken. So the numbers come out of the painted run,
/// which is the same surface a reader has.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1852_the_walk -- --nocapture
/// ```
#[test]
fn r1852_the_walk_reaches_a_topology_built_from_the_captures_own_hops() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        // The walk first, because what this asserts is about a section of an
        // application rather than about a screen in isolation.
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        assert!(
            report.itinerary().iter().any(|key| key == "packets"),
            "the walk must stand in the capture section: {:?}",
            report.itinerary()
        );

        // Stand in the capture section and read what it says about its own
        // premise. `state.go` is the same navigation the walk used.
        state.go("packets").expect("the capture section is open");
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);

        let mut said: Vec<String> = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let pinion_core::Scene::Text(text) = visit.node
                && text.content.starts_with("negotiated · session")
            {
                said.push(text.content.clone());
            }
        });
        println!("the capture section says: {said:?}");
        assert_eq!(
            said.len(),
            1,
            "★ exactly one premise run — two would be two accounts of one fact"
        );
        let sentence = &said[0];

        // ★★★★★ THE FINDING, read off the integrated application's own frame:
        // the always-visible premise now states HOW MUCH OF THE TABLE it is
        // about, and it is not all of it. Parsed rather than compared with a
        // literal, so a capture that gains a row moves this with it.
        let (covered, total) = sentence
            .rsplit_once(" of ")
            .and_then(|(head, tail)| {
                let covered = head.rsplit_once(' ')?.1.parse::<usize>().ok()?;
                let total = tail.split_whitespace().next()?.parse::<usize>().ok()?;
                Some((covered, total))
            })
            .unwrap_or_else(|| panic!("the premise states its reach: {sentence}"));
        println!("the premise covers {covered} of {total} rows");
        assert!(
            covered > 0,
            "a premise covering nothing would not be a premise"
        );
        assert!(
            covered < total,
            "★ a premise covering the whole table would make the strip's silence \
             harmless — the finding is that it covers {covered} of {total}"
        );
    });
}

/// ★★★★★ R2011 — **the capture section prints the address it lights.**
///
/// Rule (7)'s form: the claim is about a section of THIS application, reached
/// over the same walk every other section is, and settled from the painted
/// frame rather than from the mounted screen's own suite.
///
/// R1814 measured that screen B's link row printed a pair of four-octet
/// addresses — eight octets — against a six-byte extent, said one of the two
/// was wrong and declined to pick. R2011 opened the reference screen, found it
/// draws all eight under a link-layer heading, and moved the extent. This is
/// what that means for a reader: the row's text and the bytes the pane
/// highlights are the same eight octets, so the highlight is an answer to the
/// row rather than a decoration beside it.
///
/// ⚠ The notation is spelled HERE and not imported. The host must not reach
/// into a mounted screen's specification module — a host that reads the screen's
/// own constants can pass while the screen is broken — so the octets are read
/// out of the painted cells, rendered by this test's own statement of the
/// notation, and looked for among the words the frame drew.
///
/// ```text
/// cargo test -p hello-analyzer-shell r2011 -- --nocapture
/// ```
#[test]
fn r2011_the_capture_section_prints_the_address_it_lights() {
    use pinion_core::external::IntrospectValue;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        assert!(
            report.itinerary().iter().any(|key| key == "packets"),
            "the walk must stand in the capture section: {:?}",
            report.itinerary()
        );

        state.go("packets").expect("the capture section is open");

        // Open the link row through the verb the section publishes, which is
        // the same path a person's press takes.
        let mut accepted = 0usize;
        let mut externals = state.screens.externals(&state.journey.get());
        for external in &mut externals {
            let Some(surface) = external.handle.introspect_mut() else {
                continue;
            };
            if surface
                .invoke("select_field", IntrospectValue::Text("l0.link".to_owned()))
                .is_ok()
            {
                accepted += 1;
            }
        }
        drop(externals);
        assert_eq!(
            accepted, 1,
            "exactly one surface of the arrived section answers `select_field`"
        );

        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);

        // What the frame lit, what it drew in those cells, and every word on it.
        let mut lit: Vec<usize> = Vec::new();
        let mut cells: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut words: Vec<String> = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let Some(tag) = visit.node.tag()
                && let Some(rest) = tag.strip_prefix("pv.bytes.lit.")
                && let Ok(byte) = rest.parse::<usize>()
            {
                lit.push(byte);
            }
            if let pinion_core::Scene::Text(text) = visit.node {
                words.push(text.content.clone());
                if let Some(rest) = text
                    .tag
                    .as_deref()
                    .and_then(|tag| tag.strip_prefix("pv.bytes.cell."))
                    && let Ok(byte) = rest.parse::<usize>()
                {
                    cells.insert(byte, text.content.clone());
                }
            }
        });
        lit.sort_unstable();
        println!("R2011 the capture section lights {lit:?}");

        assert!(
            !lit.is_empty(),
            "the section lit no byte for the row it was told to open"
        );
        assert_eq!(
            lit.len() % 2,
            0,
            "an address pair is an even number of octets and this one lit {}",
            lit.len()
        );

        let octets: Vec<&str> = lit
            .iter()
            .map(|byte| {
                cells.get(byte).map_or_else(
                    || panic!("byte {byte} is lit and the pane drew no cell for it"),
                    String::as_str,
                )
            })
            .collect();
        let half = octets.len() / 2;
        let printed = format!(
            "{} -> {}",
            octets[..half].join(":"),
            octets[half..].join(":")
        );
        println!("R2011 those octets render as {printed:?}");

        assert!(
            words.iter().any(|word| word == &printed),
            "the capture section lights {lit:?}, which render as {printed:?}, and \
             draws no such text — so the row and its highlight are two accounts \
             of one address"
        );
    });
}

/// ★★★★★ R1851 — **the order the feed is in, the arrow it shows and the threshold
/// it applies are one state.**
///
/// Three separate claims that all reduce to *the feed does not hold the same fact
/// twice*. On the toolkit floor at 6.11.1 the indicator and the order are
/// properties of different objects, and its row filtering is a predicate over a
/// string whose vocabulary is whatever the rows happen to be spelled — the two
/// defects this test excludes by construction.
#[test]
fn r1851_the_order_the_arrow_and_the_threshold_cannot_disagree() {
    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let rect = super::alarm_body_rect(&state);
        let columns = super::alarm_columns(rect.w).expect("the opening body holds the columns");
        let glyphs = |sort| {
            super::alarm_feed("probe", rect, &columns, spec::ALARMS.len())
                .with_sort(sort)
                .sections()
                .into_iter()
                .map(|s| s.sort)
                .collect::<Vec<_>>()
        };

        // The feed opens sorted by time, newest first, and the arrow is on that
        // column and points that way.
        let opening = state.alarm_sort.get();
        assert_eq!(opening, Some(spec::ALARM_OPENING_SORT));
        let shown = glyphs(opening);
        let carrying: Vec<usize> = shown
            .iter()
            .enumerate()
            .filter(|(_, g)| g.is_some())
            .map(|(n, _)| n)
            .collect();
        assert_eq!(carrying, vec![spec::ALARM_OPENING_SORT.0]);
        assert_ne!(
            glyphs(Some((1, true)))[1],
            glyphs(Some((1, false)))[1],
            "the two directions carry different faces, or the arrow says nothing"
        );

        // The opening order really is newest first.
        let order = super::alarm_order(&state);
        let seconds: Vec<u32> = order.iter().map(|&n| spec::ALARMS[n].seconds()).collect();
        assert_eq!(order.len(), spec::ALARMS.len(), "no floor hides anything");
        let mut down = seconds.clone();
        down.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(seconds, down, "the feed opens newest first");
        // ★ And the prototype's own table is NOT in that order — its first two
        // rows are out of sequence. Which is what makes the sort observable at
        // all rather than a no-op that looks like a feature.
        let stated: Vec<u32> = spec::ALARMS.iter().map(spec::AlarmSpec::seconds).collect();
        assert_ne!(stated, seconds, "★ the table itself is not time-ordered");

        // Sorting by severity orders by RANK, not by the word: alphabetically
        // `error < info < warn`, which is not an order anybody means.
        state.alarm_sort.set(Some((0, true)));
        let by_severity = super::alarm_order(&state);
        let ranks: Vec<usize> = by_severity
            .iter()
            .map(|&n| spec::SEVERITY.rank(spec::ALARMS[n].severity).unwrap_or(0))
            .collect();
        let mut up = ranks.clone();
        up.sort_unstable();
        assert_eq!(ranks, up, "least severe first");
        let words: Vec<&str> = by_severity
            .iter()
            .map(|&n| spec::ALARMS[n].severity)
            .collect();
        let mut alphabetical = words.clone();
        alphabetical.sort_unstable();
        assert_ne!(
            words, alphabetical,
            "★ a rank order and an alphabetical order must differ here, or this \
             test would pass on a feed that sorted the WORDS"
        );

        // A threshold is a position in the order: *warnings* means warnings AND
        // errors, and errors are a subset of warnings.
        state.alarm_sort.set(Some(spec::ALARM_OPENING_SORT));
        state.alarm_floor.set(Some("warn".to_owned()));
        let warned = super::alarm_order(&state);
        state.alarm_floor.set(Some("error".to_owned()));
        let strict = super::alarm_order(&state);
        assert!(
            strict.len() < warned.len(),
            "errors are fewer than warnings"
        );
        assert!(
            warned.len() < spec::ALARMS.len(),
            "and warnings fewer than all"
        );
        let kept: std::collections::BTreeSet<usize> = warned.iter().copied().collect();
        assert!(
            strict.iter().all(|n| kept.contains(n)),
            "★ every error is also a warning — that is what an ORDER means, and a \
             set of three independent flags could not say it"
        );

        // ★★★★★ And a word the vocabulary does not hold is refused BY NAME, with
        // the vocabulary in the refusal. Measured on the toolkit floor at 6.11.1:
        // the same request there answers `0 of 6` and says nothing.
        let refused = spec::SEVERITY
            .at_least("error", "err")
            .expect_err("`err` is not a word of this scale");
        let said = refused.to_string();
        assert!(said.contains("\"err\""), "{said}");
        assert!(said.contains("info < warn < error"), "{said}");
    });
}

/// ★★★★★ R1853 — **the walk reaches a fault-injection panel derived from the
/// target's own declaration, and it names the faults it cannot offer.**
///
/// Rule (7)'s form, the same as R1852's: the capability belongs to a SECTION of
/// this application, reached over the walk every other section is reached by,
/// and asserted from here rather than from the standalone screen's suite. Screen
/// A is mounted through `pinion_screen::Mount<NodeLabView>`, so *the shell can
/// reach it* and *the panel derives its offers* are two claims and this holds
/// both.
///
/// ⚠ Asserted from what the section PAINTS and nothing else. This host mounts
/// that screen; it does not read its specification module, and it must not — a
/// host reaching into a mounted screen's internals can pass while the screen it
/// hosts is broken. So the offers come out of the painted runs, which is the
/// surface a reader has, and the only thing imported is the FRAMEWORK's own
/// vocabulary — `Scope`, which is where the boundary is defined.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1853_the_walk -- --nocapture
/// ```
#[test]
fn r1853_the_walk_reaches_a_fault_panel_derived_from_the_targets_own_settings() {
    use pinion_core::widgets::fault_injection::Scope;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        // The walk first: the claim is about a section of an application.
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        assert!(
            report.itinerary().iter().any(|key| key == "lab"),
            "the walk must stand in the node lab: {:?}",
            report.itinerary()
        );

        // Stand in the lab and read what its inspector says about the faults it
        // can and cannot inject. `state.go` is the same navigation the walk used.
        state.go("lab").expect("the node lab section is open");
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);

        let mut head: Vec<String> = Vec::new();
        let mut offers: Vec<String> = Vec::new();
        let mut boundary: Vec<String> = Vec::new();
        scene.for_each_node(&mut |visit| {
            let pinion_core::Scene::Text(text) = visit.node else {
                return;
            };
            let said = text.content.clone();
            if said.starts_with("fault injection") {
                head.push(said);
            } else if said.contains(" · ") && said.split(" · ").count() == 2 {
                // A fault row reads `<key> · <arm>`. Kept only when the arm is
                // one the FRAMEWORK publishes, so an unrelated run carrying the
                // same separator cannot be counted as an offer.
                let arm = said.split(" · ").nth(1).unwrap_or_default().to_owned();
                if pinion_core::widgets::fault_injection::DefectKind::from_wire(&arm).is_some() {
                    offers.push(said);
                }
            } else if said.contains("faults are not offered") {
                boundary.push(said);
            }
        });
        println!("head: {head:?}");
        println!("{} offer(s) painted", offers.len());
        println!("boundary: {boundary:?}");

        assert_eq!(
            head.len(),
            1,
            "★ exactly one heading — two would be two accounts of one panel"
        );
        // The heading's count is parsed rather than compared with a literal, so
        // a declaration that gains a row moves this with it.
        let counted: usize = head[0]
            .split_whitespace()
            .find_map(|word| word.parse().ok())
            .unwrap_or_else(|| panic!("the heading states a count: {:?}", head[0]));
        assert_eq!(
            counted,
            offers.len(),
            "★ the heading counts what the panel painted: {:?}",
            head[0],
        );
        assert!(
            offers.len() >= 4,
            "★★★★★ the panel must be OFFERING something inside the assembled \
             tool — a derivation nobody composed is what this round exists to \
             stop being true: {offers:?}",
        );

        // ★★★★★ And the boundary is stated, once per scope the panel cannot
        // offer, in the framework's own words. Derived from `Scope` here too, so
        // the host and the section agree by construction rather than by two
        // hand-kept lists.
        let owed: Vec<&Scope> = Scope::ALL
            .iter()
            .filter(|scope| !scope.injectable())
            .collect();
        assert_eq!(
            boundary.len(),
            owed.len(),
            "★ every scope the panel cannot offer must be named on the assembled \
             screen: {boundary:?}",
        );
        for scope in owed {
            assert!(
                boundary
                    .iter()
                    .any(|said| said.starts_with(scope.wire()) && said.contains(scope.because())),
                "★ {} is not refused in the framework's own words anywhere on \
                 this screen: {boundary:?}",
                scope.wire(),
            );
        }
    });
}

/// ★★★★★ R1857 — **in the assembled tool, every fault row is WHOLE**: the panel
/// occupies one contiguous block of addresses, every row carries the same parts
/// as its siblings, and the boundary runs are the framework's non-injectable
/// scopes and nothing else.
///
/// Rule (7)'s form. R1853's sibling above asks what the panel SAYS; this asks
/// what it is MADE OF, which is the half that was missing — the section painted
/// twenty-eight addresses and its own specification named none of them, and
/// nothing in this application could tell.
///
/// ⚠ **Nothing here is a second copy of the section's table.** The host learns
/// the panel's stem from the paint (an address is what a section publishes to
/// every client, unlike the state and specification modules R1852 established a
/// host must not reach into), the row count from the addresses themselves, the
/// PART names from row 0 — so a renamed part moves this with it — and the
/// scopes from the framework's own `Scope`. The literal below is the stem and
/// that is all.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1857_the_walk -- --nocapture
/// ```
#[test]
fn r1857_the_walk_reaches_a_fault_panel_whose_every_row_is_whole() {
    use pinion_core::widgets::fault_injection::Scope;
    use std::collections::{BTreeMap, BTreeSet};

    const STEM: &str = "lab.faults";

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        state.go("lab").expect("the node lab section is open");
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);

        let mut addresses: BTreeSet<String> = BTreeSet::new();
        scene.for_each_node(&mut |visit| {
            if let Some(tag) = visit.node.tag()
                && tag.starts_with(STEM)
            {
                addresses.insert(tag.to_owned());
            }
        });
        println!("{} address(es) under {STEM}", addresses.len());
        assert!(
            addresses.contains(STEM),
            "★ the assembled tool paints no fault panel at all: {addresses:?}",
        );

        // The rows, and the parts each one carries — both read off the
        // addresses rather than written down.
        let row_stem = format!("{STEM}.row.");
        let mut parts: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
        for tag in &addresses {
            let Some(rest) = tag.strip_prefix(&row_stem) else {
                continue;
            };
            let (index, part) = rest.split_once('.').unwrap_or((rest, ""));
            let index: usize = index
                .parse()
                .unwrap_or_else(|_| panic!("a row is addressed by its position: {tag}"));
            let seat = parts.entry(index).or_default();
            if !part.is_empty() {
                seat.insert(part.to_owned());
            }
        }
        assert!(
            parts.len() >= 4,
            "★ the assembled tool must be offering something for this to mean \
             anything: {parts:?}",
        );
        assert!(
            parts.keys().copied().eq(0..parts.len()),
            "★ the rows are addressed by POSITION, so a gap is a row that was \
             painted and then was not: {:?}",
            parts.keys().collect::<Vec<_>>(),
        );
        let first = parts[&0].clone();
        assert!(
            first.len() >= 3,
            "★ a row carries several parts, or an equality over them says \
             nothing: {first:?}",
        );
        for (index, seat) in &parts {
            assert_eq!(
                *seat, first,
                "★ row {index} does not carry the parts its siblings do — one \
                 row short of a part is exactly what a count of rows cannot see",
            );
        }

        // ★ And nothing else lives under the stem: panel, heading, the rows and
        // their parts, and one run per scope the framework cannot reach.
        let mut want: BTreeSet<String> = BTreeSet::new();
        want.insert(STEM.to_owned());
        want.insert(format!("{STEM}.head"));
        for index in parts.keys() {
            want.insert(format!("{row_stem}{index}"));
            for part in &first {
                want.insert(format!("{row_stem}{index}.{part}"));
            }
        }
        for scope in Scope::ALL.iter().filter(|scope| !scope.injectable()) {
            want.insert(format!("{STEM}.scope.{}", scope.wire()));
        }
        assert_eq!(
            addresses, want,
            "★ the assembled panel occupies addresses its own structure does \
             not account for",
        );
    });
}

/// ★★★★★ R1859 — **in the assembled tool, at the size a person runs, the run a
/// reader named holds its own text.**
///
/// Rule (7)'s form for a defect somebody SAW. The report was about the shipped
/// window — `target/release/hello-analyzer-shell`, 1440x900 — and about a run
/// inside a screen this application MOUNTS, so a check that only ever ran the
/// section standalone would be checking a different thing from the one that was
/// looked at. The section's own zero gate is next door in `hello-node-lab`;
/// this is the same property asked of the assembly.
///
/// ⚠ **Asserted from the paint, by the words, with no reach into the mounted
/// screen.** The host does not import the lab's geometry or its constants — it
/// finds the run by what it says, which is what the reader had, and asks the
/// FRAMEWORK's predicate about it. R1852 established that a host reading a
/// guest's internals can pass while the guest is broken.
///
/// ```text
/// cargo test -p hello-analyzer-shell r1859_the_walk -- --nocapture
/// ```
#[test]
fn r1859_the_walk_reaches_a_placeholder_that_holds_its_own_letters() {
    const SAID: &str = "type a name or a key";

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        state.go("lab").expect("the node lab section is open");
        let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);

        let mut found = 0usize;
        let mut short: Vec<(String, u32, u32, u32)> = Vec::new();
        scene.for_each_node(&mut |visit| {
            let pinion_core::Scene::Text(t) = visit.node else {
                return;
            };
            if t.content != SAID {
                return;
            }
            found += 1;
            let owed = pinion_core::containment::short_by(t);
            if owed > 0 {
                short.push((t.content.clone(), t.style.font_size_px, t.rect.h, owed));
            }
        });

        println!("the assembled tool paints {found} run(s) saying {SAID:?}");
        assert_eq!(
            found, 1,
            "★ the reader saw exactly one of these; none means the walk no \
             longer reaches the row and two means the row is painted twice",
        );
        assert!(
            short.is_empty(),
            "★ the run a reader reported is STILL in a box too short for its \
             own face (content, face, box height, short by): {short:?}",
        );
    });
}

/// ★★★★★ R1866 — **the walk reaches a node lab that compares two of its own
/// runs**, which is the assembly `lab.t2.17` was resting on and did not have.
///
/// # Why it is asserted HERE and through the SHELL's own external set
///
/// Rule (7): the round's deliverable is the assembly, in the integrated
/// application, driven over one walk — not a screen built beside it. The lab is
/// a mounted guest, so the honest question is whether the tool a reader runs
/// can be walked to that section and asked, and `ScreenRoster::externals` is
/// exactly the door a press or an agent goes through. A test that reached into
/// `hello_node_lab`'s own state would be asking the guest directly and would
/// pass on an application that never mounted it.
///
/// # What it drives
///
/// The scenario is played, kept, played again with one act moved, and the
/// comparison read — the four steps a person takes to answer *did this change
/// anything*. The amount is asserted, not merely the existence of a difference:
/// a comparison that reported *something moved* for any edit would be a
/// comparison nobody can act on.
/// A guest's answer as json, whichever shape the slot hands it over in.
///
/// ⚠ Two slots of one surface answer in two shapes — `spec` is text holding
/// json and `regression` is a json value — so a reader of that surface has to
/// know which is which. That is a fact about the screen rather than about this
/// test, and it is written here because this is where it was met.
fn as_json(v: pinion_core::external::IntrospectValue) -> serde_json::Value {
    use pinion_core::external::IntrospectValue;
    match v {
        IntrospectValue::Json(j) => j,
        IntrospectValue::Text(t) => serde_json::from_str(&t).expect("a text slot holding json"),
        other => panic!("expected json, got {other:?}"),
    }
}

#[test]
fn r1866_the_walk_reaches_a_lab_that_compares_two_of_its_own_runs() {
    use pinion_core::external::IntrospectValue;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        // The walk first: the claim is about a section of an application.
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        assert!(
            report.itinerary().iter().any(|key| key == "lab"),
            "the walk must stand in the node lab: {:?}",
            report.itinerary()
        );

        state.go("lab").expect("the node lab section is open");
        let mut externals = state.screens.externals(&state.journey.get());
        let tags: Vec<String> = externals.iter().map(|e| e.tag.to_string()).collect();
        println!("the lab section publishes: {tags:?}");
        let lab = externals
            .iter_mut()
            .filter_map(|e| e.handle.introspect_mut())
            .find(|it| it.query("regression").is_ok())
            .unwrap_or_else(|| {
                panic!(
                    "no external of the lab section answers `regression`, so the \
                     assembly this row rests on is not reachable from the \
                     assembled tool: {tags:?}"
                )
            });

        macro_rules! call {
            ($name:expr, $args:expr) => {
                lab.invoke($name, $args)
                    .unwrap_or_else(|why| panic!("{} refused: {why:?}", $name))
            };
        }

        // Nothing kept yet: the screen says so rather than reporting a clean
        // comparison, which is the difference a client has to be able to see.
        let opening = as_json(lab.query("regression").expect("a slot"));
        assert!(
            opening["baseline"].is_null(),
            "the lab opens with a baseline: {opening}",
        );

        // A run, kept, then the same plan with one act four seconds later.
        // ⚠ The card is READ, not written down: `schedule` refuses a target the
        // graph does not have, so naming one here would make this a claim about
        // the opening graph as well as about the comparison.
        //
        let spec = as_json(lab.query("spec").expect("a slot"));
        let card = spec["nodes"][0]["id"]
            .as_str()
            .expect("the opening graph has a card")
            .to_owned();
        call!("advance", IntrospectValue::Float(-1000.0));
        call!(
            "schedule",
            IntrospectValue::Json(serde_json::json!({
                "act": "stop", "target": card, "at": 5.0
            }))
        );
        call!("advance", IntrospectValue::Float(9.0));
        call!("record", IntrospectValue::Null);
        call!(
            "unschedule",
            IntrospectValue::Json(serde_json::json!({ "lane": "main", "at": 5.0 }))
        );
        call!(
            "schedule",
            IntrospectValue::Json(serde_json::json!({
                "act": "stop", "target": card, "at": 9.0
            }))
        );
        call!("advance", IntrospectValue::Float(-1000.0));
        call!("advance", IntrospectValue::Float(12.0));

        let said = as_json(lab.query("regression").expect("a slot"));
        println!("the lab's own comparison: {said}");
        assert_eq!(
            said["clean"], false,
            "the act moved four seconds and the comparison calls the run clean",
        );
        let shifted = said["shifted"].as_array().expect("an array");
        assert_eq!(shifted.len(), 1, "exactly one act moved: {said}");
        assert_eq!(
            shifted[0]["name"].as_str(),
            Some(format!("stop {card}").as_str()),
            "and it is the act that was moved",
        );
        let by = shifted[0]["by"].as_f64().expect("a number");
        assert!(
            (by - 4.0).abs() < 1e-6,
            "★ by exactly what it was moved — the amount is the point, because a \
             comparison that says `something moved` for any edit is one nobody \
             can act on: {by}",
        );
        // ★ And the LATENCY half of the row's sentence: the shifts summarised.
        assert!(
            said["distribution"]["samples"].as_u64() == Some(1),
            "the shift was not summarised into a distribution: {said}",
        );
    });
}

/// Where a named panel of the lab sits, out of that surface's own published
/// specification.
///
/// A free function rather than one nested in the test, because clippy is right
/// that an item after a statement reads as if it were scoped to what precedes
/// it — the same note `incompatible_lines` carries below.
fn placed_panel(spec: &serde_json::Value, name: &str) -> serde_json::Value {
    spec["panes"]
        .as_array()
        .expect("the surface publishes its panes")
        .iter()
        .find(|pane| pane["name"] == name)
        .unwrap_or_else(|| panic!("{name} is a pane this surface publishes"))["at"]
        .clone()
}

/// ★★★★★ R1887 — **the assembled tool lets a person place the node lab's side
/// panels, and refuses the placement they do not admit.**
///
/// # Why here and through the shell's own external set
///
/// Rule (7), and the argument the two walks before it make: the lab is a
/// MOUNTED guest, so the honest question is whether the tool a reader actually
/// runs can be walked to that section and asked. A test reaching into
/// `hello_node_lab`'s own state would pass on an application that never mounted
/// it — which is exactly the arrangement the reader who reported this was
/// looking at.
///
/// # What it drives, and in this order
///
/// Where the panel is, is read BEFORE and AFTER, from the wire, so "the edit
/// caused this" can be told from "it opened that way" — the rule R1885's walk
/// wrote and the reason its gate is read twice. Then the refusal, which is the
/// half no press can reach: the header's control cycles the admitted edges by
/// construction, so only a caller naming an edge outright can ask for one the
/// panel does not admit. ⇒ the two channels are not redundant.
#[test]
fn r1887_the_walk_reaches_a_lab_whose_panels_a_person_can_place() {
    use pinion_core::external::IntrospectValue;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        assert!(
            report.itinerary().iter().any(|key| key == "lab"),
            "the walk must stand in the node lab: {:?}",
            report.itinerary()
        );

        state.go("lab").expect("the node lab section is open");
        let mut externals = state.screens.externals(&state.journey.get());
        let tags: Vec<String> = externals.iter().map(|e| e.tag.to_string()).collect();
        let lab = externals
            .iter_mut()
            .filter_map(|e| e.handle.introspect_mut())
            .find(|it| it.query("gate").is_ok())
            .unwrap_or_else(|| {
                panic!("no external of the lab section answers for the lab: {tags:?}")
            });

        let opening = as_json(lab.query("spec").expect("a slot"));
        let before = placed_panel(&opening, "palette");
        assert!(
            !before.is_null(),
            "★ the surface must say WHERE a placeable panel is, not only where \
             it may go — a client told only what is permitted cannot tell \
             whether a placement did anything: {opening}"
        );
        assert_eq!(before["folded"], serde_json::Value::Bool(false));

        // ★ THE EDIT, through the verb a client is told about.
        let said = lab
            .invoke("place", IntrospectValue::Text("palette,right".to_owned()))
            .expect("`place` is a declared action of this screen");
        println!("the screen said: {said:?}");

        let after = placed_panel(&as_json(lab.query("spec").expect("a slot")), "palette");
        assert_ne!(
            after["edge"], before["edge"],
            "the panel is on the other edge now: {before} -> {after}"
        );

        // ★ AND THE REFUSAL, which no press can reach.
        let refused = lab
            .invoke("place", IntrospectValue::Text("palette,top".to_owned()))
            .expect_err("this panel admits the sides and not the top");
        let sentence = format!("{refused:?}");
        for half in ["top", "left", "right"] {
            assert!(
                sentence.contains(half),
                "★ a refusal names what was asked AND what is allowed, or the \
                 caller cannot act on it: {sentence} lacks {half:?}"
            );
        }
        let unmoved = placed_panel(&as_json(lab.query("spec").expect("a slot")), "palette");
        assert_eq!(unmoved, after, "a refused placement changes nothing at all");
    });
}

/// The launch gate's lines that are about two builds unable to negotiate.
///
/// A free function rather than a nested one, because clippy is right that an
/// item after a statement reads as if it were scoped to what precedes it.
fn incompatible_lines(gate: &serde_json::Value) -> Vec<String> {
    gate.as_array()
        .expect("the gate is a list")
        .iter()
        .filter_map(|line| line["sentence"].as_str())
        .filter(|s| s.contains("share no wire revision"))
        .map(ToOwned::to_owned)
        .collect()
}

/// The card the R1885 walk moves onto another build.
///
/// A peer rather than the store or the router: it is on more than one wire, so
/// the walk's derived expectation is a number greater than one and a rule that
/// fired on only the first wire would be caught.
const R1885_CARD: &str = "P-01";

/// ★★★★★ R1885 — **the assembled tool holds a compatibility test graph**: its
/// peers run different builds, every drawn wire negotiates, and putting one peer
/// on a build the other cannot talk to makes the launch gate say so — naming
/// both builds, both spans, and which card to change.
///
/// This is `lab.t2.19`'s assembly, and with it the `UNASSEMBLED` ratchet empties.
///
/// # Why it is asserted HERE and through the SHELL's own external set
///
/// Rule (7), and the same argument `r1866_the_walk_reaches_a_lab_that_compares_
/// two_of_its_own_runs` makes one screen up: the lab is a mounted guest, so the
/// honest question is whether the tool a reader runs can be walked to that
/// section and asked. A test reaching into `hello_node_lab`'s own state would be
/// asking the guest directly and would pass on an application that never
/// mounted it.
///
/// # What it drives, and why in this order
///
/// The gate is read BEFORE the edit as well as after. A test that only read it
/// afterwards could not tell "the edit caused this" from "the opening graph was
/// already broken" — and an opening graph that was already broken would be
/// asserting a defect rather than offering a test. So the first assertion is
/// that a heterogeneous graph is CLEAN, which is the claim that makes it a
/// compatibility test rather than a picture of one deployment.
#[test]
fn r1885_the_walk_reaches_a_lab_whose_peers_run_different_builds() {
    use pinion_core::external::IntrospectValue;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        // The walk first: the claim is about a section of an application.
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        assert!(
            report.itinerary().iter().any(|key| key == "lab"),
            "the walk must stand in the node lab: {:?}",
            report.itinerary()
        );

        state.go("lab").expect("the node lab section is open");
        let mut externals = state.screens.externals(&state.journey.get());
        let tags: Vec<String> = externals.iter().map(|e| e.tag.to_string()).collect();
        let lab = externals
            .iter_mut()
            .filter_map(|e| e.handle.introspect_mut())
            .find(|it| it.query("gate").is_ok())
            .unwrap_or_else(|| {
                panic!(
                    "no external of the lab section answers `gate`, so the launch \
                     gate this row rests on is not reachable from the assembled \
                     tool: {tags:?}"
                )
            });

        // ★ THE OPENING CLAIM: heterogeneous AND clean.
        let opening = as_json(lab.query("gate").expect("the gate slot"));
        assert_eq!(
            incompatible_lines(&opening),
            Vec::<String>::new(),
            "the opening graph must be a compatibility test that PASSES, or the \
             screen ships an assertion of a defect: {opening}"
        );
        let spec = as_json(lab.query("spec").expect("a slot"));
        let builds: Vec<&str> = spec["nodes"]
            .as_array()
            .expect("the opening graph has cards")
            .iter()
            .filter_map(|n| n["build"].as_str())
            .collect();
        let distinct: std::collections::BTreeSet<&&str> = builds.iter().collect();
        assert!(
            distinct.len() >= 2,
            "★ the graph is not heterogeneous, so it cannot be a compatibility \
             test at all — every card publishes the same build: {builds:?}"
        );

        // ★ THE EDIT: put one peer on a build the graph has moved past.
        let said = lab
            .invoke(
                "build",
                IntrospectValue::Text(format!("{R1885_CARD},legacy")),
            )
            .expect("`build` is a declared action of this screen");
        println!("the screen said: {said:?}");

        // ★ How many refusals to expect is DERIVED from the published graph —
        // every wire this card is on — and not written down. A number written
        // here would still pass if the rule fired on one wire of three, which is
        // the shape of a rule that silently stopped covering its population.
        let wires = spec["links"]
            .as_array()
            .expect("the opening graph publishes its links")
            .iter()
            .filter(|pair| {
                pair.as_array()
                    .is_some_and(|ends| ends.iter().any(|e| e.as_str() == Some(R1885_CARD)))
            })
            .count();
        assert!(
            wires > 0,
            "★ the card the edit targets is on no wire at all, so this walk \
             could not have found a refusal however the rule behaved"
        );

        // ★ AND THE GATE NOW SAYS SO.
        let after = as_json(lab.query("gate").expect("the gate slot"));
        let refused = incompatible_lines(&after);
        assert_eq!(
            refused.len(),
            wires,
            "every wire {R1885_CARD} is on ({wires}) should now fail to negotiate: {after}"
        );
        for sentence in &refused {
            for word in ["legacy", "v2-v4"] {
                assert!(
                    sentence.contains(word),
                    "★ the refusal must name the builds and their spans — a \
                     sentence that only says `incompatible` leaves the author \
                     guessing which card to change: {sentence:?} lacks {word:?}"
                );
            }
        }
        assert!(
            after
                .as_array()
                .expect("a list")
                .iter()
                .any(|line| line["blocks"] == serde_json::Value::Bool(true)
                    && line["sentence"]
                        .as_str()
                        .is_some_and(|s| s.contains("share no wire revision"))),
            "a graph asserting a session that cannot be established must not \
             launch: {after}"
        );
    });
}

// ── R1903: the palette is a placement, and everything follows it ───────────

/// ★★★★★ R1903 — **the palette opens where its own policy admits.**
///
/// R1902 built `EdgePolicy::admit_opening` on the finding that every CHANGE to
/// a placement met a policy and the opening state met nothing. This is that
/// judgement reaching the shell — its first consumer outside the node lab, and
/// the reason the palette's arrangement is a placement value here rather than
/// the `bool` a fold would otherwise be: a flag has nothing to be judged.
#[test]
fn r1903_the_palette_opens_where_its_own_policy_admits() {
    let admitted = spec::PALETTE_POLICY
        .admit_opening(spec::PALETTE_OPENS)
        .unwrap_or_else(|why| {
            panic!(
                "the palette opens at {:?}, which its own policy refuses: {}",
                spec::PALETTE_OPENS,
                why.reason()
            )
        });
    assert_eq!(
        admitted,
        spec::PALETTE_OPENS,
        "an admitted opening is returned unchanged"
    );
    // ⚠ The canon's own arrangement — that it opens SHOWING and that it CAN
    // close — is asserted in `r1903_folding_the_palette_gives_its_room_to_the
    // _canvas`, on the values the running screen answers with.
    //
    // ★ Not here, and clippy is what said so: an `assert!` on a `const` is
    // computed at compile time and optimised out, so it checks nothing at run
    // time. Two claims about this round's whole point were written that way and
    // would have passed for ever. A claim about a constant belongs where the
    // constant has become a value something read.
}

/// ★★★★★ R1903 — **every chrome derivation follows the fold.**
///
/// Measured this round: FIVE sites read the palette's open width as chrome —
/// the canvas rectangle, the sub bar's rectangle, its chip layout, the mounted
/// roster's left inset, and the panel's own rectangle. That is more than the
/// four this campaign's carry recorded, and the extra one is the point: a site
/// nobody counted is a site that goes on subtracting 292 pixels from a panel
/// that is 44 wide.
///
/// The claim is not "the canvas grew". It is that the room the panel takes and
/// the room everything else is given are **one number**, so they cannot
/// disagree — which is what a half-derivation cannot promise.
#[test]
fn r1903_folding_the_palette_gives_its_room_to_the_canvas() {
    Owner::new().run(|| {
        // R1908 — off disk, because folding is written now.
        let state = use_shell_state_off_disk();
        // ★ The canon's arrangement, read from the RUNNING screen rather than
        // asserted on a constant — see the note in the gate above for why that
        // distinction cost two checks that could never have failed.
        assert!(
            !state.palette_at.get().folded,
            "the canon opens its palette showing, so this screen does"
        );
        assert!(
            spec::PALETTE_POLICY
                .admit_fold(state.palette_at.get(), true)
                .is_ok(),
            "the canon puts a Collapse control on it, so this one folds"
        );
        let open_panel = super::palette_rect();
        let open_canvas = super::canvas_rect();
        assert_eq!(
            open_panel.w,
            spec::PALETTE_W,
            "it opens at the width the specification gives it"
        );

        super::ShellOracle::place_palette(&state, "fold").expect("the policy admits a fold");
        let shut_panel = super::palette_rect();
        let shut_canvas = super::canvas_rect();

        assert_eq!(
            shut_panel.w,
            spec::PALETTE_STRIP_W,
            "a folded palette is its strip, not nothing — the way back has to be \
             on screen"
        );
        // ★★★★★ THE WAY BACK IS REACHABLE, asserted ABSOLUTELY.
        //
        // A counterfactual found this hole and it is worth stating: setting the
        // strip width to zero — a fold that leaves nothing, which is a HIDE and
        // the thing this axis exists to distinguish — passed every check above,
        // because they are all RELATIVE. `shut_panel.w == PALETTE_STRIP_W` and
        // "the canvas grew by `PALETTE_W - PALETTE_STRIP_W`" both move with the
        // constant, so both stay true at zero. That is R1901.2's lesson in a
        // new shape: two sides derived from one source are blind to an edit
        // that moves them together.
        //
        // The property is not a width, it is REACHABILITY: a person who folded
        // the panel can press something and get it back. So the assertion is
        // the hit test's own answer at the strip's middle, which no constant
        // can satisfy vacuously.
        // ⚠ And the floor is ABSOLUTE, which took two attempts. The first was
        // the hit test alone, and it still passed at a strip zero pixels wide:
        // `palette_rect` then sits at `x == win_w()` with no extent, the probe
        // lands exactly on the window's right edge, and `Hit::at` answers
        // `PaletteStrip` for any x at or past that column. A reachability check
        // that a point OFF the screen satisfies is not one.
        assert!(
            shut_panel.w > 0,
            "a fold leaves a strip and a hide leaves nothing; that distinction \
             is this axis's whole point and it is a floor, not a ratio"
        );
        let probe = (
            shut_panel.x + shut_panel.w / 2,
            shut_panel.y + shut_panel.h / 2,
        );
        assert!(
            probe.0 < super::win_w(),
            "the point a person would aim at is ON the screen: {probe:?} in a \
             {}px window",
            super::win_w()
        );
        assert!(
            matches!(
                super::Hit::at(&state, probe.0, probe.1),
                super::Hit::PaletteStrip
            ),
            "a folded palette must leave something a pointer can press, or the \
             fold is a hide: nothing answers at {probe:?} inside {shut_panel:?}"
        );
        assert_eq!(
            shut_canvas.w - open_canvas.w,
            spec::PALETTE_W - spec::PALETTE_STRIP_W,
            "the canvas grows by EXACTLY what the panel gave up: one number, \
             read twice, rather than two derivations that agree today"
        );
        // The panel and the canvas still meet, with nothing between and nothing
        // overlapping — the property a second constant would break silently.
        assert_eq!(
            shut_canvas.x + shut_canvas.w,
            shut_panel.x,
            "the canvas ends where the strip begins"
        );
        assert_eq!(
            open_canvas.x + open_canvas.w,
            open_panel.x,
            "and did before the fold, so this is a property rather than a state"
        );

        super::ShellOracle::place_palette(&state, "unfold").expect("unfolding is never refused");
        assert_eq!(
            super::palette_rect(),
            open_panel,
            "and it comes back to exactly where it opened"
        );
        assert_eq!(super::canvas_rect(), open_canvas);
    });
}

/// ★★★★★ R1903 — **the strip is what a folded palette announces, and the
/// catalogue is not.**
///
/// A reader told about thirteen rows that are not painted is a reader sent
/// looking for them — the announce-what-is-not-drawn class this tree already
/// has a name for, and the one R1900's closing audit found in its own work.
#[test]
fn r1903_a_folded_palette_announces_its_way_back_and_not_its_rows() {
    Owner::new().run(|| {
        // R1908 — off disk, because folding is written now.
        let state = use_shell_state_off_disk();
        let open: Vec<String> = super::palette_nodes(&state)
            .into_iter()
            .map(|n| n.tag)
            .collect();
        assert!(
            open.iter().any(|t| t == "shell.palette"),
            "open, the catalogue is announced: {open:?}"
        );
        assert!(
            open.iter().any(|t| t == "shell.palette.head.fold"),
            "and so is the control that puts it away: {open:?}"
        );

        super::ShellOracle::place_palette(&state, "fold").expect("the policy admits a fold");
        let shut: Vec<String> = super::palette_nodes(&state)
            .into_iter()
            .map(|n| n.tag)
            .collect();
        assert_eq!(
            shut,
            vec!["shell.palette.strip".to_owned()],
            "folded, the strip is the whole announcement"
        );
        super::ShellOracle::place_palette(&state, "unfold").expect("unfolding is never refused");
    });
}

/// ★★★★★ R1903 — **the pointer and the wire reach one verb.**
///
/// The header control, the strip and a client all go through `place_palette`,
/// so the screen and an agent cannot come to mean different things by the same
/// act — the rule R1887 established for the sibling screen's panels. And a word
/// the verb does not know is refused BY NAME rather than ignored.
#[test]
fn r1903_both_palette_gestures_go_through_the_one_verb_and_it_refuses_by_name() {
    Owner::new().run(|| {
        // R1908 — off disk, because folding is written now.
        let state = use_shell_state_off_disk();
        for hit in [super::Hit::PaletteFold, super::Hit::PaletteStrip] {
            let before = state.palette_at.get().folded;
            super::ShellOracle::act_on_hit(&state, hit);
            assert_ne!(
                state.palette_at.get().folded,
                before,
                "each gesture toggles the one placement"
            );
        }
        assert!(
            !state.palette_at.get().folded,
            "two toggles come back to where they started"
        );

        let refused = super::ShellOracle::place_palette(&state, "vanish")
            .expect_err("the vocabulary is closed");
        let sentence = format!("{refused:?}");
        assert!(
            sentence.contains("vanish"),
            "a refusal names what was asked: {sentence}"
        );
        assert!(
            !state.palette_at.get().folded,
            "and a refusal changes nothing"
        );
    });
}

// ── R1905: a detached card's rectangle is in a SPACE, and a home change
//    crosses between two of them ──────────────────────────────────────────

/// ★★★★★ R1905 — **changing home converts the rectangle; it does not relabel
/// it.**
///
/// R1891 gave a torn-off card a `DetachHome` and deliberately left the geometry
/// alone, writing down that the transfer between the two coordinate spaces was
/// undecided. Measured on the running tool at the start of this round, the
/// consequence was exact:
///
/// ```text
/// tear off      -> floats [{x:120, y:40, home:"window"}], window at [120,40]
/// detach_home   -> floats [{x:120, y:40, home:"canvas"}]
/// ```
///
/// The identical pair, read against two different origins, so a reader watched
/// the panel jump by however far the window manager had placed this window from
/// the display's corner.
///
/// # Why this test stamps the origin itself
///
/// Because the conversion is BY the host's own origin, and a host at `(0, 0)`
/// makes converting and relabelling the same arithmetic — the shape R1901.2
/// named, where both sides of a check move together and the check goes blind.
/// Under a bare offscreen display there is no window manager to place this
/// window anywhere else, so the fact is stamped here through the very function
/// `pinion-shell` stamps it with. That is not a mock: it is the seam, driven.
#[test]
fn r1905_changing_home_crosses_the_two_coordinate_spaces() {
    Owner::new().run(|| {
        let state = use_shell_state();
        // The host is somewhere a window manager put it. Through the framework's
        // own sink, so a change to how the origin is published breaks this.
        pinion_core::external::publish_window_origins([(super::MAIN_WINDOW, (300, 150))]);
        let transfer = super::shell_transfer();
        assert!(
            transfer.knows_offset(),
            "the stamp is what the screen reads; without it every crossing is adrift"
        );

        super::ShellOracle::detach(&state, "packet#0").expect("a board card tears off");
        let torn = state
            .float("packet#0")
            .expect("the card is floating after a tear-off");
        assert_eq!(
            torn.home,
            pinion_core::detach::DetachHome::Window,
            "this host prefers a window, which is what makes the crossing real"
        );
        // Put its window somewhere the crossing lands INSIDE the canvas, so
        // this leg reads the conversion rather than the pull-in. The opening
        // (120, 40) is above and left of a host at (300, 150) and would be
        // pulled to the corner — a real behaviour, asserted in the test below.
        let before = super::Float {
            x: 900,
            y: 600,
            ..torn
        };
        state.set_float("packet#0", &before);

        super::ShellOracle::set_detach_home(
            &state,
            &super::IntrospectValue::Text("packet#0,canvas".into()),
        )
        .expect("the canvas is a home this host admits");
        let after = state.float("packet#0").expect("it is still floating");
        // ⚠ The offset is the CANVAS's origin on the display, not the window's:
        // a float's stored pair is in the canvas's own frame. Read from the
        // screen's own derivation rather than written down, or this gate would
        // assert a second spelling of the layout.
        let canvas = super::canvas_rect();
        let (ox, oy) = (300 + canvas.x, 150 + canvas.y);
        assert_eq!(
            (after.x, after.y),
            (before.x - ox, before.y - oy),
            "★★★★★ the numbers CROSS by the canvas's origin on the display. \
             Equal to `before` here is the defect this round repaid, not a pass"
        );
        assert_eq!(
            state.arrival.get(),
            Some(pinion_core::detach::Arrival::Kept),
            "and the wire says it landed where the reader last saw it"
        );

        // Back again: a card sent to the canvas and returned must not drift, or
        // a reader who changes their mind pays a display origin every trip.
        super::ShellOracle::set_detach_home(
            &state,
            &super::IntrospectValue::Text("packet#0,window".into()),
        )
        .expect("a window is a home this host admits");
        let back = state.float("packet#0").expect("it is still floating");
        assert_eq!(
            (back.x, back.y),
            (before.x, before.y),
            "a round trip is the identity"
        );
    });
}

/// ★★★★★ R1905 — **a card crossing into the canvas can still be picked up.**
///
/// The display's space is larger than this window, so a window near the far
/// corner of a desktop converts to a host coordinate past its edge — and a
/// panel whose header is outside the canvas cannot be grabbed again. R1903
/// measured the weaker rule's cost one gesture over: a reachability check
/// satisfied by a point *outside the window* is not a check, so this asks the
/// screen's own hit test, at a point inside the window, for the panel's header.
#[test]
fn r1905_a_card_crossing_into_the_canvas_stays_reachable() {
    Owner::new().run(|| {
        let state = use_shell_state();
        pinion_core::external::publish_window_origins([(super::MAIN_WINDOW, (0, 0))]);
        super::ShellOracle::detach(&state, "packet#0").expect("a board card tears off");
        // Put its window out past the far corner of this window, which is where
        // a second monitor's coordinates are.
        let float = state.float("packet#0").expect("it is floating");
        state.set_float(
            "packet#0",
            &super::Float {
                x: 4000,
                y: 3000,
                ..float
            },
        );
        super::ShellOracle::set_detach_home(
            &state,
            &super::IntrospectValue::Text("packet#0,canvas".into()),
        )
        .expect("the canvas is a home this host admits");

        let landed = state.float("packet#0").expect("it is still floating");
        // ⚠ The bound is the CANVAS's, not the window's: a canvas float's
        // rectangle is in the canvas's own frame. Read from the screen's
        // derivation so this gate cannot be a second spelling of the layout.
        let canvas = super::canvas_rect();
        assert!(
            landed.x + landed.w <= canvas.w && landed.y + landed.h <= canvas.h,
            "the whole panel is inside the canvas it crossed into: \
             {landed:?} in {}x{}",
            canvas.w,
            canvas.h
        );
        assert!(
            matches!(
                state.arrival.get(),
                Some(pinion_core::detach::Arrival::PulledIn { .. })
            ),
            "and it SAYS it was moved rather than reporting the place asked for"
        );
        // The property is not the rectangle — it is that a hand can reach it.
        // Asked of the screen's own hit test, in WINDOW coordinates, at a point
        // the window contains: R1903 measured that a reachability check
        // satisfied by a point outside the window is not a check.
        let px = canvas.x + landed.x + landed.w / 2;
        let py = canvas.y + landed.y + 8;
        assert!(
            px < super::win_w() && py < super::win_h(),
            "the probe point is inside the window: ({px}, {py})"
        );
        assert!(
            matches!(super::Hit::at(&state, px, py), super::Hit::Float(ref id) if id == "packet#0"),
            "the panel's header answers a press at ({px}, {py}); got {:?}",
            super::Hit::at(&state, px, py)
        );
    });
}

// ── R1908: an arrangement outlives the run ───────────────────────────────

/// ★★★★★ R1908 — a shell state whose persistence is **in memory**.
///
/// R1908 made folding the palette a thing that is WRITTEN, so a gate that folds
/// it now reaches storage. Without this that storage is the person's own data
/// directory: running the suite would leave a folded palette behind for whoever
/// next opens the application, and two gates run in parallel would write over
/// each other.
///
/// It works by seeding the cache slot `arrangement_storage` reads BEFORE the
/// state is built, which is the injection point that function's own module
/// documents. Each test opens its own `Owner`, so each gets its own empty
/// store — isolation and a clean slate from one line.
///
/// ⚠ Only the gates that WRITE use it, which is a stated limit rather than a
/// principle: the other paths through `persist_arrangements` (saving and
/// deleting an arrangement) are not exercised by any gate here, so the rest of
/// this file does not reach storage. A census of which gates may write is what
/// would turn that from a fact into a guarantee, and this round does not have
/// one ⇒ registered as `debt-a-gate-can-write-into-the-persons-own-data-dir`.
use pinion_core::storage::Storage as _;

fn use_shell_state_off_disk() -> std::rc::Rc<super::ShellState> {
    let _: std::rc::Rc<pinion_platform_storage::AppStorage> = Owner::current()
        .expect("an Owner scope, as use_shell_state itself requires")
        .cache(super::STORAGE_CACHE_KEY, || {
            pinion_platform_storage::AppStorage::new(Box::new(
                pinion_core::storage::InMemoryStorage::new(),
            ))
        });
    use_shell_state()
}

/// ★★★★★ R1908 — **putting the palette away is WRITTEN**, so it can outlive the
/// run.
///
/// R1903 built the gesture and the placement was re-seeded from the
/// specification at every boot, so closing the application undid it. This
/// asserts the half a single process can see: the fold reaches storage, under
/// the panel's own tag, with the extent kept — a fold that forgot its extent
/// would re-open to nothing, which is the difference between folding and hiding.
#[test]
fn r1908_putting_the_palette_away_is_written_where_arrangements_are_kept() {
    Owner::new().run(|| {
        let state = use_shell_state_off_disk();
        assert!(
            state.storage.load(super::ARRANGEMENTS_KEY).is_none(),
            "nothing is written before anything is arranged"
        );
        let open_extent = state.palette_at.get().extent;

        super::ShellOracle::place_palette(&state, "fold").expect("the policy admits a fold");
        let bytes = state.storage.load(super::ARRANGEMENTS_KEY).expect(
            "★ folding the palette writes the session; without this the \
                     gesture is undone by closing the application",
        );
        let stored: super::StoredArrangements =
            serde_json::from_slice(&bytes).expect("what was written reads back");
        let at = stored
            .chrome
            .get(super::PALETTE_STORE_KEY)
            .copied()
            .expect("the palette is stored under its own tag");
        assert!(at.folded, "and it is stored FOLDED, which is the fact");
        assert_eq!(
            at.extent, open_extent,
            "with the width kept, so opening it gives back a size worth having"
        );

        // Unfolding is written too, or a person who puts the panel back finds
        // it away again tomorrow — the same defect in the other direction.
        super::ShellOracle::place_palette(&state, "unfold").expect("unfolding is never refused");
        let bytes = state
            .storage
            .load(super::ARRANGEMENTS_KEY)
            .expect("the session is still written");
        let stored: super::StoredArrangements =
            serde_json::from_slice(&bytes).expect("what was written reads back");
        assert!(
            !stored
                .chrome
                .get(super::PALETTE_STORE_KEY)
                .copied()
                .expect("still stored")
                .folded
        );
    });
}

/// ★★★★★ R1908 — **a believed session REACHES THE SCREEN.**
///
/// 🟥 This gate exists because a counterfactual found nothing catching its
/// absence: replacing `state.palette_at.set(restored.at())` with the
/// specification's own placement — reading the session, judging it, and then
/// throwing it away — left every other gate green. The write gate watches
/// storage, the refusal gate asserts the panel ends at the specification, and
/// that is exactly where a discarded restore also ends. ⇒ two gates whose
/// expectations coincide on the broken state are one gate.
#[test]
fn r1908_a_session_this_build_can_honour_reaches_the_screen() {
    Owner::new().run(|| {
        let state = use_shell_state_off_disk();
        assert!(
            !state.palette_at.get().folded,
            "the specification opens it showing, so a fold here can only come \
             from the session"
        );
        let put_away = pinion_core::edge_panel::EdgePlacement::folded_at(
            spec::PALETTE_OPENS.edge,
            spec::PALETTE_OPENS.extent,
        );
        let stored = super::StoredArrangements {
            version: super::ARRANGEMENTS_VERSION,
            arrangements: state.presets.borrow().saved(),
            chrome: std::collections::BTreeMap::from([(
                super::PALETTE_STORE_KEY.to_owned(),
                put_away,
            )]),
        };
        state.storage.save(
            super::ARRANGEMENTS_KEY,
            &serde_json::to_vec(&stored).expect("it serialises"),
        );

        super::restore_arrangements(&state);
        assert_eq!(
            state.palette_at.get(),
            put_away,
            "★ the palette is where the person left it. Equal to the \
             specification here is the state R1903 shipped, not a pass"
        );
        assert!(
            state.palette_restored.get(),
            "and the screen says so, which `at` alone cannot"
        );
        // The room the chrome takes follows, or the placement reached a field
        // and not the screen — the half-derivation R1903 measured.
        assert_eq!(
            super::palette_room(),
            spec::PALETTE_STRIP_W,
            "a folded palette takes its strip's width"
        );
    });
}

/// ★★★★★ R1908 — **a stored placement is JUDGED, and a refusal reaches the
/// person.**
///
/// A stored placement is the one input this screen takes that did not come from
/// this build: an older version wrote it, this build may have narrowed the
/// panel's range since, and a person can edit the file. R1902's finding on this
/// axis is that a state nothing judges can contradict its own declaration for
/// the life of the program — and a fallback nothing REPORTS is that defect one
/// step on, because the reader sees the panel in the wrong place and cannot
/// learn why.
#[test]
fn r1908_a_stored_placement_this_build_refuses_is_replaced_and_explained() {
    Owner::new().run(|| {
        let state = use_shell_state_off_disk();
        // ⚠ An EDGE this panel cannot be on, and the fixture had to be that
        // rather than a width. Measured while writing this gate: the palette is
        // `allowed: []` AND `Resize::Fixed`, so no extent and no fold is
        // refusable and `admit_opening` waves everything through — the
        // judgement was vacuously true here. `EdgePolicy::restore` refuses a
        // stored edge for a pinned panel exactly because of that, and the
        // assertion below is what proves this screen's own restore is judged at
        // all.
        let illegal =
            pinion_core::edge_panel::EdgePlacement::open(pinion_core::style::ChromeEdge::Left, 292);
        assert_ne!(
            illegal.edge,
            spec::PALETTE_OPENS.edge,
            "the fixture has to be an edge this panel is not on"
        );
        assert!(
            spec::PALETTE_POLICY
                .restore(illegal, spec::PALETTE_OPENS)
                .refused()
                .is_some(),
            "the fixture has to be a placement this build actually refuses, or \
             this gate asserts nothing"
        );
        let stored = super::StoredArrangements {
            version: super::ARRANGEMENTS_VERSION,
            arrangements: state.presets.borrow().saved(),
            chrome: std::collections::BTreeMap::from([(
                super::PALETTE_STORE_KEY.to_owned(),
                illegal,
            )]),
        };
        state.storage.save(
            super::ARRANGEMENTS_KEY,
            &serde_json::to_vec(&stored).expect("it serialises"),
        );

        super::restore_arrangements(&state);
        assert_eq!(
            state.palette_at.get(),
            spec::PALETTE_OPENS,
            "★ the panel opens where the specification says rather than where an \
             unreadable session asked; a boot has to produce a screen"
        );
        assert!(
            !state.palette_restored.get(),
            "and it does NOT claim to be a restored arrangement"
        );
        let said = state.toast.showing();
        assert!(
            said.as_ref()
                .is_some_and(|u| u.clause().contains("could not open where you left it")),
            "★ the refusal reaches the person: {said:?}"
        );
    });
}

// ── R1907: a hand can change a detached panel's home ─────────────────────

/// ★★★★★ R1907 — **the home is a thing a person can change**, through the same
/// verb the wire uses.
///
/// R1891 gave a detached card a home and R1905 made the geometry follow it, and
/// after both the only caller of the screen's verb was the wire dispatch: the
/// value existed, an agent could set it, and a reader looking at the panel had
/// no way to ask. This drives the control on the panel's own header and asserts
/// that it lands where the POLICY says, not where this test says.
#[test]
fn r1907_the_header_control_sends_a_panel_to_the_home_the_policy_names() {
    Owner::new().run(|| {
        let state = use_shell_state();
        pinion_core::external::publish_window_origins([(super::MAIN_WINDOW, (0, 0))]);
        super::ShellOracle::detach(&state, "packet#0").expect("a board card tears off");
        let torn = state.float("packet#0").expect("it is floating");
        let policy = super::detach_policy();
        let expected = policy
            .next_home(torn.home)
            .expect("this host has a second home");
        assert_ne!(
            expected, torn.home,
            "'somewhere else' must be somewhere else"
        );

        super::ShellOracle::act_on_hit(&state, super::Hit::FloatHome("packet#0".into()));
        let moved = state.float("packet#0").expect("it is still floating");
        assert_eq!(
            moved.home, expected,
            "★★★★★ the control sends the panel where the policy says the next \
             home is. Unchanged here is the state R1891 left, not a pass"
        );
        // And the geometry followed, which is R1905's seam being consulted by
        // this new channel rather than only by the wire.
        assert!(
            state.arrival.get().is_some(),
            "a crossing through the header control publishes how it arrived, \
             or a client watching the panel cannot tell it moved from relabelled"
        );

        // Pressing it again returns the panel, so the control is safe to try —
        // a person who presses an unfamiliar mark can undo it with the same one.
        super::ShellOracle::act_on_hit(&state, super::Hit::FloatHome("packet#0".into()));
        let back = state.float("packet#0").expect("it is still floating");
        assert_eq!(back.home, torn.home, "two presses are the identity of home");
    });
}

/// ★★★★★ R1907 — **the paint and the hit test walk ONE roster.**
///
/// Before this round the count of controls in a detached panel's header was the
/// literal `2`, written twice in the painter and twice in the hit test with
/// nothing comparing the four. That is this screen's standing
/// `debt-paint-and-gesture-read-two-facts` in the exact place a third control
/// had to go, so the roster is derived from the policy and asserted here from
/// BOTH sides: every control the header offers is drawn under its own wire name
/// AND answers a press in the box it was drawn in.
#[test]
fn r1907_every_control_a_detached_header_offers_is_drawn_and_pressable() {
    Owner::new().run(|| {
        let state = use_shell_state();
        pinion_core::external::publish_window_origins([(super::MAIN_WINDOW, (0, 0))]);
        super::ShellOracle::detach(&state, "packet#0").expect("a board card tears off");
        // On the canvas, because that is the home this screen paints; a
        // window-homed panel is drawn by its own window.
        super::ShellOracle::set_detach_home(
            &state,
            &super::IntrospectValue::Text("packet#0,canvas".into()),
        )
        .expect("the canvas is a home this host admits");

        let offered = super::float_affordances();
        assert!(
            offered.contains(&pinion_core::detach::DetachedAffordance::SendHome),
            "this host has two homes, so its detached header offers the control"
        );
        let float = state.float("packet#0").expect("it is still floating");
        let rect = super::float_rect(&float);
        let header = super::header_rect(super::local(rect));
        let mut answered = Vec::new();
        for (n, affordance) in offered.iter().enumerate() {
            let slot = super::float_affordance_rect(header, n);
            assert!(
                slot.w > 0 && slot.h > 0,
                "the {} control occupies nothing at slot {n}",
                affordance.wire()
            );
            // ⚠ Window-absolute means the CANVAS's origin is added, not just
            // the panel's. A canvas float's stored pair is in the canvas's own
            // frame — `Hit::at` folds that origin out before it reads the
            // floats — and the first draft of this gate omitted it and probed
            // (510, 19), which is up in the application bar. R1905 measured
            // exactly this and it caught the next round anyway, which is what
            // makes it worth a second note rather than a shorter one.
            let canvas = super::canvas_rect();
            let px = canvas.x + rect.x + slot.x + slot.w / 2;
            let py = canvas.y + rect.y + slot.y + slot.h / 2;
            let hit = super::Hit::at(&state, px, py);
            assert_eq!(
                super::hit_word(&hit),
                format!("float.packet#0.{}", affordance.wire()),
                "the press at ({px}, {py}) must answer for the control the \
                 roster puts in slot {n}; got {hit:?}"
            );
            answered.push(super::hit_word(&hit));
        }
        // Distinct, because a roster whose slots overlapped would answer the
        // same control twice and every assertion above would still hold.
        let mut sorted = answered.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            answered.len(),
            "two slots answer the same control: {answered:?}"
        );
    });
}

/// ★★★★★ R1909, redirected by R1911.1 — **the assembled tool's node lab opens
/// the way its behaviour canon does, and a hand can put a pane away and bring
/// it back.**
///
/// The campaign's order step 3, asserted where the standing rule says it has to
/// be: on the screen a reader actually runs. A gate inside `hello_node_lab`
/// proves the screen; this proves the TOOL — the lab is a mounted guest, and a
/// test reaching into the guest's own state would pass on an application that
/// never mounted it.
///
/// ⚠ R1911.1 reversed which direction the round trip runs, and
/// [`assert_the_lab_opens_the_way_its_canon_does`] carries the measurement that
/// forced it: opening the inspector folded cost 33 demo walks over three CI
/// rounds and un-reproduced the canon.
///
/// # What it reads, and why both facts
///
/// `opens` and `at` are the same bit and different facts, which is exactly what
/// R1902 published them side by side for:
///
/// * `opens.folded` is the DECLARATION — *this pane is put away when you
///   arrive.* A client restoring a session, or offering "put it back", needs
///   it.
/// * `at.folded` is WHERE IT IS. Equal to `opens` here is the claim that the
///   declaration reached the screen, which is not implied by declaring it: R1902
///   measured that until then a pane's opening placement went through no policy
///   at all and could contradict everything else it said.
///
/// And the PALETTE is read in the same breath, because a gate that only looked
/// at the folded pane could not tell "this screen opens one panel away" from
/// "this screen opens with no panels".
///
/// # Why the edit goes through `place`
///
/// It is the verb the surface publishes, so this is the round trip a client
/// actually has: read where it is, ask for it back, read again. The refusal is
/// driven too — asking a pane that does not fold to unfold must say so by name,
/// or a client cannot tell a rejected request from one that did nothing.
#[test]
fn r1909_the_walk_reaches_a_lab_whose_panes_open_as_its_canon_does() {
    use pinion_core::external::IntrospectValue;

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();

        // ★★★★★ ORDER IS THE POINT, and it is the opposite of every other walk
        // in this file. The arrangement is read BEFORE the walk, because this
        // test folds a pane by hand below and the walk opens whatever it finds
        // folded. Reading afterwards would be asking about a screen the walk
        // had already changed, and the arrangement assertions would be
        // unfalsifiable.
        state.go("lab").expect("the node lab section is open");
        let mut externals = state.screens.externals(&state.journey.get());
        let tags: Vec<String> = externals.iter().map(|e| e.tag.to_string()).collect();
        let lab = externals
            .iter_mut()
            .filter_map(|e| e.handle.introspect_mut())
            .find(|it| it.query("gate").is_ok())
            .unwrap_or_else(|| {
                panic!("no external of the lab section answers for the lab: {tags:?}")
            });

        let opening = as_json(lab.query("spec").expect("a slot"));
        let inspector = pane_of(&opening, "inspector");
        assert_the_lab_opens_the_way_its_canon_does(&opening);

        // ── the client puts it away, through the published verb ───────────
        //
        // ★★★★★ R1911.1 REVERSED THE DIRECTION OF THIS ROUND TRIP, and that is
        // the repair rather than a weakening. R1909 had this screen OPEN with
        // its inspector folded and unfolded it here; measured at R1911, that
        // opening choice took **33 demo walks** red for three CI rounds —
        // proven by reverting the one line and watching them come back — and
        // it also un-reproduced the behaviour canon, which R1902 had measured
        // to have no panel that opens folded at all. So the tool opens the way
        // its canon does, and the fold is what a HAND does.
        //
        // Every framework property R1909 built is still asserted here, and the
        // headline one is asserted MORE strongly: `opens` staying put while
        // `at` moves is a sharper claim when the pane moves AWAY from its
        // declaration than when it moves toward it.
        let said = lab
            .invoke("place", IntrospectValue::Text("inspector,fold".to_owned()))
            .expect("`place` is a declared action of this screen and this pane folds");
        println!("the screen said: {said:?}");

        let after = pane_of(&as_json(lab.query("spec").expect("a slot")), "inspector");
        let refused = lab
            .invoke("place", IntrospectValue::Text("rail,fold".to_owned()))
            .expect_err("the rail declares that it does not fold");
        let sentence = format!("{refused:?}");
        assert!(
            sentence.contains("rail") || sentence.contains("fold"),
            "★ a refusal names what was asked, or a client cannot act on it: \
             {sentence}"
        );
        drop(externals);
        assert_eq!(
            after["at"]["folded"],
            serde_json::Value::Bool(true),
            "★ the pane is put away now: {after}"
        );
        assert_eq!(
            after["at"]["extent"], inspector["opens"]["extent"],
            "★ folding KEPT its extent, so bringing it back gives a pane worth \
             having. A fold that forgot its width would be a hide: {after}"
        );
        // ★★ And the DECLARATION did not move. Folding changes where the pane
        // IS, never where it OPENS — which is the whole reason the surface
        // publishes two fields for one bit, and a client offering "restore the
        // default arrangement" reads the one that stayed still.
        assert_eq!(
            after["opens"]["folded"],
            serde_json::Value::Bool(false),
            "★ `opens` is a property of the build, not a record of what somebody \
             just did: {after}"
        );
        // ★ And back, so the walk below stands in a tool a reader would meet.
        {
            let mut externals = state.screens.externals(&state.journey.get());
            let lab = externals
                .iter_mut()
                .filter_map(|e| e.handle.introspect_mut())
                .find(|it| it.query("gate").is_ok())
                .expect("the lab section still answers");
            lab.invoke(
                "place",
                IntrospectValue::Text("inspector,unfold".to_owned()),
            )
            .expect("a folded pane unfolds");
        }

        // ── and the whole application still walks ─────────────────────────
        //
        // Last, and it is the half that makes this a claim about the TOOL. The
        // three surfaces the inspector draws are judged over the walk, so a
        // pane that opened folded and could not be opened would take them to
        // `stood: none` and this to non-conforming — which is exactly what a
        // first draft of this round did before `open_whatever_arrived_folded`
        // existed.
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        assert!(
            report.itinerary().iter().any(|key| key == "lab"),
            "the walk must stand in the node lab: {:?}",
            report.itinerary()
        );
    });
}

/// ★★★★★ R1909, redirected by R1911.1 — the ARRANGEMENT half of the walk
/// above: **both panes open showing, and the screen agrees with that
/// declaration.**
///
/// Split out because the walk asserts two different kinds of thing — what the
/// tool opens as, and what a client can then do to it — and because the whole
/// of it in one function is past this workspace's line budget. The split is
/// along the seam that was already there.
///
/// ★★★★★ **What R1911.1 changed, and why it is not a weakening.** R1909 had
/// the inspector open FOLDED. Measured at R1911: that one line took **33 demo
/// walks** red for three CI rounds — the inspector was painted 18 px wide
/// against a declared floor of 312, so 34 of its elements were absent, the
/// node lab published 133 addressable regions instead of hundreds, and every
/// walk that stands in the inspector failed. Proven by changing that line back
/// and watching them come back green. It also un-reproduced the behaviour
/// canon: R1902 measured that canon and found `paletteOpen: true` and **no
/// panel that opens folded at all**, wrote exactly that down, and R1909
/// replaced the finding with an argument from a *different* reference. The
/// standing order is that the behaviour canon is what this tool reproduces.
///
/// Nothing R1909 built is discarded — `EdgePlacement::folded_at`,
/// `EdgePolicy::admit_opening` and the published `opens`/`at` pair are
/// untouched, and the round trip above now folds the pane by hand instead,
/// which tests the same wire in the sharper direction: `opens` holding still
/// while `at` moves AWAY from it.
///
/// ⚠ The palette is read in the same breath as the inspector, and that is not
/// symmetry for its own sake: a gate that read only one pane could not tell
/// *this tool opens its panels* from *this tool has one panel*.
fn assert_the_lab_opens_the_way_its_canon_does(spec: &serde_json::Value) {
    let inspector = pane_of(spec, "inspector");
    assert_eq!(
        inspector["opens"]["folded"],
        serde_json::Value::Bool(false),
        "★★★★★ R1911.1 — the assembled tool declares its inspector opens \
         SHOWING. R1902 measured the behaviour canon and found no panel that \
         opens folded; R1909 opened this one folded anyway and took 33 demo \
         walks red for three CI rounds. The standing order is that the canon \
         is what we reproduce: {inspector}"
    );
    assert_eq!(
        inspector["at"]["folded"],
        serde_json::Value::Bool(false),
        "★ and it really is — `opens` reaching `at` is the claim, not the \
         declaration on its own: {inspector}"
    );
    assert_eq!(
        inspector["at"]["extent"], inspector["opens"]["extent"],
        "★ at the extent it declares, not at a default: {inspector}"
    );

    let palette = pane_of(spec, "palette");
    assert_eq!(
        palette["opens"]["folded"],
        serde_json::Value::Bool(false),
        "★ the palette opens SHOWING: 'what can I place' is the question a \
         reader arrives with. One panel away and one showing is the point: \
         {palette}"
    );
    assert_eq!(
        palette["at"]["folded"],
        serde_json::Value::Bool(false),
        "★ and it is showing: {palette}"
    );
}

/// One published pane, by the word the `place` verb takes.
///
/// A peer of [`placed_panel`] that answers the WHOLE row rather than its `at`,
/// because R1909's walk compares `at` against `opens` and needs both halves of
/// the same row — reading them through two lookups would let a future edit
/// answer them from two different reads of the surface.
fn pane_of(spec: &serde_json::Value, name: &str) -> serde_json::Value {
    spec["panes"]
        .as_array()
        .expect("the surface publishes its panes")
        .iter()
        .find(|pane| pane["name"] == name)
        .unwrap_or_else(|| panic!("{name} is a pane this surface publishes"))
        .clone()
}

/// ★★★★★ R1954 — **the sort this screen ANNOUNCES is the sort it DRAWS.**
///
/// R1952 replaced the sort arrow's character with a stroked path for a good
/// reason: `NotoSans-Regular` has no `U+25B2`, so a reader was being shown a
/// `.notdef` box beside a column heading. Nothing then asked whether the
/// drawing and the announcement still agreed, and nothing *could* have — the
/// paint's only check counted characters, the accessibility tree's only check
/// read the state, and the two never met. They meet here, on the screen this
/// project is judged on, which opens with a sorted feed.
///
/// ⇒ this is not two oracles. It is ONE rule — `col_sort_dir` over the screen's
/// own `alarm_sort` — asked at the two moments a reader can meet it: the mark a
/// sighted reader sees, and the `aria-sort` a listening reader hears. A repair
/// that moves only one of them fails here, which is exactly the state R1952
/// could have shipped and no gate would have said so.
///
/// ⚠ The DIRECTION is compared, not the presence of a mark. A gate that asks
/// *is there an arrow* goes green on an arrow pointing the wrong way — the
/// class R1945 recorded as *counting whether a property exists gives a green
/// light to whether it is right*.
#[test]
fn r1954_the_sort_this_screen_announces_is_the_sort_it_draws() {
    use pinion_a11y::{SortDirection, WidgetA11y};
    use pinion_widget_paint::indicator::Indicator;

    let (column, _) = spec::ALARM_OPENING_SORT;
    for ascending in [true, false] {
        Owner::new().run(|| {
            let state = use_shell_state();
            state.alarm_sort.set(Some((column, ascending)));

            let drawn: Vec<Indicator> = pinion_widget_paint::indicator::marks_in(&super::view(
                ScreenState::default(),
                pinion_core::Frame::default(),
            ))
            .into_iter()
            .filter(|mark| matches!(mark, Indicator::Sort { .. }))
            .collect();
            assert_eq!(
                drawn,
                vec![Indicator::Sort { ascending }],
                "the opening screen draws exactly one sort arrow and it points \
                 the way `alarm_sort` says (ascending={ascending})",
            );

            let said: Vec<(String, SortDirection)> =
                super::AnalyzerShellView::access_node(&ScreenState::default(), None)
                    .into_iter()
                    .filter_map(|node| node.sort.map(|dir| (node.tag.clone(), dir)))
                    .collect();
            assert_eq!(
                said.len(),
                1,
                "exactly one heading announces a sort (ascending={ascending}): \
                 {said:?}",
            );
            assert_eq!(
                said[0].1,
                SortDirection::from_ascending(ascending),
                "the heading tagged {} announces {:?} while the screen drew the \
                 {} arrow — the reader who sees the mark and the reader who \
                 hears the name are being told different things",
                said[0].0,
                said[0].1,
                if ascending { "ascending" } else { "descending" },
            );
        });
    }
}

/// ★★★★★ R1989 — **no screen this application mounts declares a path it then
/// shadows**, asked of the whole assembly rather than of the screen a walk
/// happens to be standing on.
///
/// # What it is asserting
///
/// `IntrospectSchema::field_for` is a linear first-match, so two fields
/// spelling one path do not both exist: the second is unreachable and nothing
/// ever said so. When the two sit on different channels the damage is precise
/// and silent — the transport judges the channel from the shadowing field, so a
/// verb declared under a state's noun is published to no client, and everything
/// that declaration carries (its argument grammar, the words it will take, its
/// conditional cases) reaches nobody. The action still *fires*, because invoke
/// reaches the impl and the impl knows the word, which is exactly why this can
/// ship: every walk that drives the verb keeps passing.
///
/// # Why the population is the roster and not a list
///
/// Measured at R1989, four such declarations were live on **three** mounted
/// screens, and each was a state read sharing a noun with its own verb. The one
/// that was ever caught was caught by a walk standing on that screen, and the
/// walk over this shell had passed all along because it probes this shell's own
/// external and a mounted screen's schema is not reachable from there.
///
/// So the population is `ScreenRoster::externals_everywhere` — every screen
/// this application mounts, whether or not a walk visits it, and a screen
/// mounted after this was written is covered without a row being added here.
#[test]
fn r1989_no_mounted_screen_shadows_a_path_it_declares() {
    let owner = Owner::new();
    owner.run(|| {
        let roster = super::screen_roster();
        let mut asked = 0_usize;
        let mut surfaces = 0_usize;
        for (key, mut externals) in roster.externals_everywhere() {
            asked += 1;
            for external in &mut externals {
                let Some(surface) = external.handle.introspect_mut() else {
                    continue;
                };
                surfaces += 1;
                let schema = surface.schema();
                assert_eq!(
                    schema.shadowed(),
                    None,
                    "the screen at {key} declares `{}` on its external `{}` and \
                     then answers that path with a DIFFERENT field's \
                     declaration, so what this one says is published to nobody \
                     — a path is a read or an action, never both",
                    schema.shadowed().unwrap_or_default(),
                    external.tag,
                );
            }
        }
        // The denominator, asserted rather than reported: a roster that handed
        // back nothing would satisfy every assertion above without checking a
        // single schema, and this test's whole claim is about coverage.
        assert!(
            asked >= 6,
            "the analyzer mounts six screens and this census reached {asked}",
        );
        assert!(
            surfaces >= asked,
            "{surfaces} introspectable surface(s) across {asked} mounted \
             screen(s) — a mounted screen that publishes no schema at all is \
             not something this application has, so reaching fewer surfaces \
             than screens means the population was not built",
        );
    });
}

/// ★★★★★ R2012 — **the status bullet is findable, in both palettes, and the
/// tone it shows comes from the vocabulary rather than from this screen.**
///
/// R1719 put a coloured disc on the status band because a refusal and a
/// confirmation were otherwise one picture, and its own comment says the disc
/// is the whole of what a sighted reader learns the tone from. It was drawn in
/// `inverse_primary` — a role whose declared ground is `inverse_surface`, not
/// the band it sits on — and the legibility table could not see that, because
/// `inverse_primary on surface` is a pairing nobody declared.
///
/// ★★★★★ THE POPULATION IS THE FRAMEWORK'S PALETTES AND NOT THIS SCREEN, AND
/// THIS GATE'S OWN COUNTERFACTUAL IS WHAT ESTABLISHED THAT. The first draft
/// checked the floor only in the palettes this shell BINDS, and putting `Done`
/// back on `inverse_primary` left it green — because `reference_palettes`
/// happens to bind a magenta for that role, reading **7.88** light and **5.97**
/// dark. Against `Theme::light` / `Theme::dark` the same mapping reads **1.70**
/// and **2.17**, under even the 3.0 a non-text mark is held to. So the screen
/// that HAD the wrong mapping was legible, and every application inheriting
/// the defaults was not. A counterfactual that passes is a statement about the
/// population, which is this repository's own standing lesson met again.
///
/// So this judges the PAINTED FRAME and then both palettes behind it:
///
/// - it walks the assembled application, says something in each tone, and reads
///   the fill off `shell.toast.tone` — the address the disc now carries;
/// - it requires that fill to be what the theme in force resolves the tone's
///   own role to, so a screen that re-decided the colour locally is caught;
/// - and it requires the tone's role to clear the non-text floor in the BOUND
///   palette and in the CANONICAL one, per mode — four palettes in all, which
///   is the assertion the old mapping fails.
///
/// ⚠ The floor is `Floor::Boundary` and not `Floor::Text`: the disc is a
/// graphical mark and WCAG 1.4.11 is the standard it answers. `Unchanged` takes
/// `on_surface_muted`, which is body ink and clears the text floor anyway —
/// holding all three to the weaker floor is deliberate, because the claim is
/// *a reader can find this mark*, not *a reader can read it*.
///
/// ```text
/// cargo test -p hello-analyzer-shell r2012 -- --nocapture
/// ```
#[test]
fn r2012_the_status_bullet_is_findable_in_both_palettes() {
    use pinion_core::contrast::contrast_ratio;
    use pinion_core::legibility::Floor;
    use pinion_core::theme::{ColorRole, ThemeMode};
    use pinion_core::utterance::{Tone, Utterance};

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );

        let mut judged = 0_usize;
        let mut readings: Vec<String> = Vec::new();
        for (mode, word) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
            state.theme.set_mode(mode);
            // ⚠⚠ THE FADE IS WHY THIS IS THREE STATEMENTS AND NOT ONE, AND THE
            // ROUND GOT IT WRONG TWICE BEFORE GETTING IT RIGHT.
            //
            // The shell paints from `theme_animated()`, so the frame right
            // after a mode change still carries the palette being left — the
            // first draft compared against `theme()` and read the OTHER
            // palette's tone out of the frame. The second draft ticked and then
            // compared the painted colour against `theme_animated()`, which is
            // WORSE: both sides then read one mid-flight value, so the equality
            // agreed whatever the spring was doing and the contrast numbers
            // were about no palette at all. Its own printout is what said so —
            // three DIFFERENT tones all reading exactly 21.00 is not a palette,
            // it is an animation caught between two.
            //
            // So: read once to arm the spring on the new target, drive it, and
            // then REQUIRE it to have arrived. The settled palette is what the
            // rest of this loop compares against, and the arrival is asserted
            // rather than assumed — an animated read is not a palette until
            // something has checked that it stopped moving.
            //
            // ⚠⚠ AND IT IS `settle_owner_animations` AND NOT ONE BIG TICK. A
            // draft here called `tick_animations(2.0)` — two seconds of a
            // 200ms spring, which reads like generous headroom and is in fact
            // ONE integration step of dt=2.0 at stiffness 400. The integrator
            // DETONATES: the arrival assertion's own printout showed every
            // channel of the theme saturated to 0 or 255, and the contrast
            // reading before it was a flat 21.00 for three different tones.
            // The helper does sixty steps of a sixtieth, which is what a
            // spring is integrated with.
            let _arming = state.theme.theme_animated();
            pinion_core::test_fixtures::settle_owner_animations(&owner);
            let theme = state.theme.theme_animated();
            let settled = if mode == ThemeMode::Dark {
                state.theme.dark_palette()
            } else {
                state.theme.light_palette()
            };
            assert_eq!(
                theme, settled,
                "the {word} fade has not arrived after 2s of a ~200ms spring, \
                 so every reading below would be of a palette that does not \
                 exist"
            );
            for tone in Tone::ALL {
                // Said through the same door a screen uses, so the toast is in
                // the state a person would put it in.
                let utterance = match tone {
                    Tone::Done => Utterance::done("it happened"),
                    Tone::Refused => Utterance::refused(&"it did not"),
                    Tone::Unchanged => Utterance::unchanged("it was already so"),
                };
                assert_eq!(
                    utterance.tone(),
                    tone,
                    "the constructor for {} must produce it",
                    tone.wire(),
                );
                state.say(utterance);

                let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
                let mut cache = pinion_runtime::LayoutCache::new();
                pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);

                let painted = find_fill(&scene, "shell.toast.tone").unwrap_or_else(|| {
                    panic!(
                        "the {word} palette's `{}` toast paints no bullet at \
                         `shell.toast.tone`, and that mark is the only thing \
                         telling the three tones apart",
                        tone.wire(),
                    )
                });
                let owed = theme.resolve(tone.role());
                assert_eq!(
                    painted,
                    owed,
                    "the {word} palette's `{}` bullet is painted {painted:?} \
                     where the tone's own role resolves to {owed:?} — a screen \
                     deciding the colour for itself is the defect this closed",
                    tone.wire(),
                );

                // ⚠⚠ TWO PALETTES PER MODE, AND THE SECOND ONE IS THE POINT.
                //
                // The first draft held the tone to a floor only in the palette
                // THIS SHELL BINDS, and its counterfactual passed: put `Done`
                // back on `inverse_primary` and nothing went red, because this
                // shell binds a magenta for that role and it reads 7.88 / 5.97
                // here. The defect was never this screen's — it is the
                // framework's DEFAULT palettes, where the same mapping reads
                // 1.70 and 2.17. A gate that only sees the bound palette
                // reports that any application inheriting the defaults is fine.
                //
                // ⇒ A tone's role must clear the floor in the palette on
                // screen AND in the canonical one it falls back to, because
                // `Tone::role` is the vocabulary's answer for every consumer
                // and not this shell's arrangement.
                let canonical = if mode == ThemeMode::Dark {
                    pinion_core::theme::Theme::dark()
                } else {
                    pinion_core::theme::Theme::light()
                };
                for (whose, palette) in [("bound", &settled), ("canonical", &canonical)] {
                    let ink = palette.resolve(tone.role());
                    let ground = palette.resolve(ColorRole::Surface);
                    let ratio = contrast_ratio(ink, ground);
                    assert!(
                        ratio >= Floor::Boundary.ratio(),
                        "the {whose} {word} palette resolves the `{}` tone to \
                         {ink:?}, which reads {ratio:.2} on the surface it is \
                         drawn against — under the {:.1} a mark a person must \
                         be able to FIND is held to",
                        tone.wire(),
                        Floor::Boundary.ratio(),
                    );
                    readings.push(format!("{whose}/{word}/{}={ratio:.2}", tone.wire()));
                    judged += 1;
                }
            }
        }
        // The denominator, asserted rather than reported: three tones, two
        // modes, and two palettes per mode — the bound one and the canonical
        // one. A loop that reached fewer would satisfy every assertion above
        // by never entering it, and the counterfactual that exposed the first
        // draft was precisely a population that was half this size.
        assert_eq!(
            judged,
            2 * 2 * Tone::ALL.len(),
            "two modes times two palettes times {} tones is the population \
             this claim is about",
            Tone::ALL.len(),
        );
        println!("[r2012] {}", readings.join(" "));
    });
}

/// ★★★★★ R2017 §5.50 — **this screen's palette departs from the framework's
/// on exactly the roles it says it does, and on no others.**
///
/// ★★★★★ R2019 — **and the palette is no longer hand-authored**, which is what
/// this round changed and what these numbers moved for. `reference_palettes`
/// used to be twelve transcribed overrides per mode; it now adopts the
/// committed authored documents, so the departures are whatever those documents
/// say and not what somebody typed. The counts went 11 -> **16** light and
/// 12 -> **19** dark, which is the size of the drift R2017 measured and could
/// not then fix: the hand copy and the source disagreed on twenty-three of
/// thirty-eight role-and-mode pairs.
///
/// ⚠⚠ THE MEASUREMENT THAT MADE THIS GATE WORTH HAVING, taken at R2017 and not
/// before: the design system exports its palette in a shape this framework can
/// read, and the two had never been compared — nine of nineteen roles apart in
/// light and fourteen of nineteen in dark, some of it systematic (this screen's
/// dark elevation ladder sat one rung off, its `surface_container_low` being
/// exactly the export's `surface`). That comparison is `Theme::differences`,
/// one call, and this gate is what keeps the answer honest now that the
/// document rather than a copy is what the screen reads.
///
/// ```text
/// cargo test -p hello-analyzer-shell r2017 -- --nocapture
/// ```
#[test]
fn r2017_the_screens_palette_departs_from_the_default_exactly_where_it_says() {
    use pinion_core::theme::Theme;

    let (light, dark) = super::reference_palettes();
    let mut judged = 0_usize;
    // ⚠ The two counts differ by three and the difference is a RESULT: the
    // light document happens to agree with the framework on `on_accent`,
    // `on_error` and `on_warning` (all three are white in both), while the dark
    // one departs on every role it binds. Nineteen is also the whole of what
    // either document carries, so the dark line says "every authored role
    // differs from the default" and the light line says "all but three".
    for (word, mine, base, owed) in [
        ("light", light, Theme::light(), 16),
        ("dark", dark, Theme::dark(), 19),
    ] {
        let differences = base.differences(&mine);
        assert_eq!(
            differences.len(),
            owed,
            "the {word} palette declares {owed} departures and makes {}: {}",
            differences.len(),
            differences
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        );
        // And the relation is symmetric in the roles it names, which is what
        // makes `differences` a comparison rather than a subtraction.
        let back = mine.differences(&base);
        assert_eq!(
            differences.iter().map(|d| d.role).collect::<Vec<_>>(),
            back.iter().map(|d| d.role).collect::<Vec<_>>(),
            "{word}: which roles differ cannot depend on which way round it is asked",
        );
        for (there, here) in differences.iter().zip(&back) {
            assert_eq!(
                (there.mine, there.theirs),
                (here.theirs, here.mine),
                "{word}: and the two colours swap with the question",
            );
        }
        println!(
            "[r2017] {word} departs on {}: {}",
            differences.len(),
            differences
                .iter()
                .map(|d| d.role.name())
                .collect::<Vec<_>>()
                .join(" ")
        );
        judged += 1;
    }
    assert_eq!(judged, 2, "both modes, or this says half of what it claims");

    // The negative control: a palette compared with itself differs nowhere.
    // Without it, a `differences` that always returned twelve rows of noise
    // would satisfy everything above.
    assert!(
        Theme::light().differences(&Theme::light()).is_empty(),
        "a palette agrees with itself on every role"
    );
}

/// ★★★★★ R2018 §5.50 — **the palette this screen BINDS is held to the
/// elevation rule, which until now only the framework's own two were.**
///
/// R57.X pins the surface progression and pins it on `Theme::light` and
/// `Theme::dark` — the palettes nothing ships. A screen that binds its own was
/// checked by nothing, which is the same hole R2012 found in the legibility
/// table one axis over: a rule stated for a declared set, and the shipped thing
/// outside it.
///
/// ⚠⚠ AND RUNNING THE PINNED RULE HERE WOULD HAVE FAILED, WHICH IS THE ROUND'S
/// FINDING RATHER THAN A BUG IN THIS PALETTE. That rule says `surface` is the
/// lightest tone in a light palette; measured at R2018, this screen puts it
/// BETWEEN `surface_container_low` and `surface_container` — and so does an
/// authored design system's export, written independently, in the same place.
/// A grey page carrying white cards is a light theme people design on purpose,
/// so `surface`'s position is a decision the framework does not get to make.
/// What both palettes DO satisfy, and what this asserts, is that the four
/// container tiers step one way.
#[test]
fn r2018_the_bound_palettes_elevation_ladder_runs_one_way() {
    use pinion_core::theme::Theme;

    let (light, dark) = super::reference_palettes();
    for (word, palette) in [("light", light), ("dark", dark)] {
        let inversions = palette.elevation_inversions();
        assert!(
            inversions.is_empty(),
            "the {word} palette this screen binds must step one way through its \
             container tiers, and inverts at {inversions:?}",
        );
    }
    // The denominator this gate depends on: four tiers make three steps, and a
    // ladder shortened to one rung would satisfy the loop above by having
    // nothing to compare.
    assert_eq!(
        Theme::ELEVATION.len(),
        4,
        "four container tiers is what the ladder is"
    );
    println!(
        "[r2018] both bound palettes step one way through {} tiers",
        Theme::ELEVATION.len()
    );
}

/// ★★★★★ R2019 §5.50 — **this screen is painted from the authored documents,
/// and the part of it that is NOT authored is named rather than noticed.**
///
/// The debt this closes reads *an authored theme arrives and nothing reads it*:
/// a design system emitted these two palettes for a long time, a gate on the
/// authoring side enforced that this framework's roles matched them, and no
/// code here ever opened the files. What made that possible to miss is that a
/// hand-transcribed copy LOOKS like adoption — the screen was the right colour
/// most of the time. It was measured wrong on twenty-three of thirty-eight
/// role-and-mode pairs at R2017.
///
/// Each document binds nineteen roles. What it LEAVES is everything this
/// vocabulary grew after the exporter was written, and this asserts they are
/// exactly those, in both modes, because *partly authored* has to be a claim
/// somebody can check rather than a thing they discover.
///
/// ⚠ R2020 took the unauthored list from four to **ten** and the authored count
/// did not move: the exporter is unchanged and this side added six roles, so
/// the whole of the growth lands on the framework's side of the gap. That is
/// the honest reading and it is why the two numbers are asserted separately —
/// a single "ten of twenty-nine" would let an exporter that lost a role look
/// like a vocabulary that gained one. The other half of this — that the
/// AUTHORING side's export is now short of six roles rather than four — is not
/// this repository's to fix ⇒
/// [[debt-the-framework-grew-two-colour-roles-the-authored-theme-does-not-carry]].
#[test]
fn r2019_the_screen_paints_from_the_authored_documents() {
    use pinion_core::theme::{ColorRole, Theme};

    let ((light, light_gap), (dark, dark_gap)) = super::authored_palettes();
    let left_to_the_framework = vec![
        ColorRole::Success,
        ColorRole::OnSuccess,
        ColorRole::Info,
        ColorRole::OnInfo,
        ColorRole::WarningContainer,
        ColorRole::OnWarningContainer,
        ColorRole::SuccessContainer,
        ColorRole::OnSuccessContainer,
        ColorRole::InfoContainer,
        ColorRole::OnInfoContainer,
    ];
    for (word, palette, gap, base) in [
        ("light", light, &light_gap, Theme::light()),
        ("dark", dark, &dark_gap, Theme::dark()),
    ] {
        assert_eq!(
            gap.missing, left_to_the_framework,
            "{word}: the document leaves exactly the roles added after it was written",
        );
        assert!(
            gap.unknown.is_empty(),
            "{word}: and binds no key naming no role: {:?}",
            gap.unknown
        );
        // The denominator, so a document that shrank could not pass by having
        // less to disagree about.
        assert_eq!(
            ColorRole::all().len() - gap.missing.len(),
            19,
            "{word}: nineteen roles are authored"
        );
        for role in &gap.missing {
            assert_eq!(
                palette.resolve(*role),
                base.resolve(*role),
                "{word}: `{}` is unauthored, so the framework's answer stands",
                role.name()
            );
        }
    }
    // ★★★★★ THE WIRE IS LIVE, and one pinned value is what says so. Everything
    // above would still hold if `authored_palettes` went back to returning hand
    // written literals — the shape of the gap is a property of the vocabulary,
    // not of where the colours came from. So one authored colour is pinned
    // here, chosen because it is the most distinctive tone the document
    // carries: if the screen stops reading the file, or a copy creeps back,
    // this is what fails.
    //
    // ⚠ It also fires when the authoring side re-exports a different accent,
    // and that is DELIBERATE: the committed document is a copy in the one sense
    // that matters, and nothing else here would notice it going stale.
    assert_eq!(
        light.resolve(ColorRole::Accent),
        pinion_core::style::Color::rgb(0x9A, 0x00, 0x4F),
        "the light accent is read from the authored document"
    );
    assert_eq!(
        dark.resolve(ColorRole::OnAccent),
        pinion_core::style::Color::rgb(0x0A, 0x0B, 0x0E),
        "and the dark document's own ink for that accent, which is not white"
    );
    println!(
        "[r2019] {} authored role(s) per mode, {} left to the framework",
        ColorRole::all().len() - light_gap.missing.len(),
        light_gap.missing.len()
    );
}

/// ★★★★★ R2019 §5.50 — **the declared pairings this screen does not clear are
/// PINNED, not driven to zero.**
///
/// ⚠⚠ A GATE THAT DEMANDED AN EMPTY LIST WOULD BE DEMANDING THE RIGHT TO CHANGE
/// SOMEBODY ELSE'S COLOURS. These tones are authored outside this repository
/// and a contrast floor is not this side's to enforce on them by editing. What
/// this side CAN do is state the list, so a pairing that joins it is red the
/// day it appears.
///
/// ★★★★★ AND THE LIST IS SHORTER THAN WHAT THIS SCREEN SHIPPED BEFORE, which is
/// the measurement that settled whether adoption was a regression: the
/// hand-transcribed palettes were short on **four** pairings in light and
/// **five** in dark; the authored documents are short on two and four. 9 -> 6.
/// Three of those six were already this repository's own debt — `outline` on
/// `surface` in both modes, `inverse_primary` on `inverse_surface` in light,
/// `accent` on `surface` in dark — and improve rather than appear. The two that
/// are the authoring side's to decide are `on_accent` on `accent` and
/// `on_error_container` on `error_container`, both dark.
#[test]
fn r2019_the_authored_palettes_shortfalls_are_pinned() {
    use pinion_core::legibility::{PAIRINGS, shortfalls};
    use pinion_core::theme::Theme;

    let (light, dark) = super::reference_palettes();
    let named = |palette: &Theme| -> Vec<String> {
        shortfalls(palette).into_iter().map(|(n, _)| n).collect()
    };
    assert_eq!(
        named(&light),
        vec![
            "inverse_primary/inverse_surface".to_owned(),
            "outline/surface".to_owned(),
        ],
        "the light palette's shortfall list has moved: {:?}",
        shortfalls(&light)
    );
    assert_eq!(
        named(&dark),
        vec![
            "on_accent/accent".to_owned(),
            "on_error_container/error_container".to_owned(),
            "accent/surface".to_owned(),
            "outline/surface".to_owned(),
        ],
        "the dark palette's shortfall list has moved: {:?}",
        shortfalls(&dark)
    );
    // The denominator: a table that shrank would empty these lists without
    // anything about the palettes having improved.
    //
    // ⚠ R2020 moved it from 21 to 24, and the shortfall lists above did NOT
    // move — which is the fact worth recording. The three pairings added are
    // the caution, right and informational container pairs, and the authored
    // documents bind none of the six roles they involve, so all three fall back
    // to this framework's values and clear (7.2–13.3, measured in
    // `Theme::light`'s own comment). ⇒ growing the table made the claim
    // STRONGER without moving what it reports, which is what a table that is
    // checked rather than curated looks like when it grows.
    assert_eq!(
        PAIRINGS.len(),
        24,
        "the declared table is twenty-four pairings"
    );
    // And the contrast that makes the finding legible: the framework's own
    // palettes clear all of it, which is why nobody had noticed that the
    // palettes a screen binds were never asked.
    for (word, palette) in [("light", Theme::light()), ("dark", Theme::dark())] {
        assert!(
            shortfalls(&palette).is_empty(),
            "the {word} framework palette clears the table: {:?}",
            shortfalls(&palette)
        );
    }
    println!(
        "[r2019] shortfalls pinned at {} light and {} dark of {} declared",
        named(&light).len(),
        named(&dark).len(),
        PAIRINGS.len()
    );
}

/// The ground and the word colour of the first badge carrying `tag`, plus the
/// word itself — which is the whole of what a badge shows a reader.
fn badge_paint(
    scene: &pinion_core::Scene,
    tag: &str,
) -> Option<(pinion_core::style::Color, pinion_core::style::Color, String)> {
    let mut found = None;
    scene.for_each_node(&mut |visit| {
        if found.is_some() || visit.node.tag() != Some(tag) {
            return;
        }
        let pinion_core::Scene::Container(node) = visit.node else {
            return;
        };
        let Some(pinion_core::Scene::Text(word)) = node.children.first() else {
            return;
        };
        found = Some((node.style.fill, word.style.fg_color, word.content.clone()));
    });
    found
}

/// Every painted tag under `stem`, in paint order.
fn painted_under(scene: &pinion_core::Scene, stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    scene.for_each_node(&mut |visit| {
        if let Some(tag) = visit.node.tag() {
            if tag.starts_with(stem) && !out.iter().any(|seen: &String| seen == tag) {
                out.push(tag.to_owned());
            }
        }
    });
    out
}

/// Take one fault of each severity the node lab OFFERS, through the verb the
/// screen publishes, and answer which rows now carry a defect.
///
/// ⚠⚠ THE SEVERITIES ARE READ OFF THE OFFERS, not assumed to be both. A first
/// draft asked for one blocking and one non-blocking and was refused by the
/// screen: measured at R2020, every fault a FORM can inject blocks, because the
/// one that does not — a key the target does not know — is `Scope::Document`,
/// and a form reports an undeclared leaf unplaceable rather than taking it.
/// That boundary is the lab's own published decision (`fault_scopes`), so
/// asking for a defect it says it cannot make would be a gate demanding the
/// screen be a different screen.
///
/// The offers come from the screen's own `faults` slot, so this picks from what
/// the tool admits rather than naming a configuration path this file would then
/// have to keep in step with the lab's.
fn inject_a_fault_of_each_severity(state: &std::rc::Rc<super::ShellState>) -> Vec<(String, bool)> {
    use pinion_core::external::IntrospectValue;
    use pinion_core::widgets::fault_injection::DefectKind;

    let mut injected: Vec<(String, bool)> = Vec::new();
    let mut externals = state.screens.externals(&state.journey.get());
    let lab = externals
        .iter_mut()
        .filter_map(|e| e.handle.introspect_mut())
        .find(|it| it.query("gate").is_ok())
        .expect("an external of the lab section answers for the lab");
    let offers = as_json(lab.query("faults").expect("the lab publishes its offers"));
    let rows = offers.as_array().cloned().unwrap_or_default();
    let severities: std::collections::BTreeSet<bool> = rows
        .iter()
        .filter_map(|row| row["blocks"].as_bool())
        .collect();
    assert!(
        !severities.is_empty(),
        "the lab offers no injectable fault at all, so no defect badge is \
         reachable: {offers}"
    );
    for blocks in severities {
        let offer = rows
            .iter()
            .find(|row| row["blocks"].as_bool() == Some(blocks))
            .expect("the severity came from these rows");
        let key = offer["key"].as_str().expect("an offer names its row");
        let kind = offer["kind"].as_str().expect("an offer names its arm");
        // The arm is the framework's vocabulary, so a wire word that stopped
        // parsing fails here rather than colouring something by accident.
        let parsed =
            DefectKind::from_wire(kind).unwrap_or_else(|| panic!("{kind:?} is not a fault arm"));
        assert_eq!(
            parsed.blocks(),
            blocks,
            "the offer's own `blocks` disagrees with the arm it names"
        );
        lab.invoke("inject", IntrospectValue::Text(format!("{key}:{kind}")))
            .unwrap_or_else(|why| panic!("`inject` refused an offer the screen made: {why:?}"));
        injected.push((key.to_owned(), blocks));
    }
    injected
}

/// Every status badge in a painted frame, paired with the state its OWN WORD
/// claims — not with a state this file decided for it.
///
/// ★★★★★ That is the load-bearing part. An applies badge paints
/// `Applies::wire`, so the chip says which state it is in, and `applies_state`
/// — the painter's published rule — says what that state should look like. A
/// table here would be a second spelling that agreed with the painter by
/// construction and checked nothing. The same for a defect: its severity is
/// read back from the PHRASE the badge paints, through the vocabulary the
/// injection used, so a badge reporting a defect other than the one taken fails
/// here rather than silently.
fn status_badges_painted(
    scene: &pinion_core::Scene,
    injected: &[(String, bool)],
    word: &str,
) -> Vec<(String, pinion_core::theme::StateTone)> {
    use pinion_core::widgets::config_form::{Applies, ConfigDefect};
    use pinion_widget_paint::config_form::{applies_state, defect_state};

    // The applies badges: every row of the inspector that has one, found by its
    // address rather than listed here.
    let mut here = Vec::new();
    for tag in painted_under(scene, "lab.form.applies.") {
        let (_, _, said) = badge_paint(scene, &tag)
            .unwrap_or_else(|| panic!("{tag} is painted and is not a badge"));
        let applies = Applies::from_wire(&said).unwrap_or_else(|| {
            panic!(
                "{tag} paints {said:?}, which is not an applies-scope this \
                 vocabulary has — the badge's own word is what says which state \
                 it claims"
            )
        });
        here.push((tag, applies_state(applies)));
    }
    assert!(
        !here.is_empty(),
        "the {word} frame paints no applies badge at all, so this gate asked \
         nothing"
    );
    // The defect badges the injections put on the screen.
    for (key, blocks) in injected {
        let tag = format!("lab.form.defect.{key}");
        let (_, _, said) = badge_paint(scene, &tag).unwrap_or_else(|| {
            panic!(
                "the tool took a fault at {key} and paints no badge at {tag} — \
                 the defect is on the row or it is nowhere"
            )
        });
        let reported = ConfigDefect::all()
            .into_iter()
            .find(|d| d.phrase() == said)
            .unwrap_or_else(|| panic!("{tag} paints {said:?}, which names no defect"));
        assert_eq!(
            reported.blocks(),
            *blocks,
            "{tag} reports {said:?}, whose severity is not the one that was \
             injected"
        );
        here.push((tag, defect_state(&reported)));
    }
    here
}

/// Put the shell in `mode`, let the theme fade ARRIVE, and paint a frame.
///
/// Answers the palette in force, the palette the shell binds for that mode, and
/// the painted scene.
///
/// ⚠ The shell paints from `theme_animated`, so the frame right after a mode
/// change still carries the palette being LEFT. Arm the spring, drive it, and
/// REQUIRE it to have arrived — R2012 measured what each of the two wrong ways
/// costs, and one of them makes every contrast reading a number about no palette
/// at all. `settle_owner_animations` and not one big tick: a 2.0-second `dt` is
/// one integration step of a 200 ms spring and detonates it.
fn settled_frame_in_mode(
    state: &std::rc::Rc<super::ShellState>,
    owner: &Owner,
    mode: pinion_core::theme::ThemeMode,
    word: &str,
) -> (
    pinion_core::theme::Theme,
    pinion_core::theme::Theme,
    pinion_core::Scene,
) {
    use pinion_core::theme::ThemeMode;

    state.theme.set_mode(mode);
    let _arming = state.theme.theme_animated();
    pinion_core::test_fixtures::settle_owner_animations(owner);
    let theme = state.theme.theme_animated();
    let bound = if mode == ThemeMode::Dark {
        state.theme.dark_palette()
    } else {
        state.theme.light_palette()
    };
    assert_eq!(
        theme, bound,
        "the {word} fade has not arrived, so every reading below would be of a \
         palette that does not exist"
    );

    let mut scene = super::view(ScreenState::default(), pinion_core::Frame::default());
    let mut cache = pinion_runtime::LayoutCache::new();
    pinion_runtime::compute_layout(&mut scene, &mut cache, super::WIN_W, super::WIN_H);
    (theme, bound, scene)
}

/// Hold one state's badge pairing to the text floor in each palette, and record
/// the readings.
///
/// ⚠⚠ TWO PALETTES PER MODE, and the second is the point (R2012). A gate that
/// only saw the palette THIS shell binds would report that every application
/// inheriting the framework's defaults is fine.
///
/// ⚠⚠ A SHORTFALL THIS SIDE DOES NOT OWN IS EXCUSED BY NAME, NOT IGNORED —
/// R2019's rule, met by a painted mark for the first time. `shortfalls` is asked
/// of the palette rather than compared with a literal, so what is excused is
/// exactly what that palette already declares itself short of; the caller pins
/// the set of excuses. Demanding zero would be demanding the right to change
/// colours authored in another repository, and silently skipping would be worse.
fn judge_badge_legibility(
    tone: pinion_core::theme::StateTone,
    word: &str,
    palettes: &[(&str, &pinion_core::theme::Theme)],
    readings: &mut Vec<String>,
    excused: &mut std::collections::BTreeSet<String>,
) {
    use pinion_core::contrast::contrast_ratio;
    use pinion_core::legibility::Floor;

    let pairing = format!("{}/{}", tone.on_container().name(), tone.container().name());
    for (whose, palette) in palettes {
        let ratio = contrast_ratio(
            palette.resolve(tone.on_container()),
            palette.resolve(tone.container()),
        );
        let already_short = pinion_core::legibility::shortfalls(palette)
            .into_iter()
            .any(|(name, _)| name == pairing);
        if already_short {
            excused.insert(format!("{whose}/{word}/{pairing}={ratio:.2}"));
        } else {
            assert!(
                ratio >= Floor::Text.ratio(),
                "the {whose} {word} palette paints `{}`'s badge word at \
                 {ratio:.2} on its own ground — under the {:.1} a word a person \
                 has to READ is held to",
                tone.word(),
                Floor::Text.ratio(),
            );
        }
        readings.push(format!("{whose}/{word}/{}={ratio:.2}", tone.word()));
    }
}

/// ★★★★★ R2020 §5.38 §5.50 — **the assembled tool's status badges are painted
/// on their own state's ground, and the state is read off the badge's word.**
///
/// # What was wrong
///
/// Measured on the behaviour canon this project reproduces, a status badge is a
/// chip FILLED with its state's low-emphasis tone: the inspector draws `HOT` on
/// the right-state ground and `RESTART` on the caution one, and the capture
/// screen draws `Drop` on the wrong one. Here every badge was the same shape —
/// the shared raised tier with an outline — and the state was carried by the
/// colour of eight small letters. `HOT` was drawn in `accent`, this
/// vocabulary's INTERACTIVE tone, so the one badge saying *this edit lands
/// immediately* read as a thing to press.
///
/// It could not be otherwise: filling a chip with a state's ground needs
/// `<state>_container` and `on_<state>_container`, and until R2020 the palette
/// had that pair for `error` alone. The four states had four different shapes,
/// which is the debt this closes — and the badge is the consumer that forced
/// them uniform.
///
/// # What this judges, and why each part
///
/// - **The walk first**, so the claim is about an application that reproduces
///   its specification rather than about a scene assembled here.
/// - **The tone is derived from the painted WORD.** The applies badge paints
///   `Applies::wire`, so the badge itself says which state it claims to be in,
///   and `applies_state` — the painter's published rule — says what that state
///   should look like. A table here would be a second spelling that agreed with
///   the painter by construction and checked nothing.
/// - **Defects are DRIVEN, not fixtured.** Both arms, through the lab's own
///   published `inject` verb, because `blocks` is the whole reason the caution
///   tier exists and a gate that only saw one of them would pass while the two
///   were painted alike — which is what they were.
/// - **Then the floor, in four palettes.** R2012's finding applies unchanged: a
///   pairing that clears in the palette a screen BINDS can be under the floor in
///   the canonical one every other application inherits, so both are asked, per
///   mode. The floor is `Floor::Text` and not `Boundary`: the reader's task
///   here is to READ a word, not to find a mark.
///
/// ```text
/// cargo test -p hello-analyzer-shell r2020 -- --nocapture
/// ```
#[test]
fn r2020_the_assembled_tool_paints_its_status_badges_on_their_states_ground() {
    use pinion_core::theme::{StateTone, Theme, ThemeMode};

    let owner = Owner::new();
    owner.run(|| {
        let state = use_shell_state();
        let report = walk_the_application(&state);
        assert!(
            report.conforms(),
            "the application did not reproduce its specification over the walk: {}",
            report.why().unwrap_or_default()
        );
        assert!(
            report.itinerary().iter().any(|key| key == "lab"),
            "the walk must stand in the node lab, which is where the inspector \
             is: {:?}",
            report.itinerary()
        );

        state.go("lab").expect("the node lab section is open");

        // ── drive one defect of each severity, through the published verb ──
        let injected = inject_a_fault_of_each_severity(&state);
        assert!(
            !injected.is_empty(),
            "no defect was driven, so no defect badge is judged below"
        );

        // ── read every status badge the assembled tool now paints ─────────
        let mut judged: Vec<(String, StateTone)> = Vec::new();
        let mut readings: Vec<String> = Vec::new();
        let mut excused: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (mode, word) in [(ThemeMode::Light, "light"), (ThemeMode::Dark, "dark")] {
            let (theme, bound, scene) = settled_frame_in_mode(&state, &owner, mode, word);
            let here = status_badges_painted(&scene, &injected, word);

            let canonical = if mode == ThemeMode::Dark {
                Theme::dark()
            } else {
                Theme::light()
            };
            for (tag, tone) in &here {
                let (ground, ink, said) =
                    badge_paint(&scene, tag).expect("the badge was found a moment ago");
                assert_eq!(
                    ground,
                    theme.resolve(tone.container()),
                    "{word}: `{tag}` says {said:?}, which is a `{}` state, and it \
                     is filled with something else — a screen deciding a state's \
                     colour for itself is the defect this closed",
                    tone.word()
                );
                assert_eq!(
                    ink,
                    theme.resolve(tone.on_container()),
                    "{word}: `{tag}`'s word is not the ink its own ground carries",
                );
                judge_badge_legibility(
                    *tone,
                    word,
                    &[("bound", &bound), ("canonical", &canonical)],
                    &mut readings,
                    &mut excused,
                );
                judged.push((tag.clone(), *tone));
            }
        }

        // ★ The denominator, asserted rather than reported. Two modes, and in
        // each of them every applies badge the inspector painted plus the two
        // driven defects — so a frame that stopped painting the inspector, or an
        // injection that quietly did nothing, takes this down instead of
        // satisfying every assertion above by never entering the loop.
        assert_eq!(
            judged.len() % 2,
            0,
            "the two modes judged different populations: {judged:?}"
        );
        assert_eq!(
            judged.len(),
            18,
            "measured at R2020: nine status badges per mode — seven `RESTART` \
             rows, one `HOT`, and the driven defect. A count that moved means \
             the inspector's rows moved, and this claim's population with them: \
             {judged:?}"
        );
        let states: std::collections::BTreeSet<&str> =
            judged.iter().map(|(_, tone)| tone.word()).collect();
        // ⚠ THREE of the vocabulary's four, measured: seven `RESTART` badges,
        // one `HOT`, and the driven defect, in each of two modes. The fourth is
        // named here rather than quietly absent — `info` has no painted badge in
        // this tool. The canon draws one, on the capture screen, where a
        // fragment's `First` / `More` marks sit on the informational ground and
        // ours are plain runs ⇒
        // [[debt-the-capture-screens-fragment-marks-are-drawn-as-plain-runs]].
        assert_eq!(
            states.iter().copied().collect::<Vec<_>>(),
            vec!["error", "success", "warning"],
            "the states this tool paints a filled badge for have changed: \
             {judged:?}"
        );

        // ★★★★★ THE EXCUSE LIST IS PINNED, so a pairing that joins it is red on
        // the day it appears.
        //
        // ⚠ ONE entry, and it is the finding this round hands on: the authored
        // dark document's error container carries its own foreground at
        // **4.16**, under the 4.5 a word is held to. R2019 pinned that pairing
        // as a shortfall nobody painted; R2020 is the round that paints it, so
        // it has a READER now. The values are authored in another repository and
        // are not this side's to edit ⇒
        // [[debt-the-framework-grew-two-colour-roles-the-authored-theme-does-not-carry]]
        // carries the handoff, and this line is what stops the list growing
        // quietly beside it.
        assert_eq!(
            excused.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["bound/dark/on_error_container/error_container=4.16"],
            "the pairings this tool paints and cannot hold to the floor have \
             changed — a new one is somebody's regression, and a departed one \
             should be celebrated in this comment rather than silently dropped"
        );
        println!(
            "[r2020] {} badge(s) judged over states {states:?}; excused \
             {excused:?}; {readings:?}",
            judged.len(),
        );
    });
}
