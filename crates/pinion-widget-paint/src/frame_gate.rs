//! R1674 §5.32 §2 #7 — **does a painter that strokes a border keep its own
//! contents inside that border?**, asked of every painter in this crate that
//! strokes one.
//!
//! # Why this is a crate-level gate and not a screen's job
//!
//! R1673 gave [`group_box`](crate::group_box) that test and then ran two
//! counterfactuals against it: one deleting the frame's pixel reservation from
//! the content region, one deleting the title band's knowledge of the checkbox
//! it holds. **Both passed.** The whole of this crate's suite stayed green
//! while the widget painted its content over all four edges of its own outline
//! and its title checkbox stood two pixels above and below its band. The
//! defects were real and a consumer booting was the only thing that could see
//! them — one screen at a time, which is the shape R1655 already recorded here.
//!
//! Then the test that closed those two found a **third** defect on its first
//! run, in the arm with no checkbox at all. A check with that hit rate is not
//! one widget's test; it is the crate's.
//!
//! Measured the same day, the population it was missing: fifteen painters here
//! call `with_border` and two asked this question.
//!
//! # The population is derived, not listed
//!
//! [`tests::r1674_every_bordered_painter_asks_whether_it_keeps_its_frame`]
//! parses this crate's own sources and requires that a module stroking a border
//! also *runs this gate*. A hand-written roster is the failure mode
//! [[debt-param-census-blind-to-variable-keys]] names and R1651.1 measured: a
//! sweep reported "40 surfaces pass" without saying who chose the forty, and
//! three of the ones nobody chose were broken.
//!
//! The exception list is empty and there is nowhere to put an entry, which is
//! deliberate — a painter that cannot pass this has a defect, not an excuse.
//!
//! # The metric is the one the layout used, and that is not the screens' choice
//!
//! [`screen_ink`] measures a run with a font-independent stand-in, wider per
//! character than any real face, and that is right for a *screen*: its boxes
//! are constants an author wrote down, so a conservative measure asks whether
//! the author left enough room.
//!
//! It is wrong here, and the first run of this gate is the evidence. Six
//! painters failed, every one of them a label overflowing to the right by eight
//! to sixty pixels, and the cause was not the painters: a **widget** sizes its
//! box from its content, so the box under test had been computed by the real
//! shaper while the check re-measured the same string with a stand-in 60% wider.
//! The two disagreed about the same text, and the disagreement was the finding.
//! A gate that fires on every correct painter is a gate nobody keeps.
//!
//! So the ink here comes from the same [`LayoutCache`] the layout pass shaped
//! with — the arrangement `scene/containment` already uses on the wire. What
//! stays caught is the case the stand-in cannot judge: a box pinned to a
//! constant with a label that outgrows it.
//!
//! ★ **And the exposure that buys, stated rather than implied.** For a box
//! sized from its own content, a host with wider faces moves the box and the
//! ink together and this gate does not notice. For a box pinned to a
//! **constant**, it moves only the ink — so such a painter can be green here
//! and red on another machine, which is a [[zero-flake-policy]] liability and
//! not a hypothetical one: measured across the fifteen gated painters, ten
//! declare fixed-pixel boxes and no [`TextOverflow`] policy at all. A declared
//! policy removes the exposure, because the stand-in and the shaper both clamp
//! an eliding run to its own rectangle. Registered as
//! [[debt-ten-painters-pin-a-box-and-state-no-overflow-policy]] rather than
//! left as a property of whichever machine last ran this.
//!
//! [`TextOverflow`]: pinion_core::style::TextOverflow
//!
//! [`screen_ink`]: pinion_core::test_fixtures::screen_ink
//! [`LayoutCache`]: pinion_text::LayoutCache

use pinion_core::Scene;
use pinion_core::containment::escapes;
use pinion_core::scene::ContainerNode;
use pinion_core::style::{LayoutStyle, Size};

/// The window sizes every painter is asked about.
///
/// Two, because one is the size a thing was authored at and R1656 recorded what
/// that costs: every check on a screen ran the layout at the same constant the
/// screen was designed against, so the assumption and the defect were the same
/// number and five escapes went unseen. The narrow case is where a band that
/// was tall enough stops being tall enough.
const SIZES: [(u32, u32); 2] = [(420, 260), (180, 120)];

/// The tag on the window this gate lays a painter out in.
///
/// Named so an escape **owned by it** can be dropped, and the limit that
/// creates is worth stating: this gate does not report *"the painter is bigger
/// than the window"*. That is a real question and it is the SCREEN's — a
/// consumer chooses the room a widget gets, and a settings form with two fields
/// genuinely does not fit 180 pixels. What stays reported is the question this
/// gate is for: a painter's own contents leaving the painter's own frame, which
/// arrives with the painter's tag as owner. Measured while writing this: the
/// toolbar's fifth control escaping the *toolbar* survives the filter, and the
/// config form being wider than a small test window does not.
const HARNESS_ROOT: &str = "frame_gate.window";

/// Run `build` through a real layout pass at every size in [`SIZES`](self::SIZES) and assert
/// that nothing it paints lands on the frame it strokes.
///
/// `build` is called once per size rather than handed a scene, because a
/// painter that takes its width as an argument has to be asked at both — handing
/// in one pre-built scene would ask the wide question twice.
///
/// Returns how many marks were examined across all sizes, so a caller can pin a
/// floor and notice the day its painter starts producing nothing. A gate whose
/// population silently went to zero reports the same "0 escapes" as a gate that
/// passed, which is the failure R1664 recorded for `pointer_reach`.
pub(crate) fn assert_frame_contained(
    what: &str,
    build: &mut dyn FnMut(u32, u32) -> Scene,
) -> usize {
    assert_frame_contained_at(what, &SIZES, build)
}

