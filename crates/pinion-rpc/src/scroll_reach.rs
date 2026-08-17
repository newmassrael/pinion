//! `scene/scroll_reach` — what this screen is not showing, and whether the
//! reader can get to it.
//!
//! The visual twin of [`crate::pointer_reach`]. That read answers *can a press
//! land on this widget*; this one answers *can a person ever see it*, and the
//! two are independent: a row scrolled out of view is perfectly pressable once
//! it is on screen, and a widget under an opaque sibling is fully visible.
//!
//! # Why this is not a field on `scene/containment`
//!
//! Containment asks whether a mark stayed inside the box that owns it, which is
//! a question about the painter. This asks whether any offset the enclosing
//! viewport can take brings the mark into view, which is a question about the
//! pane. Measured on a synthetic pane before this existed (`pinion_core::reach`
//! carries the case as a test): a row the scroll range covers produced **no
//! containment report at all**, and a row past the end of the content produced
//! `fate: "clipped"` — the same word a two-pixel line-box rounding gets. So the
//! two defects that matter here, *the reader must scroll* and *the reader can
//! never see this*, were the same answer and no answer.
//!
//! # What a caller does with it
//!
//! `lost` is the number a gate fails on. Every entry in it names a mark the
//! window and its scroll ranges between them cannot show, so it is the direct
//! evidence for "this pane needs to scroll" and, once the pane does scroll, the
//! direct evidence that the repair worked: the same mark moves to
//! `scrollable`, carrying the offset that reveals it.
//!
//! `window` is published beside the counts because the whole derivation is
//! judged against it. A surface that has not been laid out reports a zero
//! window, and every mark on it would read as lost; publishing the number is
//! what lets a caller tell that apart from a real finding.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/scroll_reach", "id": 1 }
//! ```

use pinion_core::reach::{OutOfSight, Reach};
use pinion_core::scene::Scene;
use pinion_text::LayoutCache;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;
use crate::containment::RectReport;

/// The box a mark was judged against, and how far it can move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewportReport {
    /// The enclosing scroll node's tag, or `<window>`.
    pub name: String,
    /// (R1685) Where this window sits in the frame `rect` is expressed in.
    ///
    /// `0` for a scroll (its content has its own frame, with the origin at the
    /// top-left) and the box's own position for a container that clips because
    /// it declares `overflow: hidden`, which introduces no frame. Without it a
    /// client holding a mark's `rect` and this viewport cannot say where one
    /// sits inside the other — the numbers would be in two frames with nothing
    /// naming either.
    pub origin_x: u32,
    /// The vertical half of [`Self::origin_x`].
    pub origin_y: u32,
    /// The viewport's width.
    pub w: u32,
    /// The viewport's height.
    pub h: u32,
    /// The content's width, as the laid-out subtree reports it.
    pub content_w: u32,
    /// The content's height.
    pub content_h: u32,
    /// The horizontal offset the scene carries right now.
    pub at_x: i32,
    /// The vertical offset.
    pub at_y: i32,
    /// The largest horizontal offset this viewport can take.
    pub max_x: i32,
    /// The largest vertical offset.
    pub max_y: i32,
    /// Whether the content needs no scrolling at all. Derived, not stored —
    /// the predicate a consumer of the reference toolkit has to infer from
    /// `maximum() > 0`, which is also that toolkit's answer for a scroll area
    /// that never set its range.
    pub fits: bool,
}

/// One mark the reader cannot currently see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutOfSightReport {
    /// The mark's own tag, when it has one.
    pub tag: Option<String>,
    /// The mark's address, `/`-joined the way `scene/locate` reports it.
    pub path: String,
    /// What a text run holds: the string a reader is not seeing.
    pub content: Option<String>,
    /// Where the mark sits in its viewport's content coordinates.
    pub rect: RectReport,
    /// The viewport it was judged against.
    pub viewport: ViewportReport,
    /// `scrollable` (the range covers it) or `lost` (nothing does).
    pub reach: &'static str,
    /// For `scrollable`: the horizontal offset that shows it.
    pub to_x: Option<i32>,
    /// For `scrollable`: the vertical offset that shows it.
    pub to_y: Option<i32>,
    /// For `lost`: how far past the reachable box it reaches, per edge, in
    /// `left, top, right, bottom` order.
    pub short_by: Option<[u32; 4]>,
}

