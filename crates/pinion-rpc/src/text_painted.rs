//! `scene/text_painted` — R1654 §5.12 §5.36 §2 #7: **what a reader actually
//! sees**, when it is not what the scene says.
//!
//! An overflow policy that shortens a string (the
//! [`TextOverflow`](pinion_core::style::TextOverflow) ellipsis arms) makes the
//! painted text differ from the authored text. Without this read the scene
//! reports `demo/units/1/pose` while the screen shows `demo/uni…`, and §2 #7 —
//! the scene IS the description of what is on screen, queryable as text with no
//! pixels involved — stops being true for every elided label at once. An agent
//! reading a truncated endpoint off a snapshot would believe the whole endpoint
//! was legible.
//!
//! Measured on the reference toolkit at 6.11: nothing there can answer this.
//! Its label returns the authored string from `text()` and the elided form
//! exists only inside the paint call, so an accessibility or automation client
//! reads what the application meant rather than what the user got.
//!
//! # Every run, and the ones that were shortened say so
//!
//! A row per painted text run, with `painted` null when the authored string is
//! what was drawn. Reporting only the shortened ones was the first shape and it
//! is the wrong one: a run carries no tag of its own, so this is the only
//! surface that reports where the SCENE'S TEXT ended up — `scene/snapshot`
//! carries authored strings against pre-fold rectangles, and every tag-keyed
//! read is blind to a run by construction. A gate asking "did two runs of one
//! widget land on each other" (the signature of a run that flowed instead of
//! being placed, R1653's finding) has nowhere else to ask.

use pinion_core::Scene;
use pinion_text::LayoutCache;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One text run the frame painted as something other than what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaintedTextReport {
    /// The run's §5.20 tag, when it carries one. Untagged runs are still
    /// reported — a shortened label is worth knowing about whether or not
    /// anybody gave it an address — and identified by their box.
    pub tag: Option<String>,
    /// The nearest tagged ancestor, which is the widget this run belongs to.
    ///
    /// Carried because the question worth asking of two runs is whether they
    /// are part of the same thing: two labels of one row overlapping is a
    /// defect, while a floating annotation over a diagram is the design.
    pub owner: Option<String>,
    /// Window-absolute box, so the row can be matched to a snapshot node with
    /// no tag to match on.
    pub x: u32,
    /// Window-absolute box.
    pub y: u32,
    /// Window-absolute box.
    pub w: u32,
    /// Window-absolute box.
    pub h: u32,
    /// What the scene holds.
    pub content: String,
    /// What the frame drew, or `null` when that is exactly [`Self::content`].
    pub painted: Option<String>,
    /// How wide the glyphs actually are.
    pub ink_w: u32,
    /// How tall the glyphs actually are — more than one line's worth when the
    /// run wrapped.
    pub ink_h: u32,
    /// How many lines the shaper produced.
    ///
    /// Two is the number that matters: a run that wrapped put a second line
    /// where the author reserved room for one. Carried beside
    /// [`Self::overflows`] because the two answer different questions — a run
    /// can spill sideways on one line, and a run can wrap inside a box tall
    /// enough to hold both.
    pub lines: u32,
    /// Whether the ink is larger than the box the scene gave it.
    ///
    /// ★ The question this surface exists to answer. A rectangle in a scene is
    /// what the author PROMISED a run, and every other read reports that
    /// promise; nothing reported whether it was kept, so a screen could paint a
    /// label across the row below it and describe itself as correct. A run that
    /// overflows either wrapped onto its neighbour or spilled past its edge, and
    /// in both cases the reader is seeing something the scene does not say.
    pub overflows: bool,
}

/// Response payload for `scene/text_painted`.
#[derive(Debug, Clone, Serialize)]
pub struct TextPaintedOutcome {
    /// Every run this frame shortened, in paint order.
    pub runs: Vec<PaintedTextReport>,
}

/// Typed errors [`handle_scene_text_painted`] can return.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPaintedError {
    /// The embedder installed no list on the dispatch context.
    ///
    /// Distinct from an empty list, and the distinction is the point: empty
    /// means "this frame shortened nothing", while this means "this host
    /// cannot answer" — a host that never shapes, or a fixture with no shape
    /// cache. A caller asserting "nothing was elided" must not read the second
    /// as the first.
    TextPaintedUnavailable,
}

impl TextPaintedError {
    /// The word that rides in `error.data`.
    #[must_use]
    pub const fn wire_tag(&self) -> &'static str {
        match self {
            Self::TextPaintedUnavailable => "TextPaintedUnavailable",
        }
    }
}

/// Collect every run this frame painted as something other than what it holds.
///
/// Called by the embedder before dispatch (the `text_blocks` pattern), because
/// the answer needs BOTH the painted scene and the shape cache and only the
/// shell holds them together. `cache` must be the shell's own — the one the
/// painter shaped through — so every row here comes off an entry the frame
/// already derived and this read shapes nothing new.
///
/// The width each run is measured against is the width the painter uses: the
/// node's own box, or unbounded when it declares none. Deriving it any other
/// way would let this report an elision the frame did not perform.
pub fn collect_painted(scene: &Scene, cache: &mut LayoutCache) -> Vec<PaintedTextReport> {
    let mut rows = Vec::new();
    scene.for_each_node(&mut |visit| {
        let (Scene::Text(t), Some(rect)) = (visit.node, visit.absolute_rect()) else {
            return;
        };
        let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
        let painted = cache
            .painted_text(&t.content, &t.style, &t.runs, max_width)
            .map(str::to_owned);
        let (ink_w, ink_h) = cache.ink_size(&t.content, &t.style, &t.runs, max_width);
        let lines = cache.line_count(&t.content, &t.style, &t.runs, max_width);
        rows.push(PaintedTextReport {
            tag: t.tag.as_deref().map(str::to_owned),
            owner: visit
                .ancestors
                .iter()
                .rev()
                .find_map(|a| a.tag())
                .map(str::to_owned),
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            content: t.content.clone(),
            painted,
            ink_w,
            ink_h,
            lines,
            // Against the box the SCENE gave the run, not against the clipped
            // rectangle: a run inside a scroll that is half out of view has
            // kept its promise, and reporting that as an overflow would make
            // every scrolled label a defect.
            overflows: (t.rect.w > 0 && ink_w > t.rect.w) || (t.rect.h > 0 && ink_h > t.rect.h),
        });
    });
    rows
}

