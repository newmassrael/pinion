//! `scene/size_floor` — the smallest window this screen was **measured** to
//! work in, what forces each axis, and whether the floor it declares agrees.
//!
//! The wire peer of [`pinion_core::size_floor`], which holds the search, and
//! the read peer of [`crate::scroll_reach`], which holds the predicate: a size
//! fits when nothing on the screen is `lost` at it. Content the reader can
//! scroll to is **not** content the window is too small for — that is the whole
//! reason this is judged through reach rather than through what happens to be
//! painted right now, and it is where R1710's reading went wrong.
//!
//! # Why the answer carries its own evidence
//!
//! Each axis reports the marks that go out of reach **one pixel below** the
//! extent it names. That list is not a courtesy: it is what makes the number
//! checkable by whoever reads it, and [`pinion_core::size_floor::Measured`]
//! cannot be built without it. The reference toolkit's `minimumSizeHint()` is a
//! bare size whose cause is unobtainable from outside the widget — measured at
//! 6.11.1, its widget, layout and scroll-area classes carry 139 properties and
//! 73 methods and **none** of them names a reason.
//!
//! # The verdict
//!
//! A screen also *declares* a floor, which since R1710 the framework enforces.
//! Two numbers about the same thing, from different places, is exactly the
//! shape that goes quietly wrong, so this read puts them side by side and says
//! which case a screen is in:
//!
//! | verdict | meaning |
//! |---|---|
//! | `short` | the declared floor is **below** what was measured — a reader can shrink this window until content is unreachable. A defect. |
//! | `exact` | the two agree. |
//! | `roomier` | the declared floor is above what was measured — the window refuses sizes it could take. A decision, and this read is how anyone can tell it was made. |
//! | `undeclared` | the binding declares no floor at all. |
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/size_floor", "id": 1 }
//! ```

use pinion_core::reach::Cut;
use pinion_core::size_floor::{Axis, Floor, PairFit, Refused};
use pinion_core::size_grant::SizeBounds;
use pinion_text::LayoutCache;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;
use crate::containment::RectReport;
use crate::scroll_reach::ViewportReport;

/// One mark a size cannot show whole, however the reader scrolls.
///
/// The wire form of [`pinion_core::reach::Cut`]. Deliberately NOT the row
/// `scene/scroll_reach` answers with, because it answers a different question:
/// that read is about what the reader is not looking at *right now* and carries
/// an offset that would fix it, while this one is about what the *size* can
/// never show and carries how far past the reachable box the mark reaches. A
/// shared row would have had two fields that are null on one side each, which
/// is how one shape comes to mean two things.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CutReport {
    /// The mark's own tag, when it has one.
    pub tag: Option<String>,
    /// The mark's address, `/`-joined the way `scene/locate` reports it.
    pub path: String,
    /// What a text run holds: the string a reader cannot see all of.
    pub content: Option<String>,
    /// Where the mark sits in its viewport's content coordinates.
    pub rect: RectReport,
    /// The viewport it was judged against — the window, or the pane whose
    /// scroll range still does not cover it.
    pub viewport: ViewportReport,
    /// How far past everything that viewport can ever show it reaches, in
    /// `left, top, right, bottom` order. Never all zero.
    pub short_by: [u32; 4],
}

/// Turn the core rows into their wire form.
#[must_use]
pub fn cut_rows(cuts: &[Cut]) -> Vec<CutReport> {
    cuts.iter()
        .map(|c| CutReport {
            tag: c.tag.clone(),
            path: c.path.join("/"),
            content: c.content.clone(),
            rect: c.rect.into(),
            viewport: ViewportReport {
                name: c.viewport.name.clone(),
                origin_x: c.viewport.origin.0,
                origin_y: c.viewport.origin.1,
                w: c.viewport.size.0,
                h: c.viewport.size.1,
                content_w: c.viewport.content.0,
                content_h: c.viewport.content.1,
                at_x: c.viewport.at.0,
                at_y: c.viewport.at.1,
                max_x: c.viewport.max.0,
                max_y: c.viewport.max.1,
                fits: c.viewport.fits(),
            },
            short_by: [
                c.short_by.left,
                c.short_by.top,
                c.short_by.right,
                c.short_by.bottom,
            ],
        })
        .collect()
}

/// Measure one candidate size: everything the screen could not show whole
/// there.
///
/// The probe [`pinion_core::size_floor::measure`] drives, spelled here so the
/// embedder hands over a scene and gets the answer rather than assembling the
/// predicate itself — the same division `scroll_reach::collect` has, and for
/// the same reason: a second assembly is a second opinion.
#[must_use]
pub fn cut_at(scene: &pinion_core::scene::Scene, cache: &mut LayoutCache) -> Vec<CutReport> {
    let root = scene.rect();
    let cuts = pinion_core::reach::cut(scene, (root.w, root.h), &mut |t| {
        let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
        cache.ink_size(&t.content, &t.style, &t.runs, max_width)
    });
    cut_rows(&cuts)
}

