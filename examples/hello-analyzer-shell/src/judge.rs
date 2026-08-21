//! ★★★★★ R1761 §5.27 §5.40 §2 #7 — **the dashboard's verdict about itself,
//! answered from the paint, by the host that draws it.**
//!
//! # What forced this module, and the instruction it corrects
//!
//! R1738 counted the sections of the assembled application that had been judged
//! and recorded this one as unjudged, with the route out of it written into the
//! framework's own type: *closing it means giving that page a `Screen` of its
//! own, which is what the trait being public is for*. The entry sat for
//! twenty-three rounds and nobody did it, which is the first thing worth
//! noticing about an instruction.
//!
//! Measured before this module existed, standing on this page and asking the
//! running application where its own rectangles are:
//!
//! ```text
//! shell.canvas    1096x802 at (52, 98)     <- the page region a screen gets
//! shell.subbar    1096x46  at (52, 52)     <- the section's layout bar, ABOVE it
//! shell.palette    292x848 at (1148, 52)   <- the section's palette, BESIDE it
//! ```
//!
//! A screen judges what it paints, and a host paints a section's chrome outside
//! the page region *because that is what chrome is*. The recorded route would
//! have produced a verdict blind to about a quarter of its own section — and
//! blind to the two surfaces this screen is most distinctive for, since the
//! palette is where the reference makes its whole argument about scope. So the
//! framework grew the smaller thing instead: [`SectionJudge`], registered per
//! destination, answering one question and granting nothing else.
//!
//! # Why the paint and not the model
//!
//! Every reading here is of the marks the frame drew. The model would be easier
//! to ask and would be a **second account of the same fact**: this screen's
//! tables and this screen's painter were written in the same edit, so a
//! comparison between them cannot fail for the reason comparisons exist. Two of
//! this tree's sections were caught doing exactly that at R1758.
//!
//! # Which surfaces are titled by their words, and which by this screen's
//!
//! Where the reference fixes the WORDS — the panel's own heading, a group's
//! heading — the parts are titled by
//! [`parts_as_read`], so a painter that labelled a group anything at all leaves
//! a difference the pin refuses. Where a part is a value or a control that
//! relabels itself while a reader works (the layout in effect, how many widgets
//! are placed, the verb that reads *Done* while editing), the parts are titled
//! by this screen's tables and what the pin fixes is the roster and its order.
//! Pinning painted words there would report a working screen as wrong the
//! moment somebody used it.
//!
//! # ★★★★★ What running it measured: two parts a reader sees had no address
//!
//! Not designed — found, by writing the specification down and asking the paint
//! for it. The layout bar draws four things and **three** of them were tagged:
//! the placed count was loose ink, so the one part of that bar which is not a
//! control was the one no specification could reach. The palette's own heading
//! and its one-line hint were loose ink for the same reason. Both are addressed
//! now, which is what makes `layout_bar` and `palette_head` checkable at all.
//!
//! ⚠ The away condition here is **the host's own answer to where the reader
//! is** ([`Showing`]), never *I found none of my parts*. The second would be an
//! escape hatch wide enough to swallow the mechanism: a page that stopped
//! painting half of itself would report exactly what a page nobody is looking
//! at reports.

use pinion_core::conformance::{
    Built, DocumentReport, Part, parts_as_read, parts_titled, titles_from,
};
use pinion_core::painted::PaintedRegions;
use pinion_screen::{SectionJudge, Showing};

use crate::{PALETTE_HEAD, VIEW_TAG, spec};

/// Where the layout bar's parts are addressed.
const LAYOUT_BAR: &str = "shell.subbar.";
/// Where the palette's group headings are addressed.
const PALETTE_GROUPS: &str = "shell.palette.section.";
/// Where the palette's entries and its two counts are addressed.
///
/// The stem the entries share with nothing else: a heading is addressed one
/// level deeper (`section.`), and a row's own parts one level deeper again
/// (`part.`), so both are excluded by the shape of their names rather than by a
/// list this file would have to keep.
const PALETTE: &str = "shell.palette.";
/// Where a placed card is addressed.
const BOARD: &str = "card.";

/// What answers for the dashboard, which is a page this shell paints itself.
pub struct BoardJudge;

impl SectionJudge for BoardJudge {
    fn conformance(&self, showing: Showing) -> DocumentReport {
        spec::dashboard_document().report_from_paint(VIEW_TAG, &|regions, surface| {
            built(regions, surface, showing)
        })
    }
}

/// One dashboard surface, as the last painted frame has it — or the reason it
/// was not on that frame.
///
/// # Panics
///
/// If asked for a surface the specification does not name, which is a defect in
/// this file rather than a state the screen can reach: the population is the
/// document's own.
#[must_use]
pub fn built(regions: &PaintedRegions, surface: &str, showing: Showing) -> Built {
    if !showing.is_on_screen() {
        // One sentence for every surface of this page, because there is one
        // reason: the reader is elsewhere and this page painted no part of the
        // last frame. The host's store is not empty — it is full of the page
        // that IS showing — which is exactly why the framework's own
        // "has not painted" answer cannot cover this.
        return Built::away(
            "the reader is on another section, so this page painted no part of the last frame",
        );
    }
    match surface {
        "layout_bar" => Built::Standing(parts_titled(regions, LAYOUT_BAR, &title_in("layout_bar"))),
        // ★ Read, not titled — see the module header.
        "palette_head" => Built::Standing(parts_as_read(regions, PALETTE_HEAD)),
        "palette_groups" => Built::Standing(parts_as_read(regions, PALETTE_GROUPS)),
        "palette" => Built::Standing(parts_titled(regions, PALETTE, &title_in("palette"))),
        "board" => Built::Standing(board(regions)),
        other => panic!("no dashboard surface named {other}"),
    }
}

/// The cards the board is holding, titled by the kind each was placed from.
///
/// A card's address is its kind and the ordinal it was placed under, so the
/// title is looked up by the kind alone — a second card of one kind is a part
/// the specification does not declare, which is what it should read as.
fn board(regions: &PaintedRegions) -> Vec<Part> {
    parts_titled(regions, BOARD, &|key| {
        let kind = key.split('#').next().unwrap_or(key);
        spec::CATALOGUE
            .iter()
            .find(|entry| entry.kind == kind)
            .map(|entry| entry.label.to_owned())
    })
}

/// What one part of `surface` is called, in this screen's own words.
///
/// # Panics
///
/// If asked about a surface with no table, which is the same defect as above.
fn title_in(surface: &str) -> impl Fn(&str) -> Option<String> + use<> {
    titles_from(match surface {
        "layout_bar" => spec::LAYOUT_BAR.to_vec(),
        "palette" => spec::CATALOGUE
            .iter()
            .map(|entry| (entry.kind, entry.label))
            .chain(spec::PALETTE_FOOT.iter().copied())
            .collect(),
        other => panic!("no dashboard surface named {other}"),
    })
}