/// `scene/text_painted` dispatcher entry.
///
/// # Errors
///
/// [`TextPaintedError::TextPaintedUnavailable`] when the embedder installed no
/// list.
pub fn handle_scene_text_painted(runs: Option<&[PaintedTextReport]>) -> Result<Value, RpcError> {
    let Some(runs) = runs else {
        return Err(RpcError::invalid_params(
            TextPaintedError::TextPaintedUnavailable.wire_tag(),
        ));
    };
    let outcome = TextPaintedOutcome {
        runs: runs.to_vec(),
    };
    serde_json::to_value(outcome).map_err(|err| RpcError::internal_error(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{ContainerNode, Rect, TextNode};
    use pinion_core::style::{TextOverflow, TextStyle};

    fn label(content: &str, w: u32, overflow: TextOverflow, tag: &'static str) -> Scene {
        let mut style = TextStyle::new();
        style.font_size_px = 12;
        style.overflow = overflow;
        // 30px of height for a 12px face: tall enough that these fixtures are
        // about WIDTH. A box shorter than its own line box overflows too, and
        // that is a separate (and very common) authoring nit.
        Scene::Text(TextNode::styled(content, Rect::new(0, 0, w, 30), style).with_tag(tag))
    }

    /// ★ Only the runs the frame shortened, and each says both strings.
    #[test]
    fn r1654_the_read_names_what_was_authored_and_what_was_drawn() {
        let scene = Scene::Container(ContainerNode::new(vec![
            label("demo/units/1/pose", 60, TextOverflow::Ellipsis, "cut"),
            label("ok", 200, TextOverflow::Ellipsis, "fits"),
            label("demo/units/1/pose", 60, TextOverflow::Visible, "not_elided"),
        ]));
        let mut cache = LayoutCache::new();
        let rows = collect_painted(&scene, &mut cache);
        assert_eq!(rows.len(), 3, "every run, shortened or not: {rows:?}");
        let cut = rows
            .iter()
            .find(|r| r.tag.as_deref() == Some("cut"))
            .unwrap();
        assert_eq!(cut.content, "demo/units/1/pose");
        let painted = cut.painted.as_deref().expect("shortened");
        assert!(painted.ends_with('\u{2026}'), "{painted}");
        assert_ne!(painted, cut.content);
        for tag in ["fits", "not_elided"] {
            let row = rows.iter().find(|r| r.tag.as_deref() == Some(tag)).unwrap();
            assert_eq!(row.painted, None, "{tag}: drawn as authored, and says so");
        }
    }

    /// ★ R1654 — a run whose glyphs are bigger than the box the scene gave it
    /// says so, which is the question `scene/text_painted` was asked for: a
    /// screen can paint a label across the row below it and every other read
    /// reports the promise rather than whether it was kept.
    #[test]
    fn r1654_a_run_whose_ink_leaves_its_box_reports_it() {
        // 60px of box for a string that measures far more, with the policy that
        // keeps every character.
        let scene = Scene::Container(ContainerNode::new(vec![
            label("demo/units/1/pose", 60, TextOverflow::Visible, "spilling"),
            label("ok", 200, TextOverflow::Visible, "roomy"),
        ]));
        let mut cache = LayoutCache::new();
        let rows = collect_painted(&scene, &mut cache);
        let spilling = rows
            .iter()
            .find(|r| r.tag.as_deref() == Some("spilling"))
            .unwrap();
        let roomy = rows
            .iter()
            .find(|r| r.tag.as_deref() == Some("roomy"))
            .unwrap();
        assert!(
            spilling.overflows,
            "60px of box, {}px of ink",
            spilling.ink_w
        );
        assert!(
            spilling.ink_w > 0 && spilling.ink_h > 0,
            "the ink is measured"
        );
        assert!(!roomy.overflows, "and a run with room does not cry wolf");
        assert!(roomy.ink_w <= 200, "{}", roomy.ink_w);
    }

    /// An eliding run does not overflow — which is the whole point of the arm,
    /// and the property that lets a screen be checked rather than eyeballed.
    #[test]
    fn r1654_an_eliding_run_does_not_overflow() {
        let scene = Scene::Container(ContainerNode::new(vec![label(
            "demo/units/1/pose",
            60,
            TextOverflow::Ellipsis,
            "cut",
        )]));
        let mut cache = LayoutCache::new();
        let rows = collect_painted(&scene, &mut cache);
        assert!(!rows[0].overflows, "ink {}px in a 60px box", rows[0].ink_w);
        assert!(rows[0].painted.is_some(), "and it says it was shortened");
    }

    /// An absent list and an empty one are different answers.
    #[test]
    fn r1654_no_list_is_not_an_empty_list() {
        let err = handle_scene_text_painted(None).expect_err("unavailable");
        assert_eq!(
            err.data,
            Some(Value::String("TextPaintedUnavailable".to_owned()))
        );
        let ok = handle_scene_text_painted(Some(&[])).expect("empty is an answer");
        assert_eq!(ok["runs"], Value::Array(vec![]));
    }
}
