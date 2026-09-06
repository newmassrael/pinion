//! ★★★★★ R1742 §5.27 §5.40 §2 #7 — **the node lab's verdict about its own
//! inspector, answered from the paint, wherever the screen is running.**
//!
//! # What forced this module
//!
//! R1732 wrote `docs/analyzer-inspector-spec.json` and compared the painted
//! inspector against it — inside a `#[cfg(test)]` module of this binary. R1738
//! then made an assembled application count the sections it was judged on, and
//! measured that of its six open sections **two** published a verdict and four
//! did not. This screen was the interesting one of the four: it is the only
//! section that already *had* a written specification and already compared
//! itself against it, somewhere the assembled application could not see.
//!
//! Two things were in the way, and only the second was work.
//!
//! **Where the parts come from.** The comparison read them out of a painted
//! `Scene` through a test-only fixture, and a screen answering
//! [`WidgetView::conformance`](pinion_shell::WidgetView::conformance) has no
//! scene: the hook is a question a host asks between frames. But the framework
//! already keeps what every surface's last frame painted
//! ([`pinion_core::painted`]), so the parts are readable from a running window
//! with nothing handed in — and since R1742 that store keeps what each mark
//! **reads**, which is the half the roster below is specified by.
//!
//! **What a screen says when the surface being judged is not on screen.** That
//! is the decision, and it is why this could not be wiring. This screen's
//! specified surfaces are *session-dependent*: the inspector draws rows once a
//! card is selected, and the roster one row collapses exists once that row is
//! opened. A lab nobody has touched paints none of them. Reporting that as
//! `0 of everything reproduced` would say a working screen is broken; reporting nothing
//! is what it did for ten rounds. So each surface answers
//! [`Built::Away`] with **its own
//! reason**, the report counts it as unjudged rather than as reproduced, and
//! the report does not reconcile while one is away. The verdict is honestly
//! about a session, and it says which session it is about.
//!
//! # ★★★★★ What running it measured: two of the three surfaces are ALTERNATIVES
//!
//! Not designed — found, on the first frame this function was asked about a
//! live session. `docs/analyzer-inspector-spec.json` specifies the enumeration
//! row **with its roster shut**, and says so in its own words, because a roster
//! standing over the row is a part of the row that is not always there. With
//! the roster open the row has an eighth part, and calling that a divergence
//! would report the screen as wrong for doing exactly what the reference does.
//!
//! So `enum_row` is away while `enum_roster` is standing, and the consequence
//! is worth stating plainly rather than discovering later: **this document
//! cannot be fully judged at any one instant.** A reader who wants the whole
//! verdict drives the session and reads twice — which is not a defect in the
//! report, it is what "the verdict is about a session" means when a session has
//! states that exclude each other. What the framework cannot yet say is that
//! two surfaces are alternatives rather than one being merely unopened; the
//! screen says it in the away sentence, and that is the only place it is said.
//!
//! ⚠ The condition for that away is **the sibling surface standing**, never
//! *the row has a part I did not expect*. The second would be an escape hatch
//! big enough to swallow the whole mechanism: any divergence could be dodged by
//! declaring surprise at it.
//!
//! # Why the paint and not the model
//!
//! Every reading here is of the marks the frame drew. The model would be easier
//! to ask and would be a **second account of the same fact**: a table of
//! intentions passes while the painter draws something else, which is exactly
//! the defect R1732 repaired — an enumeration drawn as a row of chips. The
//! screen's own paint sweep now drives *this* function rather than a copy of
//! it, so the standalone binary, the assembled application and the unit test
//! all judge one build by one rule.

use std::collections::{BTreeMap, BTreeSet};

use pinion_core::conformance::{Built, DocumentReport, Part};
use pinion_core::painted::PaintedRegions;

use crate::VIEW_TAG;
use crate::spec;

/// The tag every inspector row's parts are addressed under.
///
/// ★ R2053 — derived from the screen's declaration rather than spelled here.
fn form_stem() -> &'static str {
    crate::address::FORM_STEM
}

