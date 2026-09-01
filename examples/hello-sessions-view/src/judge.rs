//! ★★★★★ R1948 §5.27 §5.40 §2 #7 — **the sessions section's verdict about
//! itself, answered from the paint, wherever the screen is running.**
//!
//! The sibling of `hello-topology-view`'s module of the same name, and written
//! against the same measurement R1758 recorded: a verdict computed from this
//! crate's own tables is a verdict that cannot fail.
//!
//! ★ Both surfaces are [`Built::Standing`] whenever this section shows. The
//! status chips narrow which ROWS the list holds, which changes what `rows`
//! contains and not whether `rows` is there — a distinction worth stating,
//! because a filter that emptied the grid would leave a surface reporting zero
//! parts and that must read as a defect rather than as absence.

use pinion_core::conformance::{Built, DocumentReport, parts_titled, titles_from};
use pinion_core::painted::PaintedRegions;

use crate::{VIEW_TAG, spec};

/// Where the list pane's parts are addressed.
const LIST: &str = "sv.list.";
/// Where the detail pane's parts are addressed.
const DETAIL: &str = "sv.detail.";

/// How much of `docs/analyzer-sessions-spec.json` this build is showing.
#[must_use]
pub fn conformance() -> DocumentReport {
    spec::document().report_from_paint(VIEW_TAG, &built)
}

/// One sessions surface, as the last painted frame has it.
///
/// # Panics
///
/// If asked for a surface the specification does not name, which is a defect in
/// this file rather than a state the screen can reach.
#[must_use]
pub fn built(regions: &PaintedRegions, surface: &str) -> Built {
    match surface {
        "list" => Built::Standing(parts_titled(regions, LIST, &title_in("list"))),
        "detail" => Built::Standing(parts_titled(regions, DETAIL, &title_in("detail"))),
        other => panic!("no sessions surface named {other}"),
    }
}

/// What one part of `surface` is called, in this screen's own words.
///
/// # Panics
///
/// If asked about a surface with no table, which is the same defect as above.
pub fn title_in(surface: &str) -> impl Fn(&str) -> Option<String> + use<> {
    titles_from(match surface {
        "list" => spec::LIST.iter().map(|p| (p.key, p.title)).collect(),
        "detail" => spec::DETAIL.iter().map(|p| (p.key, p.title)).collect(),
        other => panic!("no sessions surface named {other}"),
    })
}
