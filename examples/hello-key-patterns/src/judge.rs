//! ★★★★★ R1758 §5.27 §5.40 §2 #7 — **the key-pattern section's verdict about
//! itself, answered from the paint, wherever the screen is running.**
//!
//! # What forced this module, and the measurement that reopened it
//!
//! R1730 built this section against `docs/analyzer-keys-spec.json` and wired a
//! `conformance` hook so the assembled application could ask it. What the hook
//! answered with was `crate::spec`'s own tables, copied into a roster of
//! [`Part`](pinion_core::conformance::Part)s — the model, not the frame.
//!
//! R1742 then settled the rule this breaks: *judge from the paint; a verdict
//! read from the model is structurally consistent with the model, so it cannot
//! fail for the reason it exists*. That round wrote the rule in the node lab's
//! header and did not come back here, and R1747's closing sweep measured what
//! it cost. Standing on a page that is not this one, so this section had not
//! painted a frame in the session at all, the assembled application's own
//! report said:
//!
//! ```text
//! packets  showing=false  reproduced=  0 of 26  away=[all six surfaces]
//! lab      showing=false  reproduced=  0 of 15  away=[all three surfaces]
//! keys     showing=false  reproduced= 21 of 21  away=none      <-- here
//! logs     showing=false  reproduced= 15 of 15  away=none
//! ```
//!
//! Re-measured at R1758 before this file existed: unchanged. Nothing was
//! failing. The number was simply not about pixels — and a painter that drew
//! **nothing at all** would have produced the same 21.
//!
//! # Why this is not `crate::spec`, which already holds the same keys
//!
//! Because a screen compared with its own table is a screen agreeing with
//! itself. `crate::spec` is what the RUNNING screen needs — paint tags, column
//! widths, row data, the wording of a refusal — written by the same hand in the
//! same edit as the painter it feeds. The comparison it supports is worth what
//! it costs and it is a different claim: that this build is self-consistent.
//! The judgment below is made against `docs/analyzer-keys-spec.json`, extracted
//! from the behaviour reference in neutral vocabulary and reviewed as a claim.
//!
//! # Why it is not `#[cfg(test)]`
//!
//! `painted.rs` next door already read these three surfaces out of a painted
//! scene and held them against the pin in both directions — and it is a unit
//! test of this binary, so the assembled application could not see it. That is
//! the shape R1742 named *one unpublished*: the verdict existed and stopped at
//! this crate's own `cargo test`. This module is the same reading with no
//! `cfg`, so the standalone window, the page mounted in the analysis tool and
//! the sweep in `painted.rs` are **one rule with three entry points** rather
//! than three builds wearing one name.
//!
//! # What the titles come from, and why it differs per surface
//!
//! Two of the three surfaces carry no label a reader sees — a section header's
//! summary and a record pane's rows are addressed by tag and drawn as values —
//! so their parts are titled by this screen's table
//! ([`parts_titled`]). The column
//! headers ARE the words a reader reads, so they are titled by
//! [`parts_as_read`]: the run painted
//! inside each header's own tag. A painter that labelled the fourth column
//! anything at all would otherwise leave the difference invisible here, and
//! R1730 had to assert it in a separate test that only ran in this binary.

use pinion_core::conformance::{Built, DocumentReport, parts_as_read, parts_titled, titles_from};
use pinion_core::painted::PaintedRegions;

use crate::{VIEW_TAG, spec};

/// Where the section header's parts are addressed.
const HEADER: &str = "kp.header.";
/// Where the list's column headers are addressed.
const COLUMNS: &str = "kp.column.";
/// Where the record pane's parts are addressed.
const DETAIL: &str = "kp.detail.";

/// How much of `docs/analyzer-keys-spec.json` this build is showing.
///
/// The value the screen publishes on its own wire and the value it hands a host
/// that mounted it. One derivation, so a section that conforms in its own
/// window and is never asked as a page cannot be two builds wearing one name.
#[must_use]
pub fn conformance() -> DocumentReport {
    spec::document().report_from_paint(VIEW_TAG, &built)
}

/// One key-pattern surface, as the last painted frame has it.
///
/// Every arm is [`Built::Standing`]: this section's three surfaces are drawn
/// whenever it is showing, so none of them can be away for a reason of its own.
/// The one state that *is* away — the section has not painted at all — belongs
/// to the framework and is answered by
/// [`report_from_paint`](pinion_core::conformance::SpecDocument::report_from_paint),
/// which is exactly what this module exists to start using.
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
        // ★ Read, not titled — see the module header.
        "columns" => Built::Standing(parts_as_read(regions, COLUMNS)),
        "detail" => Built::Standing(parts_titled(regions, DETAIL, &title_in("detail"))),
        other => panic!("no key-pattern surface named {other}"),
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
        other => panic!("no key-pattern surface named {other}"),
    })
}