/// How much of `docs/analyzer-inspector-spec.json` this build is showing.
///
/// The value the screen publishes on its own wire and the value it hands a host
/// that mounted it. One derivation, so a section that conforms in its own
/// window and is never asked as a page cannot be two builds wearing one name.
#[must_use]
pub fn conformance() -> DocumentReport {
    // ★ R1747 obligation-3b — this was four lines here, and the capture viewer
    // copied them byte for byte, away sentence included. It is a fact about the
    // framework's paint store rather than about either screen, so it lives in
    // `SpecDocument::report_from_paint` now and both screens ask it.
    spec::inspector_document().report_from_paint(VIEW_TAG, &built)
}

/// One inspector surface, as the last painted frame has it — or the reason it
/// was not on that frame.
///
/// # Panics
///
/// If asked for a surface the specification does not name, which is a defect in
/// this file rather than a state the screen can reach: the population is the
/// document's own.
#[must_use]
pub fn built(regions: &PaintedRegions, surface: &str) -> Built {
    if let Some(why) = below_the_width_this_screen_lays_out_at(regions) {
        return Built::away(why);
    }
    // ★★★★★ R1909 — **the pane all three of these surfaces live in is folded**,
    // which is a state the screen is in and can point at.
    //
    // From R1909 the inspector OPENS folded, so this is the arrangement a
    // reader arrives at rather than an exotic one. Without this the three
    // surfaces still reported away — the rows are genuinely not on screen — but
    // each gave a reason that was FALSE: "no card is selected", "the roster is
    // shut", "no selected card carries the row". A card was selected and the
    // roster's state was not the point; the pane was put away. Measured through
    // the assembled tool's own walk, which failed carrying all three sentences.
    //
    // ⇒ ★★★★★ R1742's rule, hit from a new direction: *an away condition must
    // be a state the screen can NAME, not the condition under which I failed.*
    // A true verdict with a false reason is worse than a refusal, because a
    // reader acts on the reason.
    //
    // ⚠ Read from the STRIP, which is what a folded panel paints — this file
    // reads the frame and holds no model, and that discipline is what makes its
    // verdicts about the screen. A folded pane is not an absence here: it draws
    // something, and that something is its name.
    if regions.rect_of(&folded_inspector_strip()).is_some() {
        return Built::away(
            "the inspector is folded to its strip, so it draws no rows to judge — \
             which is how this screen OPENS, and a press on the strip is what \
             brings it back",
        );
    }
    match surface {
        "enum_row" => {
            let parts = regions.parts_of(form_stem(), spec::ENUM_KEY);
            // ★★★★★ The row exists on screen exactly when its `key` part does,
            // and testing THAT rather than "any part at all" is load-bearing.
            // Measured by running the standalone binary: with no such row on
            // the card, the palette still paints the chip that would ADD it,
            // and that chip is tagged under the ADD part with the row's own
            // address. Reading "any part" therefore found the palette's chip,
            // called the row present, and reported all seven specified parts
            // absent. The chip is painted exactly when the row is NOT, so a
            // test on `key` cannot see it and no exclusion list is needed.
            // (The two sharing an address is
            // `debt-a-rail-tag-prefix-holds-seats-and-chrome-alike` one level
            // down: a tag prefix here holds a row's parts and a palette's chip
            // alike.)
            if !parts.iter().any(|(part, _)| part == "key") {
                return Built::away(format!(
                    "no selected card carries the `{}` row, so the row this surface \
                     is specified for is not on screen",
                    spec::ENUM_KEY,
                ));
            }
            // ★★★★★ The pin specifies this row **with its roster shut** — and
            // says so, because a roster standing over the row is a part of the
            // row that is not always there. Measured the first time this
            // function ran against a live frame: with the roster open the row
            // has an eighth part the pin does not declare, and calling that a
            // divergence would report the screen as wrong for doing exactly
            // what the reference does.
            //
            // ⚠ The condition is **the sibling surface standing**, not "the row
            // has a part I did not expect". The second would be an escape
            // hatch: every divergence could be dodged by declaring surprise.
            // This one is a state the screen is in and can point at.
            if roster_is_open(regions) {
                return Built::away(format!(
                    "the `{}` roster is standing over this row, and this surface is \
                     specified for the row with its roster shut",
                    spec::ENUM_KEY,
                ));
            }
            Built::Standing(titled(parts, &row_part_title))
        }
        "enum_roster" => {
            let stem = roster_stem();
            let words = regions.parts_under(&stem);
            if words.is_empty() {
                return Built::away(format!(
                    "the `{}` roster is shut, so it has no options on screen",
                    spec::ENUM_KEY,
                ));
            }
            Built::Standing(
                words
                    .into_iter()
                    .map(|(word, _)| {
                        // ★ The title is **the word the roster drew**, read back
                        // out of the run inside that option's box — not the key
                        // repeated. A roster whose third row drew the second
                        // word would otherwise be a surface nothing could tell
                        // apart, and the order is the whole reason this surface
                        // is specified.
                        let drawn = regions
                            .reads(&format!("{stem}{word}"))
                            .unwrap_or("<nothing is drawn in this option>");
                        Part::new(word, drawn.to_owned())
                    })
                    .collect(),
            )
        }
        "controls" => control_kinds(regions),
        other => panic!("no inspector surface named {other}"),
    }
}