/// One axis' measured boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxisReport {
    /// The extent that fits.
    pub extent: u32,
    /// The extent that does not — always one less, named rather than left for
    /// a caller to compute so the two ends of the boundary read as one fact.
    pub short_extent: u32,
    /// How many probes this axis cost. Each is a full view and layout, so a
    /// caller choosing when to ask can see what it is buying.
    pub probes: usize,
    /// What [`Self::short_extent`] puts out of the reader's reach. Never empty:
    /// it is the evidence for [`Self::extent`], in the same rows
    /// `scene/scroll_reach` answers with.
    pub forced_by: Vec<CutReport>,
}

/// The size a floor is expressed in, on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SizeReport {
    /// The horizontal extent.
    pub width: u32,
    /// The vertical extent.
    pub height: u32,
}

impl From<(u32, u32)> for SizeReport {
    fn from((width, height): (u32, u32)) -> Self {
        Self { width, height }
    }
}

/// Whether the two per-axis answers are a size in their own right.
///
/// ★★★★★ The field that stops the most likely misreading of this whole answer.
/// Each axis is measured with the other at the ceiling — which is what a window
/// minimum *is* — so their pair is a separate question, and on two of the three
/// analysis-tool screens the pair does **not** fit: a window below the floor on
/// one axis stops using the live extent on the other, so narrowing it also
/// un-shortens it and cuts the bottom off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairReport {
    /// `fits` or `loses`.
    pub verdict: &'static str,
    /// What is out of reach at the pair. Empty for `fits`.
    pub out_of_reach: Vec<CutReport>,
}

/// Why no floor could be measured. An answer about the screen, not a malformed
/// request — which is why it rides in the result rather than in an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefusedReport {
    /// Which axis was being measured.
    pub axis: &'static str,
    /// `ceiling_is_short` (the largest size does not fit either, so there is no
    /// floor to find and the screen needs repairing) or `nothing_is_ever_lost`
    /// (the probe reports nothing at any size, so a number here would have no
    /// evidence under it).
    pub reason: &'static str,
    /// For `ceiling_is_short`: what is out of reach even there.
    pub out_of_reach: Vec<CutReport>,
}

/// What the binding declares, for the verdict to be read against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DeclaredReport {
    /// The declared floor, or `null` where the binding declares none.
    pub floor: Option<SizeReport>,
    /// The declared ceiling, likewise.
    pub ceiling: Option<SizeReport>,
}

/// The `scene/size_floor` result.
///
/// `needed` and `refused` are mutually exclusive: exactly one is present, so a
/// caller cannot read a number that was never measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SizeFloorOutcome {
    /// The measured floor, absent when the search was refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needed: Option<SizeReport>,
    /// The horizontal boundary, absent when the search was refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<AxisReport>,
    /// The vertical boundary, likewise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<AxisReport>,
    /// Whether `needed` is a size the screen actually works at. See
    /// [`PairReport`] — this is not a formality, and two of three screens
    /// measured here answer `loses`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pair: Option<PairReport>,
    /// Why there is no answer, absent when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<RefusedReport>,
    /// The largest size the search asked about — the size a screen is expected
    /// to fit in, and the one the refusal `ceiling_is_short` is about.
    pub ceiling: SizeReport,
    /// What the binding declares for this window.
    pub declared: DeclaredReport,
    /// `short` / `exact` / `roomier` / `undeclared`, or `unmeasured` when the
    /// search was refused. See the module header for what each means.
    pub verdict: &'static str,
    /// What the whole search cost.
    pub probes: usize,
}

/// Typed errors [`handle_scene_size_floor`] can return.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeFloorError {
    /// The embedder installed no outcome — this host cannot lay the screen out
    /// at a size it is not at, which is a different statement from "the screen
    /// has no floor".
    SizeFloorUnavailable,
}

impl SizeFloorError {
    /// The word that rides in `error.data`.
    #[must_use]
    pub const fn wire_tag(&self) -> &'static str {
        match self {
            Self::SizeFloorUnavailable => "SizeFloorUnavailable",
        }
    }
}

/// The verdict for one axis: how the declared floor stands to the measured one.
fn axis_verdict(declared: Option<u32>, needed: u32) -> &'static str {
    match declared {
        None => "undeclared",
        Some(at) if at < needed => "short",
        Some(at) if at > needed => "roomier",
        Some(_) => "exact",
    }
}

