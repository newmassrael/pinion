//! ★★★★★ R1747 §5.27 §5.40 §2 #7 — **the capture viewer's verdict about
//! itself, answered from the paint, wherever the screen is running.**
//!
//! # What forced this module, and the sentence it corrects
//!
//! R1738 made an assembled application count the sections it was judged on, and
//! recorded this one as unjudged with the reason *the capture viewer has no
//! written specification at all — not one unpublished, one unwritten*. That
//! sentence was false when it was written and stayed false for nine rounds.
//! Measured 2026-08-20 before this file existed, by listing the crate's modules
//! and grepping the tree for the hook: R1663 wrote screen B's specification as
//! a value in `crate::spec` and `crate::painted` already ran the real `view()`
//! plus `compute_layout()` and compared the painted scene against it in both
//! directions. Both modules predate the entry.
//!
//! What was true is the other half of the same entry. `painted` and `tests` are
//! `#[cfg(test)]` and `PacketView` implemented no
//! [`WidgetView::conformance`](pinion_shell::WidgetView::conformance), so the
//! verdict was computed and stopped at this binary's own test run — the shape
//! the node lab was in before R1742, which is *one unpublished*.
//!
//! # Why this is not `crate::spec`, which already exists
//!
//! Because a screen compared with its own table is a screen agreeing with
//! itself. `crate::spec` is what the RUNNING screen needs — paint tags, row
//! data, the wording of a refusal — and it was written by the same hand in the
//! same edit as the painter it feeds. The comparison it supports is worth what
//! it costs and it is a different claim: that this build is self-consistent,
//! not that it is the reference. So the judgment below is made against
//! `docs/analyzer-packets-spec.json`, extracted from the behaviour reference in
//! neutral vocabulary, loaded with `include_str!` the way its sibling pins are
//! — see [`spec::packets_document`]. (How many siblings there are is a thing to
//! count rather than to write down: `grep -rn 'include_str!(".*docs/analyzer'`.)
//!
//! # Why the paint and not the model
//!
//! Every reading here is of the marks the frame drew. The model would be easier
//! to ask and would be a **second account of the same fact**: a table of
//! intentions passes while the painter draws something else, which is the exact
//! defect this screen's own sweep exists for. Reading the paint also costs
//! nothing extra — the framework already keeps what every surface's last frame
//! painted and, since R1742, what each mark reads.
//!
//! # ★★★★★ What running it measured: a row can light no bytes for TWO reasons
//!
//! Not designed — found, by driving all twenty-one rows the decode tree draws
//! and reading the byte pane after each. **Two** rows hold a value the decoder
//! worked out rather than read, and the tree marks them. **One** more was read
//! from the reassembled payload, which is a second byte source the pane is not
//! showing. The pane says `no bytes here` in both cases; the tree is what
//! tells them apart, so [`selection`] answers `away` with the reason that
//! applies and names which. That the reader's own sentence cannot distinguish
//! them is a defect this round found and did not repair — see
//! `debt-no-bytes-here-names-two-different-facts`.
//!
//! ⚠ The away condition is **the pane's own statement that it is showing none
//! of the open row's bytes**, never *no bytes are lit*. The second would take a
//! highlight scrolled out of the pane and report it as an explanation instead
//! of a defect — the escape hatch R1742's header refuses, arrived at from a
//! different direction.

use pinion_core::conformance::{Built, DocumentReport, Part};
use pinion_core::painted::{PaintedRegions, in_reading_order, painted_regions};
use pinion_core::scene::Rect;

use crate::VIEW_TAG;
use crate::spec;

/// Where the filter bar's parts are addressed.
const FILTER: &str = "pv.filter.";
/// Where the session-context strip's parts are addressed.
const CONTEXT: &str = "pv.context.";
/// Where the message list's column headers are addressed.
const HEADERS: &str = "pv.list.head.";
/// Where a decode row is addressed — a layer by its own path, and the rows
/// under it by a path beneath that, which is what makes the layer headings
/// findable by the shape of a name rather than by a list.
const FIELDS: &str = "pv.tree.field.";
/// Where the reassembly strip's parts are addressed.
const STRIP: &str = "pv.reassembly.";
/// The band the tree draws behind the row a reader has open.
const OPEN_ROW: &str = "pv.tree.selected";
/// The byte pane's readout: which row is open and which bytes it was read from.
const SPAN: &str = "pv.bytes.span";
/// Where one lit byte is addressed.
const LIT: &str = "pv.bytes.lit.";
/// Where the mark saying a row's value was worked out rather than read is.
const DERIVED: &str = "pv.tree.derived.";