/// ★★★★★ R1770 — **the surface is narrower than the width this screen declares
/// it lays out at**, and the reason, or `None` when it is not.
///
/// # What was measured
///
/// Driven at R1770 across eight window sizes with one binary, standing in this
/// section inside the assembled tool. The height moves nothing: at 900 tall and
/// at 1531 tall the verdict is identical. The width moves everything — surface
/// 1388 wide gives `controls` 1 of 5 and `enum_row` 1 of 7; 1548 gives 1 and 3;
/// 1748 gives 5 and 7. The parts that go are the right-hand ones: the type
/// word, the applies badge, the remove seat, the pick arrow.
///
/// That is not a defect and repairing it would undo a decision three rounds
/// measured. This screen declares a shrink policy whose comfortable width was
/// established at R1712–R1714 and whose own documentation says it in as many
/// words: *below it the app bar's right end and the inspector are clipped, and
/// above it nothing is*. So below that width the frame is not a frame of this
/// screen laid out — it is a slice of one — and a specification comparison
/// against it is a comparison with something the specification never described.
///
/// # ⚠ Why this is not the escape hatch R1742 refused
///
/// That round rejected an away-condition for `controls` and stated the test:
/// the condition must be **a state the screen is in and can point at**, never
/// *a case in which I would fail*. This one is the first kind, and by a wider
/// margin than the two conditions already in this file:
///
/// * it names a number the screen **declares to the framework** — the same
///   `comfortable` the window manager is told and `scene/size_floor` publishes,
///   not a threshold invented here for this check;
/// * it is read from the **host's** grant, so this screen cannot enter the
///   state by drawing badly, only by being given less room than it asked for;
/// * it makes the numbers **worse**, not better. Away counts as nothing
///   reproduced (R1742's own rule), so wherever this fires a section goes from
///   whatever it did reproduce to none of it, and from a section that fails to
///   one that is honestly not being asked. Nothing can be flattered by it.
///
/// ⚠ **That bullet used to name the shipped window and two counts** — *at
/// 1440x900 this section goes from 2 of 12 reproduced to 0 of 12*. True when
/// written, false by R1791, which brought the declared width down to what the
/// shell's page can give: the condition does not fire at the shipped size at
/// all now. The counts are DELETED rather than restated, because a count
/// written into prose beside the thing that answers it is what R1813 and R1814
/// each caught one round apart. The property is the argument; the instance
/// never was.
///
/// The alternative was a ledger entry per clipped part, at each measured width.
/// Refused: twelve entries would declare *this build does not reproduce its
/// specification* as an accepted difference, which is exactly the reading a
/// ledger must never be able to carry.
fn below_the_width_this_screen_lays_out_at(regions: &PaintedRegions) -> Option<String> {
    let extent = regions.extent()?;
    let (comfortable, _) = crate::comfortable_size();
    (extent.width() < comfortable).then(|| {
        format!(
            "this screen is laid out {comfortable} wide and clips below that, and the \
             surface it was given is {extent} — so what is on this frame is a slice of \
             the screen rather than the screen",
        )
    })
}

/// Where the roster's option boxes are addressed.
fn roster_stem() -> String {
    crate::address::form_part_prefix("option") + spec::ENUM_KEY + "."
}