/// The screen's verdict: the worst of its two axes.
///
/// Ordered by how much a reader should care — a window that can be shrunk past
/// what it can show is a defect, an undeclared floor is unknown territory, and
/// a roomier one is a decision somebody made. Folding to the worst is what
/// stops one honest axis from hiding the other.
#[must_use]
pub fn verdict(declared: SizeBounds, needed: (u32, u32)) -> &'static str {
    let floor = declared.floor();
    let width = axis_verdict(floor.map(|f| f.0), needed.0);
    let height = axis_verdict(floor.map(|f| f.1), needed.1);
    for rank in ["short", "undeclared", "roomier"] {
        if width == rank || height == rank {
            return rank;
        }
    }
    "exact"
}

/// Turn a search result into its wire form.
///
/// `into_rows` maps whatever the probe reported into the rows
/// `scene/scroll_reach` publishes, so the evidence a caller reads here is the
/// same shape it reads there.
#[must_use]
pub fn report<T>(
    result: &Result<Floor<T>, Refused<T>>,
    ceiling: (u32, u32),
    declared: SizeBounds,
    into_rows: &dyn Fn(&[T]) -> Vec<CutReport>,
) -> SizeFloorOutcome {
    let declared_report = DeclaredReport {
        floor: declared.floor().map(SizeReport::from),
        ceiling: declared.ceiling().map(SizeReport::from),
    };
    match result {
        Ok(floor) => {
            let needed = floor.extent();
            SizeFloorOutcome {
                needed: Some(needed.into()),
                width: Some(AxisReport {
                    extent: floor.width.extent(),
                    short_extent: floor.width.short_extent(),
                    probes: floor.width.probes(),
                    forced_by: into_rows(floor.width.forced_by()),
                }),
                height: Some(AxisReport {
                    extent: floor.height.extent(),
                    short_extent: floor.height.short_extent(),
                    probes: floor.height.probes(),
                    forced_by: into_rows(floor.height.forced_by()),
                }),
                pair: Some(PairReport {
                    verdict: floor.pair.wire_word(),
                    out_of_reach: match &floor.pair {
                        PairFit::Fits => Vec::new(),
                        PairFit::Loses(marks) => into_rows(marks),
                    },
                }),
                refused: None,
                ceiling: ceiling.into(),
                declared: declared_report,
                verdict: verdict(declared, needed),
                probes: floor.probes(),
            }
        }
        Err(refused) => SizeFloorOutcome {
            needed: None,
            width: None,
            height: None,
            pair: None,
            refused: Some(RefusedReport {
                axis: refused.axis().wire_word(),
                reason: refused.wire_word(),
                out_of_reach: match refused {
                    Refused::CeilingIsShort { out_of_reach, .. } => into_rows(out_of_reach),
                    Refused::NothingIsEverLost { .. } => Vec::new(),
                },
            }),
            ceiling: ceiling.into(),
            declared: declared_report,
            verdict: "unmeasured",
            probes: 0,
        },
    }
}

/// The axis words, for a caller that wants to check its own table against ours.
#[must_use]
pub const fn axis_words() -> [&'static str; 2] {
    [Axis::Width.wire_word(), Axis::Height.wire_word()]
}