/// How much of `docs/analyzer-packets-spec.json` this build is showing.
///
/// The value the screen publishes on its own wire and the value it hands a host
/// that mounted it. One derivation, so a section that conforms in its own
/// window and is never asked as a page cannot be two builds wearing one name.
#[must_use]
pub fn conformance() -> DocumentReport {
    // ★ R1747 obligation-3b — the "it has not painted" answer belongs to the
    // framework, not to this file. R1742 wrote it in the node lab and this file
    // copied it byte for byte, away sentence included; a third screen phrasing
    // it its own way would blur a distinction that is load-bearing (see
    // `report_from_paint`).
    spec::packets_document().report_from_paint(VIEW_TAG, &built)
}

/// One capture-viewer surface, as the last painted frame has it — or the reason
/// it was not on that frame.
///
/// # Panics
///
/// If asked for a surface the specification does not name, which is a defect in
/// this file rather than a state the screen can reach: the population is the
/// document's own.
#[must_use]
pub fn built(regions: &PaintedRegions, surface: &str) -> Built {
    match surface {
        "filter_bar" => Built::Standing(filter_bar(regions)),
        "context" => Built::Standing(read(regions, CONTEXT)),
        "list_columns" => Built::Standing(read(regions, HEADERS)),
        "decode_layers" => Built::Standing(read(regions, FIELDS)),
        "reassembly" => Built::Standing(read(regions, STRIP)),
        "selection" => selection(regions),
        other => panic!("no capture-viewer surface named {other}"),
    }
}

/// The filter bar's three parts.
///
/// ★★★★★ **The query box is not among this surface's marks, and that is a fact
/// about the framework rather than a defect in either.** Measured the first
/// time this file ran against the assembled application: the bar reported the
/// query *absent* and the other two parts *out of place*, which is what a
/// missing leading part does to an ordered roster. The box is an
/// [`External`](pinion_core::external::External) of its own — it owns focus and
/// takes keystrokes — so the paint store files it as a SURFACE, and a surface
/// is not a thing painted inside itself. Its marks are one level down, in its
/// own store.
///
/// So it is found by asking whether **its** surface painted, which is the same
/// store one question lower rather than a different kind of evidence. Worth
/// stating for the next screen that embeds a framework widget and finds a part
/// of itself missing: `parts_under` answers about one surface, and an embedded
/// External is another one.
///
/// ⚠ It is placed FIRST rather than sorted into reading order with its
/// siblings, because the rectangle that would sort it belongs to the other
/// store and this one has no coordinate for it. The bar leads with the query in
/// both documents, so the position is the specification's; what this surface
/// therefore cannot catch is the query box moving to the far end of its own
/// bar, and saying so is cheaper than a reader assuming it could.
fn filter_bar(regions: &PaintedRegions) -> Vec<Part> {
    let mut parts = Vec::new();
    if painted_regions(crate::QUERY_TAG).is_some() {
        parts.push(Part::new(
            "query",
            bar_part_title("query").unwrap_or_default(),
        ));
    }
    parts.extend(named(regions, FILTER, &bar_part_title));
    parts
}

/// A surface whose parts the reference titles by what they READ.
///
/// The word a column header draws, the heading a layer draws, the value a
/// negotiation was settled to. Where the frame drew nothing under a part's own
/// name the reading is said to be missing rather than guessed at, because a
/// part that draws nothing and a part that draws the wrong thing are different
/// defects and a blank would make them read alike.
fn read(regions: &PaintedRegions, stem: &str) -> Vec<Part> {
    in_reading_order(regions.parts_under(stem))
        .into_iter()
        .map(|(key, _)| {
            let said = regions
                .reads(&format!("{stem}{key}"))
                .unwrap_or("<this part is painted and draws no words>");
            Part::new(key, said.to_owned())
        })
        .collect()
}

/// A surface whose parts carry no label a reader sees, titled by this screen's
/// own table.
///
/// The rule the pin states: what a part IS, where what it reads is capture data
/// that moves with the session. Titling those by their reading would make the
/// document a claim about one capture.
fn named(
    regions: &PaintedRegions,
    stem: &str,
    title: &dyn Fn(&str) -> Option<&'static str>,
) -> Vec<Part> {
    in_reading_order(regions.parts_under(stem))
        .into_iter()
        .map(|(key, _)| {
            let said = title(&key).map_or_else(
                || format!("<{key} is painted and no table names it>"),
                str::to_owned,
            );
            Part::new(key, said)
        })
        .collect()
}

/// What one part of the filter bar is, in the words the specification uses.
fn bar_part_title(part: &str) -> Option<&'static str> {
    let said = match part {
        "query" => "the box a query is typed into",
        "saved" => "the queries kept beside it, one press each",
        "count" => "how much of the capture the query kept",
        _ => return None,
    };
    Some(said)
}