/// The `scene/scroll_reach` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScrollReachOutcome {
    /// The window every mark with no scroll ancestor was judged against.
    pub window: RectReport,
    /// How many painted marks were examined — what keeps an empty list from
    /// reading as coverage on a surface that painted nothing.
    pub marks: usize,
    /// How many are off screen but within some viewport's range.
    pub scrollable: usize,
    /// How many nothing can bring into view. The number a gate fails on.
    pub lost: usize,
    /// Every mark that is off screen, in paint order.
    pub out_of_sight: Vec<OutOfSightReport>,
}

/// Typed errors [`handle_scene_scroll_reach`] can return.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrollReachError {
    /// R1711 — `params.at` is present but is not a `{width, height}` pair of
    /// non-zero unsigned integers.
    ///
    /// A separate arm rather than a silently ignored key: measured before this
    /// existed, this method accepted `at`, `viewport`, `width`/`height` and
    /// `dry_run` alike and answered about the live window every time, so a
    /// caller asking about a size it was not at was told a number that looked
    /// like an answer. A parameter a method does not implement has to be an
    /// error, or the method is lying by omission.
    InvalidAt,
    /// The embedder installed no report.
    ///
    /// Distinct from an empty list for the reason its sibling states: empty
    /// means "everything is in view", this means "this host cannot answer".
    ScrollReachUnavailable,
}

impl ScrollReachError {
    /// The word that rides in `error.data`.
    #[must_use]
    pub const fn wire_tag(&self) -> &'static str {
        match self {
            Self::InvalidAt => "InvalidAt",
            Self::ScrollReachUnavailable => "ScrollReachUnavailable",
        }
    }
}

/// R1711 §2 #3 — the size this request is about, or `None` for the live window.
///
/// One rule with two callers: the embedder reads it to decide which scene to
/// measure, and the dispatcher reads it to refuse a malformed one. Writing the
/// parse twice is how the two would come to disagree about what `at: {}` means.
///
/// # Errors
///
/// [`ScrollReachError::InvalidAt`] for anything other than an object carrying
/// two non-zero unsigned integers. Zero is refused rather than clamped: a
/// window of no extent puts every mark out of reach, so answering it would hand
/// back a confident list of losses for a size no screen can be at.
pub fn parse_at(params: Option<&Value>) -> Result<Option<(u32, u32)>, ScrollReachError> {
    let Some(at) = params.and_then(|p| p.get("at")) else {
        return Ok(None);
    };
    let obj = at.as_object().ok_or(ScrollReachError::InvalidAt)?;
    let read = |key: &str| -> Result<u32, ScrollReachError> {
        let raw = obj
            .get(key)
            .and_then(Value::as_u64)
            .ok_or(ScrollReachError::InvalidAt)?;
        let value = u32::try_from(raw).map_err(|_| ScrollReachError::InvalidAt)?;
        if value == 0 {
            return Err(ScrollReachError::InvalidAt);
        }
        Ok(value)
    };
    Ok(Some((read("width")?, read("height")?)))
}

/// Turn the core report into its wire form.
///
/// The counts are folded out of the list here, so a caller cannot be told
/// "nothing is lost" beside a list of losses.
#[must_use]
pub fn report(window: (u32, u32), out: &[OutOfSight], marks: usize) -> ScrollReachOutcome {
    ScrollReachOutcome {
        window: RectReport {
            x: 0,
            y: 0,
            w: window.0,
            h: window.1,
        },
        marks,
        scrollable: out.iter().filter(|o| !o.reach.is_lost()).count(),
        lost: out.iter().filter(|o| o.reach.is_lost()).count(),
        out_of_sight: out
            .iter()
            .map(|o| {
                let (to_x, to_y, short_by) = match o.reach {
                    Reach::Scrollable { to } => (Some(to.0), Some(to.1), None),
                    Reach::Lost { short_by } => (
                        None,
                        None,
                        Some([short_by.left, short_by.top, short_by.right, short_by.bottom]),
                    ),
                };
                OutOfSightReport {
                    tag: o.tag.clone(),
                    path: o.path.join("/"),
                    content: o.content.clone(),
                    rect: o.rect.into(),
                    viewport: ViewportReport {
                        name: o.viewport.name.clone(),
                        origin_x: o.viewport.origin.0,
                        origin_y: o.viewport.origin.1,
                        w: o.viewport.size.0,
                        h: o.viewport.size.1,
                        content_w: o.viewport.content.0,
                        content_h: o.viewport.content.1,
                        at_x: o.viewport.at.0,
                        at_y: o.viewport.at.1,
                        max_x: o.viewport.max.0,
                        max_y: o.viewport.max.1,
                        fits: o.viewport.fits(),
                    },
                    reach: o.reach.wire_word(),
                    to_x,
                    to_y,
                    short_by,
                }
            })
            .collect(),
    }
}