/// `scene/size_floor` dispatcher entry.
///
/// # Errors
///
/// [`SizeFloorError::SizeFloorUnavailable`] when the embedder installed no
/// outcome.
pub fn handle_scene_size_floor(outcome: Option<&SizeFloorOutcome>) -> Result<Value, RpcError> {
    let Some(outcome) = outcome else {
        return Err(RpcError::invalid_params(
            SizeFloorError::SizeFloorUnavailable.wire_tag(),
        ));
    };
    serde_json::to_value(outcome).map_err(|err| RpcError::internal_error(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::size_floor::{Measured, measure};

    fn rows(items: &[&'static str]) -> Vec<CutReport> {
        items
            .iter()
            .map(|name| CutReport {
                tag: Some((*name).to_string()),
                path: String::new(),
                content: None,
                rect: RectReport {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                },
                viewport: ViewportReport {
                    name: "<window>".to_string(),
                    origin_x: 0,
                    origin_y: 0,
                    w: 0,
                    h: 0,
                    content_w: 0,
                    content_h: 0,
                    at_x: 0,
                    at_y: 0,
                    max_x: 0,
                    max_y: 0,
                    fits: true,
                },
                short_by: [0, 0, 1, 0],
            })
            .collect()
    }

    fn needs(need: (u32, u32)) -> impl FnMut(u32, u32) -> Vec<&'static str> {
        move |w, h| {
            let mut out = Vec::new();
            if w < need.0 {
                out.push("narrow");
            }
            if h < need.1 {
                out.push("short");
            }
            out
        }
    }

    #[test]
    fn r1711_a_declared_floor_below_the_measured_one_is_short() {
        let result = measure((1600, 900), needs((1200, 500)));
        let declared = SizeBounds::floored((1200, 400));
        let out = report(&result, (1600, 900), declared, &|t| rows(t));
        assert_eq!(out.verdict, "short");
        assert_eq!(out.needed, Some(SizeReport::from((1200, 500))));
        assert_eq!(out.declared.floor, Some(SizeReport::from((1200, 400))));
    }

    #[test]
    fn r1711_agreement_is_exact_and_a_larger_declaration_is_roomier() {
        let result = measure((1600, 900), needs((1200, 500)));
        let exact = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1200, 500)),
            &|t| rows(t),
        );
        assert_eq!(exact.verdict, "exact");
        let roomier = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1400, 600)),
            &|t| rows(t),
        );
        assert_eq!(roomier.verdict, "roomier");
        let none = report(&result, (1600, 900), SizeBounds::UNBOUNDED, &|t| rows(t));
        assert_eq!(none.verdict, "undeclared");
        assert_eq!(none.declared.floor, None);
    }

    #[test]
    fn r1711_one_short_axis_is_not_hidden_by_an_honest_one() {
        let result = measure((1600, 900), needs((1200, 500)));
        // Width agrees exactly; height is declared 100 pixels below what was
        // measured. A fold that took the FIRST axis, or an average, would
        // publish "exact" for a window a reader can break.
        let out = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1200, 400)),
            &|t| rows(t),
        );
        assert_eq!(out.verdict, "short");
    }

    #[test]
    fn r1711_the_evidence_rides_with_the_number() {
        let result = measure((1600, 900), needs((1200, 500)));
        let out = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1200, 500)),
            &|t| rows(t),
        );
        let width = out.width.expect("a measured width");
        assert_eq!(width.extent, 1200);
        assert_eq!(width.short_extent, 1199);
        assert_eq!(
            width
                .forced_by
                .iter()
                .map(|r| r.tag.clone())
                .collect::<Vec<_>>(),
            [Some("narrow".to_string())]
        );
        assert!(width.probes > 0);
    }

    #[test]
    fn r1711_the_pair_rides_on_the_wire_with_what_it_loses() {
        // The analysis tool's shape (see the core module's test of the same
        // name): narrowing past the floor un-shortens the layout.
        let result = measure((1600, 900), |w, h| {
            let laid_out = if w >= 1600 && h >= 340 {
                (w, h)
            } else {
                (1600, 900)
            };
            let mut out = Vec::new();
            if laid_out.0 - 119 > w {
                out.push("right");
            }
            if laid_out.1 > h {
                out.push("bottom");
            }
            out
        });
        let out = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1481, 340)),
            &|t| rows(t),
        );
        let pair = out.pair.expect("a measured pair");
        assert_eq!(pair.verdict, "loses");
        assert_eq!(
            pair.out_of_reach
                .iter()
                .map(|r| r.tag.clone())
                .collect::<Vec<_>>(),
            [Some("bottom".to_string())]
        );
        // And the axes still agree with the declaration — which is exactly why
        // a verdict alone would have read as a clean bill of health.
        assert_eq!(out.verdict, "exact");
    }

    #[test]
    fn r1711_a_refusal_carries_no_number_at_all() {
        let result = measure((800, 600), needs((1200, 500)));
        let out = report(&result, (800, 600), SizeBounds::floored((400, 300)), &|t| {
            rows(t)
        });
        assert_eq!(out.verdict, "unmeasured");
        assert_eq!(out.needed, None);
        assert_eq!(out.width, None);
        let refused = out.refused.expect("a refusal");
        assert_eq!(refused.axis, "width");
        assert_eq!(refused.reason, "ceiling_is_short");
        assert_eq!(refused.out_of_reach.len(), 1);
        // And the declared floor is still published: the refusal is about the
        // screen, so what it declared is exactly what a reader wants to see.
        assert_eq!(out.declared.floor, Some(SizeReport::from((400, 300))));
    }

    #[test]
    fn r1711_an_absent_outcome_is_a_different_answer_from_an_empty_one() {
        let err = handle_scene_size_floor(None).expect_err("no outcome installed");
        assert!(format!("{err:?}").contains("SizeFloorUnavailable"));
    }

    #[test]
    fn r1711_the_axis_words_are_the_cores() {
        assert_eq!(axis_words(), ["width", "height"]);
        assert!(Measured::<&str>::new(10, Vec::new(), 1).is_none());
    }
}