/// ★★★★★ The relation this screen exists for: the row the tree draws open, what
/// the byte pane says it was read from, and those bytes drawn lit.
///
/// The three parts are in three regions whose reading order changes with which
/// row is open, so the order below is this file's and the pin says it is not a
/// claim. What IS claimed is that all three are on screen together, and — for
/// the two that can be checked against each other from the paint alone — that
/// they agree. A pane naming one row while the tree has another open, or
/// lighting bytes that are not the ones it named, is the defect this screen was
/// built to make impossible; a surface that only counted three painted parts
/// would pass through both.
fn selection(regions: &PaintedRegions) -> Built {
    let open = open_row(regions);
    let said = regions.reads(SPAN);
    // The pane's own statement, which is the one state this surface is away
    // for — and never "no bytes are lit", which would swallow a highlight
    // scrolled out of the pane.
    if said.is_some_and(|said| said.ends_with(spec::NO_BYTES)) {
        let derived = open
            .as_deref()
            .is_some_and(|path| regions.rect_of(&format!("{DERIVED}{path}")).is_some());
        return Built::away(if derived {
            "the open decode row holds a value the decoder worked out rather than read \
             -- the tree draws its derived mark on it -- so the byte pane shows none of \
             its bytes"
        } else {
            "the open decode row was read from bytes this pane is not showing -- the tree \
             draws no derived mark on it, so the value was read rather than worked out"
        });
    }
    let mut parts = Vec::new();
    if let Some(path) = &open {
        parts.push(Part::new("field", "the decode row the reader has open"));
        if let Some(said) = said {
            parts.push(Part::new(
                "span",
                if said.starts_with(path.as_str()) {
                    "which bytes that row was read from".to_owned()
                } else {
                    format!("<the pane's readout says \"{said}\" and the tree has `{path}` open>")
                },
            ));
        }
    } else if said.is_some() {
        // The pane is speaking about a row the tree is not showing as open.
        // Reported as the span alone rather than as a third away condition: the
        // surface is short a part, which is what it is.
        parts.push(Part::new("span", "which bytes that row was read from"));
    }
    let lit = lit_bytes(regions);
    if !lit.is_empty() {
        parts.push(Part::new(
            "lit",
            match said.and_then(extent) {
                Some((first, last)) if lit == (first..=last).collect::<Vec<_>>() => {
                    "those bytes, drawn lit in the byte pane".to_owned()
                }
                Some((first, last)) => format!(
                    "<the readout names 0x{first:02x}..0x{last:02x} and \
                     {} byte(s) are lit>",
                    lit.len()
                ),
                None => format!(
                    "<{} byte(s) are lit and the readout names no extent>",
                    lit.len()
                ),
            },
        ));
    }
    Built::Standing(parts)
}

/// The decode row the tree is drawing open, read from the band it draws behind
/// it rather than from the model.
///
/// The band carries no name of its own — it is a rectangle — so the row is the
/// one whose own mark lies within it. That is the paint's account of *which row
/// is open*, and it is deliberately the only one this file has: asking the model
/// would agree with the model by construction, and the disagreement worth
/// catching is between the tree and the byte pane.
fn open_row(regions: &PaintedRegions) -> Option<String> {
    let band = regions.rect_of(OPEN_ROW)?;
    regions
        .marks()
        .filter_map(|(tag, rect)| tag.strip_prefix(FIELDS).map(|path| (path, rect)))
        .find(|(_, rect)| within(*rect, band))
        .map(|(path, _)| path.to_owned())
}

/// Whether `inner` sits inside `outer`, vertically. The band spans the pane's
/// width and the row's own mark is indented inside it, so height is what
/// decides which row a band is behind.
fn within(inner: Rect, outer: Rect) -> bool {
    inner.y >= outer.y && inner.y + inner.h <= outer.y + outer.h
}

/// Every byte the pane drew lit, in ascending order.
fn lit_bytes(regions: &PaintedRegions) -> Vec<usize> {
    let mut found: Vec<usize> = regions
        .parts_under(LIT)
        .into_iter()
        .filter_map(|(key, _)| key.parse().ok())
        .collect();
    found.sort_unstable();
    found
}

/// The byte extent the pane's readout names, read back out of the words it drew.
///
/// The screen writes this line and this reads it, which is a round trip through
/// the paint rather than a second copy of the model — the point being that the
/// two accounts of *which bytes* (the sentence and the highlight) are compared
/// with each other rather than both with the map that produced them.
fn extent(said: &str) -> Option<(usize, usize)> {
    let (_, span) = said.rsplit_once(" · ")?;
    let (first, last) = span.split_once("..")?;
    Some((
        usize::from_str_radix(first.strip_prefix("0x")?, 16).ok()?,
        usize::from_str_radix(last.strip_prefix("0x")?, 16).ok()?,
    ))
}
