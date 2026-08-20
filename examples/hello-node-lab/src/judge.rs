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
//! `0 of 15 reproduced` would say a working screen is broken; reporting nothing
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
use pinion_core::painted::{PaintedRegions, painted_regions};

use crate::VIEW_TAG;
use crate::spec;

/// The tag every inspector row's parts are addressed under.
const FORM: &str = "lab.form.";

/// The family that every shown row has exactly one of — which is what makes it
/// the roster of rows the paint can be asked for.
const KEY_FAMILY: &str = "lab.form.key.";

/// How much of `docs/analyzer-inspector-spec.json` this build is showing.
///
/// The value the screen publishes on its own wire and the value it hands a host
/// that mounted it. One derivation, so a section that conforms in its own
/// window and is never asked as a page cannot be two builds wearing one name.
#[must_use]
pub fn conformance() -> DocumentReport {
    let regions = painted_regions(VIEW_TAG);
    spec::inspector_document().report(&|surface| match regions.as_deref() {
        Some(regions) => built(regions, surface),
        // Not "reproduces nothing": a screen that has not painted has not been
        // asked to draw anything yet, and the two are different facts a reader
        // acts on differently.
        None => Built::away("this screen has not painted a frame yet, so none of it is on screen"),
    })
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
    match surface {
        "enum_row" => {
            let parts = regions.parts_of(FORM, spec::ENUM_KEY);
            // ★★★★★ The row exists on screen exactly when its `key` part does,
            // and testing THAT rather than "any part at all" is load-bearing.
            // Measured by running the standalone binary: with no such row on
            // the card, the palette still paints the chip that would ADD it,
            // and that chip is tagged `lab.form.add.<key>` — the row's own
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

/// Where the roster's option boxes are addressed.
fn roster_stem() -> String {
    format!("{FORM}option.{}.", spec::ENUM_KEY)
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
        "toggle" => "the switch a boolean row is set with",
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
        } else if has("toggle") {
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
        let Some(address) = tag.strip_prefix(KEY_FAMILY) else {
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
        let Some(rest) = tag.strip_prefix(FORM) else {
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
