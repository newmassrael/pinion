//! ★★★★★ R1758 §5.27 §5.40 §2 #7 — **the log section's verdict about itself,
//! answered from the paint, wherever the screen is running.**
//!
//! The sibling of `hello-key-patterns`'s module of the same name, written in
//! the same round and for the same measurement. R1731 built this section
//! against `docs/analyzer-logs-spec.json` and wired a `conformance` hook; what
//! the hook answered with was `crate::spec`'s own tables copied into a roster
//! of [`Part`](pinion_core::conformance::Part)s.
//!
//! Measured on the assembled application, standing on a page that is not this
//! one — so this section had painted no frame in the session at all — it
//! reported **15 of 15 reproduced, nothing away**, beside two sibling sections
//! correctly reporting `0 of 26` and `0 of 15`. A painter drawing nothing would
//! have produced the same 15.
//!
//! Why the paint rather than the model, why this is not `#[cfg(test)]`, and
//! where the titles come from are all the same three answers as next door; the
//! key-pattern module states them at length and this one does not repeat them.
//! The one difference worth naming here: this section's list is **narrowed**
//! two ways at once, so the states its sweep runs are severity choices as well
//! as queries, and a surface that only conforms while everything is shown has
//! not been checked in the state a person reaches for when something is wrong.

use pinion_core::conformance::{Built, DocumentReport, parts_as_read, parts_titled, titles_from};
use pinion_core::painted::PaintedRegions;

use crate::{VIEW_TAG, spec};

/// Where the section header's parts are addressed.
const HEADER: &str = "lv.header.";
/// Where the list's column headers are addressed.
const COLUMNS: &str = "lv.column.";
/// Where the event pane's parts are addressed.
const DETAIL: &str = "lv.detail.";

/// How much of `docs/analyzer-logs-spec.json` this build is showing.
///
/// The value the screen publishes on its own wire and the value it hands a host
/// that mounted it. One derivation, so a section that conforms in its own
/// window and is never asked as a page cannot be two builds wearing one name.
#[must_use]
pub fn conformance() -> DocumentReport {
    spec::document().report_from_paint(VIEW_TAG, &built)
}

/// One log surface, as the last painted frame has it.
///
/// Every arm is [`Built::Standing`]: this section's three surfaces are drawn
/// whenever it is showing. The one state that *is* away — the section has not
/// painted at all — belongs to the framework and is answered by
/// [`report_from_paint`](pinion_core::conformance::SpecDocument::report_from_paint).
///
/// # Panics
///
/// If asked for a surface the specification does not name, which is a defect in
/// this file rather than a state the screen can reach: the population is the
/// document's own.
#[must_use]
pub fn built(regions: &PaintedRegions, surface: &str) -> Built {
    match surface {
        "header" => Built::Standing(parts_titled(regions, HEADER, &title_in("header"))),
        // ★ The column headers are the words a reader reads, so they are held
        // against the specification as drawn rather than as tabled.
        "columns" => Built::Standing(parts_as_read(regions, COLUMNS)),
        "detail" => Built::Standing(parts_titled(regions, DETAIL, &title_in("detail"))),
        other => panic!("no log surface named {other}"),
    }
}

/// What one part of `surface` is called, in this screen's own words.
///
/// # Panics
///
/// If asked about a surface with no table, which is the same defect as above.
pub fn title_in(surface: &str) -> impl Fn(&str) -> Option<String> + use<> {
    titles_from(match surface {
        "header" => spec::HEADER.iter().map(|p| (p.key, p.title)).collect(),
        "columns" => spec::COLUMNS.iter().map(|c| (c.key, c.title)).collect(),
        "detail" => spec::DETAIL.iter().map(|p| (p.key, p.title)).collect(),
        other => panic!("no log surface named {other}"),
    })
}