/// Measure the painted scene and build the wire report.
///
/// The window is read off the painted root's own rectangle rather than plumbed
/// in from the embedder: `compute_layout` assigns it there, so this is the
/// extent the scene was actually laid out against and cannot drift from a
/// second copy of the number. A root with no extent publishes a zero window,
/// which is why [`ScrollReachOutcome::window`] rides on the wire.
///
/// The ink of a text run is asked of the same [`LayoutCache`] the frame shaped
/// with, so this reports what was drawn and not a second opinion about it.
#[must_use]
pub fn collect(scene: &Scene, cache: &mut LayoutCache) -> ScrollReachOutcome {
    let root = scene.rect();
    let mut marks = 0usize;
    scene.for_each_node(&mut |_| marks += 1);
    let out = pinion_core::reach::out_of_sight(scene, (root.w, root.h), &mut |t| {
        let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
        cache.ink_size(&t.content, &t.style, &t.runs, max_width)
    });
    report((root.w, root.h), &out, marks)
}

/// `scene/scroll_reach` dispatcher entry.
///
/// # Errors
///
/// [`ScrollReachError::InvalidAt`] for a malformed `params.at`, and
/// [`ScrollReachError::ScrollReachUnavailable`] when the embedder installed no
/// report. The `at` check runs FIRST: a host that cannot lay out a hypothetical
/// size and a request that named an impossible one are different faults, and
/// reporting the second as the first would send a caller to fix the wrong end.
pub fn handle_scene_scroll_reach(
    outcome: Option<&ScrollReachOutcome>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    if let Err(err) = parse_at(params) {
        return Err(RpcError::invalid_params(err.wire_tag()));
    }
    let Some(outcome) = outcome else {
        return Err(RpcError::invalid_params(
            ScrollReachError::ScrollReachUnavailable.wire_tag(),
        ));
    };
    serde_json::to_value(outcome).map_err(|err| RpcError::internal_error(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::containment::Overhang;
    use pinion_core::reach::Viewport;
    use pinion_core::scene::Rect;

    fn viewport() -> Viewport {
        Viewport {
            name: "pane".into(),
            origin: (0, 0),
            size: (100, 100),
            content: (100, 300),
            at: (0, 0),
            max: (0, 200),
        }
    }

    fn entry(reach: Reach) -> OutOfSight {
        OutOfSight {
            tag: Some("row".into()),
            path: vec!["4".into(), "0".into()],
            content: Some("data, express".into()),
            rect: Rect::new(0, 240, 60, 12),
            viewport: viewport(),
            reach,
        }
    }

    /// ★ The counts are derived from the list, so a caller cannot be told
    /// "nothing is lost" beside a list of losses.
    #[test]
    fn r1662_the_counts_are_derived_from_the_list() {
        let out = report(
            (800, 600),
            &[
                entry(Reach::Scrollable { to: (0, 152) }),
                entry(Reach::Lost {
                    short_by: Overhang {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 112,
                    },
                }),
            ],
            331,
        );
        assert_eq!(out.scrollable, 1);
        assert_eq!(out.lost, 1);
        assert_eq!(out.marks, 331);
        assert_eq!(out.window.w, 800);
        assert_eq!(out.out_of_sight[0].reach, "scrollable");
        assert_eq!(out.out_of_sight[1].reach, "lost");
    }

    /// ★ Each arm carries only its own payload: an offset for the one that has
    /// one and a shortfall for the one that does not. A row with both filled in
    /// would be a row nobody can read.
    #[test]
    fn r1662_each_arm_carries_only_its_own_payload() {
        let out = report((800, 600), &[entry(Reach::Scrollable { to: (3, 152) })], 1);
        assert_eq!(out.out_of_sight[0].to_y, Some(152));
        assert_eq!(out.out_of_sight[0].to_x, Some(3));
        assert_eq!(out.out_of_sight[0].short_by, None);

        let out = report(
            (800, 600),
            &[entry(Reach::Lost {
                short_by: Overhang {
                    left: 1,
                    top: 2,
                    right: 3,
                    bottom: 4,
                },
            })],
            1,
        );
        assert_eq!(out.out_of_sight[0].short_by, Some([1, 2, 3, 4]));
        assert_eq!(out.out_of_sight[0].to_y, None);
    }

    /// ★ The predicate the reference makes a consumer infer rides as a field.
    #[test]
    fn r1662_the_viewport_publishes_whether_it_fits() {
        let out = report((800, 600), &[entry(Reach::Scrollable { to: (0, 152) })], 1);
        let v = &out.out_of_sight[0].viewport;
        assert!(!v.fits);
        assert_eq!((v.content_w, v.content_h), (100, 300));
        assert_eq!((v.max_x, v.max_y), (0, 200));
    }

    /// ★ An absent report is not a screen that shows everything, and the wire
    /// says which.
    #[test]
    fn r1662_an_unavailable_report_is_not_a_screen_in_view() {
        let err = handle_scene_scroll_reach(None, None).expect_err("no report installed");
        assert!(
            format!("{err:?}").contains("ScrollReachUnavailable"),
            "{err:?}"
        );
        let clean = handle_scene_scroll_reach(Some(&report((800, 600), &[], 12)), None)
            .expect("empty is answer");
        assert_eq!(clean["out_of_sight"].as_array().map(Vec::len), Some(0));
        assert_eq!(clean["lost"], 0);
        assert_eq!(clean["marks"], 12);
    }

    /// ★ The path is `/`-joined the way every sibling read spells an address,
    /// so a caller can hand it straight to `scene/snapshot`.
    #[test]
    fn r1662_the_path_is_the_address_the_other_reads_accept() {
        let out = report((800, 600), &[entry(Reach::Scrollable { to: (0, 1) })], 1);
        assert_eq!(out.out_of_sight[0].path, "4/0");
    }

    /// ★★ R1711 — the size a caller asks about is read once, here, and
    /// anything that is not a size is a refusal rather than a shrug.
    #[test]
    fn r1711_a_size_to_ask_about_is_parsed_or_refused() {
        let at = |v: serde_json::Value| parse_at(Some(&serde_json::json!({ "at": v })));
        assert_eq!(
            at(serde_json::json!({"width": 1625, "height": 360})),
            Ok(Some((1625, 360)))
        );
        assert_eq!(parse_at(None), Ok(None));
        assert_eq!(parse_at(Some(&serde_json::json!({}))), Ok(None));
        for bad in [
            serde_json::json!({"width": 1625}),
            serde_json::json!({"w": 1625, "h": 360}),
            serde_json::json!({"width": 0, "height": 360}),
            serde_json::json!({"width": 1625, "height": 0}),
            serde_json::json!({"width": -3, "height": 360}),
            serde_json::json!([1625, 360]),
        ] {
            assert_eq!(at(bad.clone()), Err(ScrollReachError::InvalidAt), "{bad}");
        }
    }

    /// ★★★★★ R1711 — the defect this parameter closes: before it, THIS method
    /// took `at`, `viewport`, `width`/`height` and `dry_run` without complaint
    /// and answered about the live window every time. A caller asking about a
    /// size the window is not at was handed a number that read like an answer.
    #[test]
    fn r1711_a_size_this_host_cannot_answer_is_an_error_and_not_the_live_window() {
        let live = report((1625, 900), &[], 12);
        let err = handle_scene_scroll_reach(
            Some(&live),
            Some(&serde_json::json!({"at": {"width": 0, "height": 360}})),
        )
        .expect_err("a zero extent is not a window");
        assert!(format!("{err:?}").contains("InvalidAt"), "{err:?}");
        // And the check runs before the availability one, so the two faults
        // stay distinguishable.
        let err = handle_scene_scroll_reach(None, Some(&serde_json::json!({"at": 7})))
            .expect_err("a malformed ask is not a missing host");
        assert!(format!("{err:?}").contains("InvalidAt"), "{err:?}");
    }
}