/// The tag a folded inspector paints, derived from the pane's own name rather
/// than written out.
///
/// ★ R1909 — one spelling, because the painter builds it the same way
/// (`format!("{}.strip", which.tag())`) and a second literal here would be free
/// to drift from it. The pane is named by the specification, so a rename moves
/// both.
fn folded_inspector_strip() -> String {
    format!("{}.strip", spec::PANES[3].tag)
}

/// Whether the enumeration row's roster is standing over it.
///
/// The one state two of this document's surfaces are specified against, read
/// once so the two answers cannot disagree about which state the frame is in.
fn roster_is_open(regions: &PaintedRegions) -> bool {
    !regions.parts_under(&roster_stem()).is_empty()
}

/// Each part, titled by this screen's own table.
fn titled(
    parts: Vec<(String, pinion_core::scene::Rect)>,
    title: &dyn Fn(&str) -> Option<String>,
) -> Vec<Part> {
    parts
        .into_iter()
        .map(|(key, _)| {
            let said =
                title(&key).unwrap_or_else(|| format!("<{key} is painted and no table names it>"));
            Part::new(key, said)
        })
        .collect()
}

/// What one part of a form row is, in the words the specification uses.
///
/// The screen's table rather than the paint, because a row's parts carry no
/// label a reader sees: the specification fixes that they are THERE and in what
/// order, and what each one *is* is a fact about this build's vocabulary. Where
/// a title is drawn — the roster's words — it is read from the paint instead.
fn row_part_title(part: &str) -> Option<String> {
    let said = match part {
        "key" => "the configuration path this row is about",
        "type" => "the type word, and how many words are on offer",
        "applies" => "when an edit to this row lands",
        "remove" => "the seat that takes the row out of the form",
        "control" => "the box the value is entered in",
        "author" => "the seat that takes the row's value over",
        "disown" => "the seat that gives this row's half back",
        "aside" => "that the row is not configuration",
        "defect" => "what is wrong with the value",
        "shown" => "the word the row holds",
        "pick" => "the arrow that opens the roster",
        // ★★★★★ R1837 — `switch`, and it is the substrate's own track and knob
        // now. This read `toggle`, which was the form's hand-rolled mark: a
        // bordered pill carrying a tick or a SPACE, published as a part inside
        // the control and announced as a second checkbox beside it.
        "switch" => "the switch a boolean row is set with",
        "said" => "the row's spoken description",
        _ => return None,
    };
    Some(said.to_owned())
}

