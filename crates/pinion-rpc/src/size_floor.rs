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
//! | `short` | the declared floor is **below** what was measured, and nothing accounts for the difference — a reader can shrink this window until content is sliced. A defect. |
//! | `conceded` | ★ R1712 — the same relation, **declared**: the binding carries a [`ShrinkPolicy`] naming what the band gives up, and the screen honours it. A decision. |
//! | `exact` | the two agree. |
//! | `roomier` | the declared floor is above what was measured — the window refuses sizes it could take. A decision, and this read is how anyone can tell it was made. |
//! | `undeclared` | the binding declares no floor at all. |
//!
//! # The concession
//!
//! R1712 — a screen has two minimums (the size its layout stops reflowing at,
//! and the size its window stops shrinking at) and every binding in this tree
//! spelled them as one number, which meant the decision *"let the reader make
//! this window smaller than it lays out at, and here is what that costs"* could
//! not be written down. `concession` is that decision, put beside what the
//! screen actually does at the floor it names: what is clipped there, what the
//! declaration covered, and — the part a concession can never excuse — whether
//! anything is out of reach altogether.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/size_floor", "id": 1 }
//! ```

use pinion_core::reach::Cut;
use pinion_core::shrink::ShrinkPolicy;
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
                declared: c.viewport.declared.into(),
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
    let cuts = pinion_core::reach::cut(scene, (root.w, root.h), &mut crate::ink_of(cache));
    cut_rows(&cuts)
}