/// [`assert_frame_contained`] at sizes the caller picks.
///
/// For a painter whose surface is **anchored rather than bound** — a popup, a
/// dropdown, a tooltip that overhangs its anchor. Such a surface is absolutely
/// positioned and is allowed to extend past whatever is behind it (R1672), so
/// running it in a window smaller than itself reports that a 200px menu is
/// wider than a 180px window. True, and not the question: this gate asks
/// whether a painter's own contents stay inside the painter's own frame.
///
/// The sizes stay a caller's argument rather than an exemption flag, so the
/// answer is still measured at more than one size — which is the axis R1656
/// recorded as never having been one.
pub(crate) fn assert_frame_contained_at(
    what: &str,
    sizes: &[(u32, u32)],
    build: &mut dyn FnMut(u32, u32) -> Scene,
) -> usize {
    let mut marks = 0usize;
    assert!(
        sizes.len() >= 2,
        "{what}: one size is the case R1656 measured — the assumption and the \
         defect are then the same number",
    );
    for &(width, height) in sizes {
        let mut scene = Scene::Container(
            ContainerNode::new(vec![build(width, height)])
                .with_tag(HARNESS_ROOT)
                .with_layout(LayoutStyle::new().with_size(Size::px(width, height))),
        );
        let mut cache = pinion_text::LayoutCache::new();
        pinion_runtime::layout::compute_layout(&mut scene, &mut cache, width, height);
        // The SAME cache the layout just shaped with — see the module header
        // for why a stand-in is the wrong measure for a content-sized box.
        let found: Vec<_> = escapes(&scene, &mut |text| {
            let max_width = (text.rect.w > 0).then_some(text.rect.w);
            cache.ink_size(&text.content, &text.style, &text.runs, max_width)
        })
        .into_iter()
        .filter(|e| e.owner != HARNESS_ROOT)
        .collect();
        assert!(
            found.is_empty(),
            "{what} at {width}x{height}: {} painted mark(s) left the box that \
             owns them — {:?}",
            found.len(),
            found
                .iter()
                .map(|e| (
                    e.content.clone().or_else(|| e.tag.clone()),
                    e.owner.clone(),
                    e.over,
                    e.trespass.clone(),
                ))
                .take(6)
                .collect::<Vec<_>>()
        );
        scene.for_each_node(&mut |_| marks += 1);
    }
    assert!(
        marks > 0,
        "{what} painted nothing at either size, so this gate proved nothing",
    );
    marks
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Every module in `src/` that calls `with_border`, and every module that
    /// runs [`super::assert_frame_contained`], read from the sources.
    ///
    /// A parse rather than a grep of the whole file: R1669 re-classified a "23
    /// call sites" figure into one production call, eighteen test fixtures,
    /// three doc lines and a builder definition, and recorded raw grep counts
    /// as a census population as the fifth failure of that shape. Here the two
    /// questions are asked of different halves of each file — a border STROKED
    /// is production code, a gate RUN is test code — so a scan that could not
    /// tell them apart would answer both wrong.
    fn census(dir: &Path) -> (BTreeSet<String>, BTreeSet<String>) {
        let mut strokes = BTreeSet::new();
        let mut gated = BTreeSet::new();
        for entry in std::fs::read_dir(dir).expect("src/ is readable") {
            let path = entry.expect("a readable dir entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let name = path
                .file_stem()
                .expect("a .rs file has a stem")
                .to_string_lossy()
                .into_owned();
            let source = std::fs::read_to_string(&path).expect("a readable source file");
            let file: syn::File = syn::parse_str(&source).expect("this crate's own sources parse");
            let mut walk = Walk::default();
            syn::visit::Visit::visit_file(&mut walk, &file);
            if walk.strokes {
                strokes.insert(name.clone());
            }
            if walk.gated {
                gated.insert(name);
            }
        }
        (strokes, gated)
    }

    /// Which of the two calls a file makes, found in the syntax tree so a
    /// mention inside a doc comment or a string cannot answer for either.
    #[derive(Default)]
    struct Walk {
        strokes: bool,
        gated: bool,
    }

    impl syn::visit::Visit<'_> for Walk {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if call.method == "with_border" {
                self.strokes = true;
            }
            syn::visit::visit_expr_method_call(self, call);
        }

        fn visit_expr_call(&mut self, call: &syn::ExprCall) {
            if let syn::Expr::Path(path) = &*call.func
                && path.path.segments.last().is_some_and(|s| {
                    s.ident == "assert_frame_contained" || s.ident == "assert_frame_contained_at"
                })
            {
                self.gated = true;
            }
            syn::visit::visit_expr_call(self, call);
        }
    }

    /// ★★ The gate's own population check: a painter that strokes a border runs
    /// this gate.
    ///
    /// Measured when it was written: fifteen modules stroke, two asked. The
    /// direction of the assertion is the load-bearing part — it is
    /// `strokes ⊆ gated`, so *adding* a bordered painter fails here until it
    /// asks, which is the case a roster written today cannot cover.
    #[test]
    fn r1674_every_bordered_painter_asks_whether_it_keeps_its_frame() {
        let (strokes, gated) = census(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path());
        assert!(
            strokes.len() >= 15,
            "the census found {} bordered painters, and there were 15 when this \
             was measured — a scan that stopped seeing them would pass this \
             file vacuously: {strokes:?}",
            strokes.len(),
        );
        let missing: Vec<&String> = strokes.difference(&gated).collect();
        assert!(
            missing.is_empty(),
            "{} painter(s) stroke a border and never ask whether their own \
             contents stay inside it: {missing:?}. Add a test calling \
             `frame_gate::assert_frame_contained`. There is no exception list \
             on purpose — a painter that cannot pass this has a defect.",
            missing.len(),
        );
    }
}