/// The kinds of value control this build draws, each titled by **what the paint
/// actually put inside it**.
///
/// Classified from the painted affordances rather than from the field's shape,
/// which is the whole point: a table of intentions passes while the painter
/// draws something else, and the defect R1732 repaired — an enumeration drawn
/// as a row of chips — is exactly a shape whose control was the wrong kind.
///
/// The order is the specification's, and this surface is the one place that is
/// not a claim: the reference's five kinds are the order its markup TESTS them
/// in, which no screen lays out. What is judged here is which kinds exist and
/// what each draws; a part out of place would say nothing, so nothing is
/// arranged to make it say something.
///
/// # ⚠ What this surface's verdict is about, measured
///
/// The pin says the build's side is read from *a form holding one row of each
/// kind*. A running screen paints **the selected card's** form, and most cards
/// do not hold one of each. Measured in the assembled tool at rest: the card it
/// opens on draws **three** kinds — a text box, a row of permission chips and a
/// list — so of the five the pin fixes, three report absent and one reports out
/// of place. Those sentences are true about the session and read as claims
/// about the build, which is the R1738 defect one level down.
///
/// It is reported anyway, and the alternative was considered and refused: an
/// away-condition of *the form does not hold a row of every specified kind*
/// would be an escape hatch wide enough to swallow the mechanism — a painter
/// that drew an enumeration as a row of chips (the exact defect R1732 repaired)
/// would take the enum kind off the surface and the surface would decline to be
/// judged. Deciding it from the FIELD SHAPES instead of from the classification
/// would be sound, and this hook has no model to read. Recorded in the pin's own
/// comment rather than papered over.
fn control_kinds(regions: &PaintedRegions) -> Built {
    let rows = shown_rows(regions);
    if rows.is_empty() {
        return Built::away(
            "no card is selected, so the inspector draws no value controls to classify",
        );
    }
    let mut found: BTreeMap<&'static str, String> = BTreeMap::new();
    for row in &rows {
        let parts = row_families(regions, row);
        let has = |p: &str| parts.contains(p);
        let (kind, drawn) = if has("author") {
            ("derived", "a read-out with no way to write into it")
        } else if has("pick") {
            (
                "enum",
                "a collapsed control holding one word, and an arrow that opens the roster",
            )
        } else if has("switch") {
            // ★★★★★ R1837 — classified by the SWITCH the row paints, which is
            // what the specification's own sentence says a boolean is. It was
            // classified by a `toggle` part that no longer exists: the control
            // IS the switch now, so the form publishes no part for it and the
            // painter is `pinion_widget_paint::switch` rather than a
            // thirteenth hand-rolled track.
            ("bool", "a switch, and the word it is set to")
        } else if has("step") {
            ("int", "a stepper that cannot leave the declared range")
        } else if has("item") {
            ("list", "one row per element, and a row that appends one")
        } else if has("option") {
            ("perm", "one chip per permission word, each on or off")
        } else {
            ("text", "a box holding the value as text")
        };
        found.entry(kind).or_insert_with(|| drawn.to_owned());
    }
    // The specification's order first, then anything this build has beyond it —
    // which is where the two second-pass controls land, and where the ledger
    // expects them.
    let mut out = Vec::new();
    for kind in ["text", "enum", "bool", "perm", "derived"] {
        if let Some(drawn) = found.remove(kind) {
            out.push(Part::new(kind, drawn));
        }
    }
    for (kind, drawn) in found {
        out.push(Part::new(kind, drawn));
    }
    Built::Standing(out)
}

/// Every configuration path the inspector currently draws a row for, in reading
/// order.
///
/// Read from the `key` part each row has exactly one of, so the population is
/// **what is on screen** rather than what the model holds. The two differ, and
/// the difference is the point: a row scrolled out of the pane is not a control
/// this frame drew, and a verdict that counted it would be about the model.
///
/// ★★★★★ `pub(crate)` for a gate, and the gate exists because a counterfactual
/// found the hole: reading a **wider** population than this — every part family
/// as if it were a row — changed nothing the conformance check could see. Extra
/// addresses carry no part family, so they classify as the plain text control,
/// which the surface already has; the kinds are a SET and a spurious member of
/// it is invisible. Reading too narrow a population loses a kind and fails
/// loudly, so only one direction was covered. The paint's rows are now compared
/// with the model's, which is the one other account of the same fact.
pub(crate) fn shown_rows(regions: &PaintedRegions) -> Vec<String> {
    let mut found: Vec<(String, pinion_core::scene::Rect)> = Vec::new();
    for (tag, rect) in regions.marks() {
        let Some(address) = crate::address::form_part_key("key", tag) else {
            continue;
        };
        if address.is_empty() || found.iter().any(|(seen, _)| seen == address) {
            continue;
        }
        found.push((address.to_owned(), rect));
    }
    pinion_core::painted::in_reading_order(found)
        .into_iter()
        .map(|(address, _)| address)
        .collect()
}