/// R1712 — check a binding's declared concession against the screen at the
/// floor that concession names.
///
/// `scene` must be the mirror paint laid out at [`ShrinkPolicy::floor`]; the
/// two predicates are read here rather than by the embedder for the reason
/// [`cut_at`] states, and this one needs **both** — what the size cannot show
/// whole, and what it puts out of reach altogether. A concession is allowed to
/// clip and is never allowed to lose.
///
/// `declared` is what the window system was actually told, so the report can
/// say whether the binding built its `SizeStrategy` from this same policy.
#[must_use]
pub fn audit_at(
    scene: &pinion_core::scene::Scene,
    cache: &mut LayoutCache,
    policy: ShrinkPolicy,
    declared: SizeBounds,
) -> ConcessionReport {
    let root = scene.rect();
    let cuts = pinion_core::reach::cut(scene, (root.w, root.h), &mut crate::ink_of(cache));
    let sightings =
        pinion_core::reach::out_of_sight(scene, (root.w, root.h), &mut crate::ink_of(cache));
    let audit = pinion_core::shrink::audit(policy, &cuts, &sightings);
    ConcessionReport {
        comfortable: policy.comfortable().into(),
        floor: policy.floor().into(),
        band: policy.band().into(),
        recourse: policy.recourse().wire_word(),
        gives_up: policy
            .gives_up()
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        cut_at_floor: cut_rows(&cuts),
        covered: audit.covered(),
        unreachable: audit.unreachable().to_vec(),
        unnamed: audit.unnamed().to_vec(),
        stale: audit.stale().to_vec(),
        declaration_split: declared.floor() != Some(policy.floor()),
        verdict: audit.wire_word(),
    }
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
    /// Which axis this is about, or `null` for `ceiling_is_short`.
    ///
    /// ★ R1711.1 — nullable because a size that does not fit has no axis to
    /// name: the evidence says which way it is short, and it can be short in
    /// both. Before this it always carried one, and named the axis the search
    /// had reached — `width` for a ceiling one pixel short in height.
    pub axis: Option<&'static str>,
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

/// R1712 — what the binding decided to give up to let its window get smaller
/// than the size it lays out at, and whether the screen honours that.
///
/// Absent entirely on a binding that declares no
/// [`ShrinkPolicy`] — which is a different
/// statement from a policy conceding nothing, and the difference is the whole
/// reason `undeclared` exists as a verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConcessionReport {
    /// The size below which the layout stops reflowing and the window clips.
    pub comfortable: SizeReport,
    /// The size below which the window refuses to shrink.
    pub floor: SizeReport,
    /// How much smaller than its layout minimum the window may go, per axis.
    /// `0` on an axis that concedes nothing.
    pub band: SizeReport,
    /// ★★★★★ (R1714) How the band is served: `clip` — the window cuts and what
    /// it cuts is gone — or `pan` — the window becomes a viewport onto the
    /// layout and everything stays one gesture away.
    ///
    /// The word a reader needs before any other field here means anything:
    /// [`Self::gives_up`] is a list of losses under one and empty by
    /// construction under the other.
    pub recourse: &'static str,
    /// The regions the binding declares the band clips, by the name a reader
    /// addresses them with. Empty exactly when there is no band **that clips** —
    /// a pan gives nothing up, so it names nothing.
    pub gives_up: Vec<String>,
    /// What the floor actually cuts — measured, in the same rows every other
    /// list here uses.
    pub cut_at_floor: Vec<CutReport>,
    /// How many of those rows a declared name accounted for. Published because
    /// a declaration made of region names covers many marks with few words, and
    /// a reader judging whether that is too coarse needs the number it bought.
    pub covered: usize,
    /// ★ Marks nothing can bring into view at the floor. A concession may clip;
    /// it may never make something unreachable, so this list being non-empty is
    /// a broken floor rather than a stale list.
    pub unreachable: Vec<String>,
    /// Cut at the floor and covered by no declared name — the screen giving up
    /// more than it admits to.
    pub unnamed: Vec<String>,
    /// Declared and covering nothing — a declaration that outlived its screen.
    pub stale: Vec<String>,
    /// ★ Whether the floor this policy declares is the floor the window system
    /// was actually told. `true` is a binding that wrote its minimum twice and
    /// they disagree — the drift [`ShrinkPolicy`]
    /// exists to make unrepresentable, reported because a binding can still
    /// reach around the type by spelling `SizeStrategy` by hand.
    pub declaration_split: bool,
    /// `honoured` / `stale` / `surprised` / `unreachable`, worst first. See
    /// [`pinion_core::shrink::Audit::wire_word`].
    pub verdict: &'static str,
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
    /// R1712 — the concession the binding declared, absent when it declares
    /// none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concession: Option<ConcessionReport>,
    /// `short` / `exact` / `roomier` / `conceded` / `undeclared`, or
    /// `unmeasured` when the search was refused. See the module header for what
    /// each means.
    pub verdict: &'static str,
    /// What the whole search cost, in full mirror paints.
    ///
    /// ★ R1712 — the search's own probes. A binding that declares a policy
    /// costs **one more** than this, for the audit paint at its floor, and that
    /// one is not counted here because it is not part of the search: a caller
    /// comparing this number across screens is comparing searches.
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
///
/// ★ R1712 — `short` and `conceded` are the same arithmetic and different
/// facts. A floor below what the screen needs whole is a defect **when nothing
/// accounts for it**; when the binding declared a
/// [`ShrinkPolicy`] and the screen honours
/// it, the same relation is a decision, and this is where the two stop reading
/// alike. Before R1712 there was no way to declare it, so `short` was the only
/// available word and the node lab's 1625-pixel floor had to stay where a
/// 1600-pixel display could not open it.
#[must_use]
pub fn verdict(
    declared: SizeBounds,
    needed: (u32, u32),
    concession: Option<&ConcessionReport>,
) -> &'static str {
    let floor = declared.floor();
    let width = axis_verdict(floor.map(|f| f.0), needed.0);
    let height = axis_verdict(floor.map(|f| f.1), needed.1);
    let honoured = concession.filter(|c| c.verdict == "honoured" && !c.declaration_split);
    // ★★★★★ R1714 — a window that PANS gets its own word, and it is read before
    // the arithmetic rather than inside one of its branches.
    //
    // The measurement is why. A panning screen puts nothing beyond reach at any
    // size, so `needed` bottoms out at one pixel and the arithmetic reads
    // `roomier` — "the floor is above what the screen needs, so somebody decided
    // it". True, and it is the wrong headline: `roomier` is also what a clipping
    // screen reads when its floor is generous, and the two are not the same
    // fact. `panned` says which decision it was, and the evidence for it —
    // `needed`, and a `concession` whose `unreachable` list is empty — rides
    // alongside.
    if honoured.is_some_and(|c| c.recourse == "pan") {
        return "panned";
    }
    for rank in ["short", "undeclared", "roomier"] {
        if width == rank || height == rank {
            // ★ R1712 — `short` and `conceded` are the same arithmetic and
            // different facts; the declaration is what parts them.
            if rank == "short" && honoured.is_some() {
                return "conceded";
            }
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
    concession: Option<ConcessionReport>,
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
                verdict: verdict(declared, needed, concession.as_ref()),
                concession,
                probes: floor.probes(),
            }
        }
        Err(refused) => SizeFloorOutcome {
            needed: None,
            width: None,
            height: None,
            pair: None,
            refused: Some(RefusedReport {
                axis: refused.axis().map(Axis::wire_word),
                reason: refused.wire_word(),
                out_of_reach: match refused {
                    Refused::CeilingIsShort { out_of_reach, .. } => into_rows(out_of_reach),
                    Refused::NothingIsEverLost { .. } => Vec::new(),
                },
            }),
            ceiling: ceiling.into(),
            declared: declared_report,
            concession,
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
    use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
    use pinion_core::size_floor::{Measured, measure};
    use pinion_text::LayoutCache;

    /// The analysis tool's own shape at the node lab's conceded floor, small
    /// enough to read: a 1625-wide bar in a 1506-wide window, with a chip whose
    /// tail and a pane whose right third are past the edge.
    ///
    /// ★ R1712.1 — this fixture exists because a counterfactual PASSED.
    /// [`audit_at`] assembles the entire concession report and had no test at
    /// this layer at all: the tests below build a `ConcessionReport` by hand, so
    /// inverting `declaration_split` inside `audit_at` left every one of them
    /// green. The gate was living a layer above the defect, in the integration
    /// demo — the shape R1710 recorded and this round repeated.
    fn screen() -> Scene {
        // The root is the SURFACE and carries no tag, exactly as the real
        // screens are shaped: a mark's path runs through the panes above it, so
        // `lab.appbar` is what covers the chip and the root covers nothing.
        let mut bar = ContainerNode::new(vec![
            Scene::Text(
                TextNode::new("state", Rect::new(1505, 10, 100, 12)).with_tag("lab.appbar.state"),
            ),
            Scene::Text(
                TextNode::new("ok", Rect::new(10, 10, 16, 12)).with_tag("lab.appbar.title"),
            ),
        ]);
        bar.rect = Rect::new(0, 0, 1625, 40);
        bar.tag = Some("lab.appbar".into());
        let mut pane = ContainerNode::new(Vec::new());
        pane.rect = Rect::new(1313, 40, 312, 60);
        pane.tag = Some("lab.inspector".into());
        let mut root = ContainerNode::new(vec![Scene::Container(bar), Scene::Container(pane)]);
        root.rect = Rect::new(0, 0, 1506, 100);
        Scene::Container(root)
    }

    fn audit_screen(policy: ShrinkPolicy, declared: SizeBounds) -> ConcessionReport {
        audit_at(&screen(), &mut LayoutCache::new(), policy, declared)
    }

    /// The honest case, end to end through the real predicates — and the
    /// declaration is the node lab's own, so this fixture is that screen shrunk
    /// to something a reader can hold in their head.
    #[test]
    fn r1712_a_declaration_that_names_what_the_floor_clips_is_honoured() {
        let policy =
            ShrinkPolicy::conceding((1625, 100), (1506, 100), &["lab.appbar", "lab.inspector"]);
        let report = audit_screen(policy, SizeBounds::floored((1506, 100)));
        assert_eq!(report.verdict, "honoured");
        assert_eq!(report.band, SizeReport::from((119, 0)));
        assert!(
            report.unreachable.is_empty(),
            "nothing is out of reach here"
        );
        assert!(report.unnamed.is_empty(), "every clipped mark is declared");
        // ★ Two names, THREE marks — the app bar is clipped, so is the chip
        // inside it, and the pane is the third. That third one is the ancestry
        // rule doing its job, and the number is what publishes it.
        assert_eq!(report.covered, 3);
    }

    /// The partial case, kept separate: naming only the pane leaves the app bar
    /// and its chip undeclared, and the report says WHICH by name.
    #[test]
    fn r1712_a_declaration_that_names_half_of_it_says_which_half() {
        let policy = ShrinkPolicy::conceding((1625, 100), (1506, 100), &["lab.inspector"]);
        let report = audit_screen(policy, SizeBounds::floored((1506, 100)));
        assert_eq!(report.verdict, "surprised");
        assert_eq!(report.unnamed, ["lab.appbar", "lab.appbar.state"]);
        assert_eq!(report.covered, 1);
        assert!(
            report.stale.is_empty(),
            "the pane it named really is clipped"
        );
    }

    /// ★★★★★ The counterfactual's own case: the two declared floors are
    /// COMPARED, and a binding that told the window system a different number
    /// reads `declaration_split`. Nothing at this layer checked it before.
    #[test]
    fn r1712_a_floor_the_window_system_was_not_told_reads_as_split() {
        let policy = ShrinkPolicy::conceding((1625, 100), (1506, 100), &["lab.inspector"]);
        let agreed = audit_screen(policy, SizeBounds::floored((1506, 100)));
        assert!(
            !agreed.declaration_split,
            "the window system was told exactly what the policy declares"
        );
        let split = audit_screen(policy, SizeBounds::floored((1625, 100)));
        assert!(
            split.declaration_split,
            "a binding that wrote its minimum twice and disagreed says so"
        );
        let undeclared = audit_screen(policy, SizeBounds::UNBOUNDED);
        assert!(
            undeclared.declaration_split,
            "and declaring no floor at all is not agreement either"
        );
    }

    /// The report carries the marks themselves, in the same rows every other
    /// list here uses, so a caller reading `unnamed` can find out how far past
    /// the window each one reaches.
    #[test]
    fn r1712_the_concession_carries_the_marks_it_is_about() {
        let policy = ShrinkPolicy::conceding((1625, 100), (1506, 100), &["lab.inspector"]);
        let report = audit_screen(policy, SizeBounds::floored((1506, 100)));
        let mut tags: Vec<_> = report
            .cut_at_floor
            .iter()
            .filter_map(|row| row.tag.clone())
            .collect();
        tags.sort();
        assert_eq!(tags, ["lab.appbar", "lab.appbar.state", "lab.inspector"]);
        for row in &report.cut_at_floor {
            assert!(
                row.short_by.iter().any(|edge| *edge > 0),
                "a row that overhangs nothing is not a cut"
            );
        }
    }

    /// A rigid policy over the same screen names everything, because it claims
    /// nothing is clipped at its floor and three things are.
    #[test]
    fn r1712_a_rigid_policy_over_a_clipping_floor_is_surprised() {
        let report = audit_screen(
            ShrinkPolicy::rigid((1506, 100)),
            SizeBounds::floored((1506, 100)),
        );
        assert_eq!(report.verdict, "surprised");
        assert_eq!(report.covered, 0);
        assert_eq!(
            report.unnamed,
            ["lab.appbar", "lab.appbar.state", "lab.inspector"]
        );
    }

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
                    declared: RectReport {
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                    },
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
        let out = report(&result, (1600, 900), declared, None, &|t| rows(t));
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
            None,
            &|t| rows(t),
        );
        assert_eq!(exact.verdict, "exact");
        let roomier = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1400, 600)),
            None,
            &|t| rows(t),
        );
        assert_eq!(roomier.verdict, "roomier");
        let none = report(&result, (1600, 900), SizeBounds::UNBOUNDED, None, &|t| {
            rows(t)
        });
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
            None,
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
            None,
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
            None,
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
        let out = report(
            &result,
            (800, 600),
            SizeBounds::floored((400, 300)),
            None,
            &|t| rows(t),
        );
        assert_eq!(out.verdict, "unmeasured");
        assert_eq!(out.needed, None);
        assert_eq!(out.width, None);
        let refused = out.refused.expect("a refusal");
        // R1711.1 — no axis: the size does not fit, and the evidence is what
        // says which way. Here it is short in width only, and the row says so.
        assert_eq!(refused.axis, None);
        assert_eq!(refused.reason, "ceiling_is_short");
        assert_eq!(refused.out_of_reach.len(), 1);
        assert_eq!(refused.out_of_reach[0].tag.as_deref(), Some("narrow"));
        // And the declared floor is still published: the refusal is about the
        // screen, so what it declared is exactly what a reader wants to see.
        assert_eq!(out.declared.floor, Some(SizeReport::from((400, 300))));
    }

    #[test]
    fn r1711_an_absent_outcome_is_a_different_answer_from_an_empty_one() {
        let err = handle_scene_size_floor(None).expect_err("no outcome installed");
        assert!(format!("{err:?}").contains("SizeFloorUnavailable"));
    }

    /// A concession report shaped like the node lab's, parameterised on the
    /// three things the verdict rule reads.
    fn concession(verdict: &'static str, split: bool) -> ConcessionReport {
        clipping_concession(verdict, split, "clip")
    }

    fn clipping_concession(
        verdict: &'static str,
        split: bool,
        recourse: &'static str,
    ) -> ConcessionReport {
        ConcessionReport {
            comfortable: (1200, 500).into(),
            floor: (1100, 400).into(),
            band: (100, 100).into(),
            recourse,
            gives_up: vec!["lab.inspector".to_string()],
            cut_at_floor: rows(&["lab.inspector"]),
            covered: 1,
            unreachable: Vec::new(),
            unnamed: Vec::new(),
            stale: Vec::new(),
            declaration_split: split,
            verdict,
        }
    }

    /// ★★★★★ R1712 — the same arithmetic that reads `short` reads `conceded`
    /// once the binding has declared what the band costs and the screen honours
    /// it. Before this the word did not exist, so a screen that wanted to let
    /// its window get smaller had no way to say so and stay green.
    #[test]
    fn r1712_a_declared_and_honoured_shortfall_reads_conceded() {
        let result = measure((1600, 900), needs((1200, 500)));
        let out = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1100, 400)),
            Some(concession("honoured", false)),
            &|t| rows(t),
        );
        assert_eq!(out.verdict, "conceded");
        assert_eq!(out.needed, Some(SizeReport::from((1200, 500))));
        let published = out.concession.expect("the concession rides along");
        assert_eq!(published.floor, SizeReport::from((1100, 400)));
        assert_eq!(published.comfortable, SizeReport::from((1200, 500)));
    }

    /// ★★★★★ R1714 — the same arithmetic again, and a **different word**, for
    /// the band that moves instead of cutting.
    ///
    /// `conceded` says the reader gave something up. A panning window gives
    /// nothing up, so telling a caller it did would be the one misreading this
    /// whole round exists to prevent — and the two are indistinguishable from
    /// the numbers alone, which is why the recourse rides on the report.
    #[test]
    fn r1714_a_declared_and_honoured_pan_reads_panned() {
        let result = measure((1600, 900), needs((1200, 500)));
        let out = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1100, 400)),
            Some(clipping_concession("honoured", false, "pan")),
            &|t| rows(t),
        );
        assert_eq!(out.verdict, "panned");
        assert_eq!(
            out.concession.expect("it rides along").recourse,
            "pan",
            "and a reader can tell which decision it was",
        );
    }

    /// ★★ And it is read before the arithmetic, which is the case the real
    /// screen produces: a pan puts nothing beyond reach at any size, so `needed`
    /// bottoms out far below the floor and the arithmetic alone would say
    /// `roomier` — the same word a generously-floored clipping screen gets.
    #[test]
    fn r1714_a_pan_reads_panned_even_where_the_arithmetic_says_roomier() {
        let result = measure((1600, 900), needs((1, 1)));
        let floored = SizeBounds::floored((748, 360));
        assert_eq!(
            report(&result, (1600, 900), floored, None, &|t| rows(t)).verdict,
            "roomier",
            "with nothing declared, a floor above what is needed is just roomy",
        );
        assert_eq!(
            report(
                &result,
                (1600, 900),
                floored,
                Some(clipping_concession("honoured", false, "pan")),
                &|t| rows(t)
            )
            .verdict,
            "panned",
        );
    }

    /// ★★★ A pan the screen does NOT honour keeps the arithmetic's word: the
    /// new verdict is a claim about a working pan, so it has to be able to fail.
    #[test]
    fn r1714_a_pan_that_leaves_something_unreachable_is_not_panned() {
        let result = measure((1600, 900), needs((1, 1)));
        let out = report(
            &result,
            (1600, 900),
            SizeBounds::floored((748, 360)),
            Some(clipping_concession("unreachable", false, "pan")),
            &|t| rows(t),
        );
        assert_eq!(out.verdict, "roomier");
    }

    /// A declaration that does not match the screen buys nothing: the window is
    /// still shrinkable past what it can show, and the reason it is allowed to
    /// be has stopped being true.
    #[test]
    fn r1712_a_shortfall_whose_concession_is_not_honoured_stays_short() {
        let result = measure((1600, 900), needs((1200, 500)));
        for verdict in ["surprised", "stale", "unreachable"] {
            let out = report(
                &result,
                (1600, 900),
                SizeBounds::floored((1100, 400)),
                Some(concession(verdict, false)),
                &|t| rows(t),
            );
            assert_eq!(out.verdict, "short", "concession verdict {verdict}");
        }
    }

    /// ★ And a binding that declared a policy but told the window system a
    /// different floor is not credited either — the concession describes a
    /// floor nobody is standing at.
    #[test]
    fn r1712_a_split_declaration_is_not_credited_as_a_concession() {
        let result = measure((1600, 900), needs((1200, 500)));
        let out = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1050, 400)),
            Some(concession("honoured", true)),
            &|t| rows(t),
        );
        assert_eq!(out.verdict, "short");
    }

    /// The other verdicts are untouched by a concession riding along — only the
    /// `short` relation has a second reading.
    #[test]
    fn r1712_a_concession_does_not_change_an_exact_or_roomier_verdict() {
        let result = measure((1600, 900), needs((1200, 500)));
        let exact = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1200, 500)),
            Some(concession("honoured", false)),
            &|t| rows(t),
        );
        assert_eq!(exact.verdict, "exact");
        let roomier = report(
            &result,
            (1600, 900),
            SizeBounds::floored((1400, 600)),
            Some(concession("honoured", false)),
            &|t| rows(t),
        );
        assert_eq!(roomier.verdict, "roomier");
    }

    #[test]
    fn r1711_the_axis_words_are_the_cores() {
        assert_eq!(axis_words(), ["width", "height"]);
        assert!(Measured::<&str>::new(10, Vec::new(), 1).is_none());
    }
}
