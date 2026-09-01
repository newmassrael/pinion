//! ★★★★★ R1947 §5.27 §5.40 §2 #7 — **the topology section's verdict about
//! itself, answered from the paint, wherever the screen is running.**
//!
//! The sibling of `hello-log-view`'s and `hello-key-patterns`'s modules of the
//! same name, and written against the measurement R1758 recorded there: a
//! verdict computed from this crate's own tables is a verdict that cannot fail.
//! Measured then, on the assembled application, standing on a page that was not
//! the log section — so that section had painted no frame in the session at
//! all — it reported 15 of 15 reproduced. A painter drawing nothing would have
//! produced the same 15.
//!
//! So the roster comes out of [`PaintedRegions`] — the last frame this section
//! actually drew — and a section that is not showing reports what it is: away,
//! reproducing nothing, reconciling nothing.
//!
//! ★ One thing here is this section's own. Its three panes are all
//! [`Built::Standing`] whenever it shows, because unlike the log section's list
//! this screen narrows nothing away: a toggle hides a *class of link inside the
//! plot*, which changes what `canvas` contains and not whether `canvas` is
//! there. A surface that disappeared when a switch went off would be a surface
//! this roster had to report away, and the plot is deliberately not that.

use pinion_core::conformance::{Built, DocumentReport, parts_titled, titles_from};
use pinion_core::painted::PaintedRegions;

use crate::{VIEW_TAG, spec};

/// Where the filter rail's parts are addressed.
const FILTERS: &str = "tv.filters.";
/// Where the graph column's parts are addressed.
const GRAPH: &str = "tv.graph.";
/// Where the inspector's parts are addressed.
const INSPECTOR: &str = "tv.inspector.";

/// How much of `docs/analyzer-topology-spec.json` this build is showing.
///
/// The value the screen publishes on its own wire and the value it hands a host
/// that mounted it. One derivation, so a section that conforms in its own
/// window and is never asked as a page cannot be two builds wearing one name.
#[must_use]
pub fn conformance() -> DocumentReport {
    spec::document().report_from_paint(VIEW_TAG, &built)
}

/// One topology surface, as the last painted frame has it.
///
/// # Panics
///
/// If asked for a surface the specification does not name, which is a defect in
/// this file rather than a state the screen can reach: the population is the
/// document's own.
#[must_use]
pub fn built(regions: &PaintedRegions, surface: &str) -> Built {
    match surface {
        "filters" => Built::Standing(parts_titled(regions, FILTERS, &title_in("filters"))),
        "graph" => Built::Standing(parts_titled(regions, GRAPH, &title_in("graph"))),
        "inspector" => Built::Standing(parts_titled(regions, INSPECTOR, &title_in("inspector"))),
        other => panic!("no topology surface named {other}"),
    }
}

/// What one part of `surface` is called, in this screen's own words.
///
/// # Panics
///
/// If asked about a surface with no table, which is the same defect as above.
pub fn title_in(surface: &str) -> impl Fn(&str) -> Option<String> + use<> {
    titles_from(match surface {
        "filters" => spec::FILTERS.iter().map(|p| (p.key, p.title)).collect(),
        "graph" => spec::GRAPH.iter().map(|p| (p.key, p.title)).collect(),
        "inspector" => spec::INSPECTOR.iter().map(|p| (p.key, p.title)).collect(),
        other => panic!("no topology surface named {other}"),
    })
}