/// Which part FAMILIES the paint gave the row addressed `address`.
///
/// ★★★ A second reading beside [`PaintedRegions::parts_of`], and the difference
/// is a fact about this widget's tag vocabulary rather than a preference: **a
/// form row's parts come in two shapes.** Most are `<family>.<address>` exactly
/// — the key, the type badge, the seat, the chevron — and the ones a shape can
/// have several of carry a discriminator after the address: `option.<key>.
/// <word>`, `step.<key>.up`, `item.<key>.<n>`. The address-suffix reading finds
/// the first kind and cannot find the second, and the first draft of this gate
/// reported `perm`, `int` and `list` absent for exactly that reason.
///
/// A family holds no dots, so the seam is the FIRST segment after the prefix,
/// and what follows is the address with whatever the shape appended to it.
/// ⇒ `debt-a-rail-tag-prefix-holds-seats-and-chrome-alike` is the same question
/// one level up, and is still open.
fn row_families(regions: &PaintedRegions, address: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (tag, _) in regions.marks() {
        let Some(rest) = tag.strip_prefix(form_stem()) else {
            continue;
        };
        let Some((family, drawn)) = rest.split_once('.') else {
            continue;
        };
        if drawn == address
            || drawn
                .strip_prefix(address)
                .is_some_and(|tail| tail.starts_with('.'))
        {
            found.insert(family.to_owned());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use pinion_core::painted::{Extent, PaintedRegions};
    use pinion_core::scene::Rect;

    use super::{Built, built};

    /// A surface with nothing on it but the extent the host granted, which is
    /// all the away condition reads.
    fn given(width: u32) -> PaintedRegions {
        PaintedRegions::from_marks(vec![("lab.appbar".to_owned(), Rect::new(0, 0, width, 54))])
            .with_extent(Extent::new(width, 900))
    }

    /// ★★★★★ R1791 — the away condition's SHAPE, tested where it can still be
    /// produced.
    ///
    /// # Why this is here and not in a demo
    ///
    /// It was in one — `r1770`'s section D drove the assembled tool to a window
    /// where this screen declined and read the sentence off the wire. R1791 made
    /// that state unreachable: this screen's declared width came down to what
    /// the shell's narrowest window can give it, and the shell will not take a
    /// window narrower than its own floor. So the demo's subject went away
    /// because the defect it was about was fixed — and the assertions moved
    /// rather than died.
    ///
    /// Which repays something the same round created. Until this test was
    /// written the away condition had **no test at all**: every check of it went
    /// through a window size, and R1791 had just made every such window
    /// impossible to ask for.
    #[test]
    fn r1791_an_away_names_both_numbers_and_the_relation_between_them() {
        let (comfortable, _) = crate::comfortable_size();
        let narrow = comfortable - 200;
        let away = built(&given(narrow), "enum_row");
        assert!(away.parts().is_none(), "a surface below the width is away");
        let Built::Away(why) = away else {
            unreachable!("checked above")
        };

        // The two numbers, each found on its own so a failure says which is
        // missing rather than that the sentence changed.
        assert!(
            why.contains(&format!("laid out {comfortable} wide")),
            "★★ the reason names the width this screen DECLARES it lays out at, \
             so a reader is not left to find it: {why}"
        );
        assert!(
            why.contains(&format!("{narrow}x900")),
            "★★ and the extent it was actually GIVEN, in the same sentence: {why}"
        );
        assert!(
            narrow < comfortable,
            "★★★★★ and the two stand in the relation that makes declining honest: \
             it was given LESS than it declares. This is a state of the host's \
             grant rather than a case in which the judge would fail, which is the \
             test R1742 set for an away condition"
        );
    }

    /// ★★★★★ R1821 — **a mounted screen does not decline a page over chrome its
    /// host is drawing**, which is the whole of this round in one assertion.
    ///
    /// The width in the middle is the point: `MIN_W - RAIL_W` is a page too
    /// narrow for the screen that draws its own rail and wide enough for the
    /// same screen mounted where the host draws one. Before this round both
    /// answers were the first, so the assembled tool's section declined pages
    /// that would have fitted — and it declined them by 54 pixels, the width of
    /// a rail it was not painting.
    ///
    /// ★ Both halves are asserted, and the standalone half is not a formality:
    /// it is what says the derivation SUBTRACTS rather than simply returns a
    /// smaller number. A `comfortable_size` that ignored the host entirely would
    /// pass the first assertion; one that always subtracted would pass the
    /// second; only a screen that answers the two differently passes both.
    ///
    /// # ⚠⚠ It reads the width condition, and the first draft read `built`
    ///
    /// That draft asserted `Away` standalone and *not* `Away` mounted, and both
    /// assertions were unsound in opposite directions. [`given`] is a surface
    /// carrying an extent and nothing else, so once the width lets a frame
    /// through, `built` declines it anyway — the row this surface is specified
    /// for is not on that frame either. So the standalone half **passed for a
    /// reason that had nothing to do with the width**, and the mounted half
    /// **could not pass at any width**.
    ///
    /// Two assertions over one fixture, one vacuous and one impossible, both
    /// reading a predicate that answers a different question than the one being
    /// asked. That is R1813's class, caught here two rounds after the round
    /// that named it — which is the argument for asserting on the *smallest*
    /// thing that changed rather than on the verdict that contains it.
    #[test]
    fn r1821_a_mounted_screen_is_not_charged_for_the_rail_its_host_draws() {
        use pinion_core::chrome::{HostChrome, Part, with_host_chrome};

        let (standalone, _) = crate::comfortable_size();
        let page = standalone - crate::RAIL_W;

        assert!(
            super::below_the_width_this_screen_lays_out_at(&given(page)).is_some(),
            "★ standalone this screen draws its own rail, so a page {page} wide \
             really is narrower than the {standalone} it lays out at"
        );

        with_host_chrome(HostChrome::NONE.with(Part::Navigation), || {
            let (mounted, _) = crate::comfortable_size();
            assert_eq!(
                mounted, page,
                "★★★★★ mounted where the host draws the navigation, this screen \
                 needs exactly the rail's width less -- derived from the same \
                 `draws_own_rail` the paint, the access tree, the keyboard ring \
                 and the hit test read, so the five cannot disagree"
            );
            assert!(
                super::below_the_width_this_screen_lays_out_at(&given(page)).is_none(),
                "★★★★★ and so the SAME page it declined standalone is judged \
                 here rather than declined -- a section that is given room it \
                 can use must not report itself as not being asked"
            );
        });
    }

    /// ★★★★★ R1821 — **the width the layout stops at and the width a frame is
    /// judged against are ONE width**, mounted as well as standalone.
    ///
    /// R1770 introduced [`crate::comfortable_size`] so that `judge` would *read*
    /// the declared number rather than restate it, and R1712 had already written
    /// the same property from the other end: the window's floor is "derived from
    /// `SHRINK`, the same value `window_size` clamps against". Subtracting the
    /// host's chrome in one of those readers and not the other is what would
    /// break it — and would break it silently, because the two are equal
    /// standalone and every rectangle assertion in `painted.rs` is standalone.
    ///
    /// What the disagreement would cost is specific: at a grant between the two
    /// numbers the layout would still ask for the larger one, so the framework
    /// would pan a page the judge had just called whole. A verdict that says
    /// *this frame is the screen laid out* while the layout is 54 pixels wider
    /// than the frame is the one thing this module may not do.
    #[test]
    fn r1821_the_layout_floor_and_the_judged_width_are_one_width() {
        use pinion_core::chrome::{HostChrome, Part, with_host_chrome};
        use pinion_core::external::with_surface_extent;

        for chrome in [HostChrome::NONE, HostChrome::NONE.with(Part::Navigation)] {
            with_host_chrome(chrome, || {
                let (comfortable, _) = crate::comfortable_size();
                // Granted exactly what this screen says it needs. The layout may
                // not then ask for more: `layout_size` floors the grant, so any
                // floor above `comfortable` shows up here as a wider answer.
                with_surface_extent(crate::VIEW_TAG, (comfortable, 900), || {
                    assert_eq!(
                        crate::window_size().0,
                        comfortable,
                        "★★★★★ the layout stops at the same width `judge` reads, \
                         so a page this screen calls sufficient is a page it \
                         lays out inside rather than one the framework pans"
                    );
                });
            });
        }
    }

    /// ★ And the condition is a threshold, not a slope: at exactly the declared
    /// width the width is not what puts the surface away.
    ///
    /// Written because `<` and `<=` are one character apart and the difference
    /// is whether a screen judges itself at the very width it declares — which
    /// is the width its own window floor resolves an ask up to, and therefore
    /// the most common width it will ever be judged at.
    #[test]
    fn r1791_at_exactly_the_declared_width_the_width_is_not_the_reason() {
        let (comfortable, _) = crate::comfortable_size();
        let Built::Away(why) = built(&given(comfortable), "enum_row") else {
            return; // standing: the width did not put it away, which is the claim
        };
        assert!(
            !why.contains("clips below that"),
            "★ at exactly {comfortable} the screen is laid out, not clipped -- \
             whatever else may be away here, it must not be the width: {why}"
        );
    }
}
