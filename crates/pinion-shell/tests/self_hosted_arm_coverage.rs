//! R1506 §5.37 §5.27 — how much of a real widget's text does the self-hosted
//! arm still paint?
//!
//! [`self_hosted_text_eligible`] is a predicate over a leaf, and R1505 asserted
//! it one leaf at a time: an aligned leaf defers. What that test could not say
//! is how much text the rule actually moves, because a predicate does not know
//! how often it is asked.
//!
//! It moved a lot. R1504 set `ColumnLayout`'s default alignment to `Center` (the toolkit's
//! header view default), and the arm requires `Start` — so in one round every
//! header label in the workspace's header widget left the arm for parley. That
//! is the CORRECT outcome, since parley is the reference and R1505's pixel
//! guard is what proves the alignment it applies is real. It is also a silent
//! change in which engine paints production text, and nothing measured it.
//!
//! This is the measurement, over a scene built from the same
//! [`view_header_cell`] the binding paints. It is a census, not a threshold: it
//! asserts the split is a FUNCTION OF THE DECLARATION — every label declines
//! while the rule is `Center`, every one is served when the rule is `Start` —
//! so a future change to either the predicate or a widget's defaults shows up
//! here as a diff instead of as nothing.

use pinion_core::scene::Scene;
use pinion_core::style::TextAlign;
use pinion_core::theme::Theme;
use pinion_core::widgets::column_layout::{SectionPlacement, SectionSelection};
use pinion_runtime::text_engine::SelfHostedTextEngine;
use pinion_text_font::Font;
use pinion_widget_paint::column_header::{ColumnHeaderStyle, HeaderSection, view_header_cell};

/// The §5.37 parser fixture. The arm holds one face and this test asks only
/// which leaves it MAY paint, so the face's identity is irrelevant — what
/// matters is that it is a real parsed font rather than host discovery.
const NOTO: &[u8] = include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");

const HEADERS: [&str; 5] = ["Name", "Type", "Size", "Modified", "Owner"];
const WIDTHS: [u32; 5] = [150, 90, 100, 130, 100];

/// The real header strip, painted through the binding's own function.
///
/// `sorted` names the section showing a sort arrow, so the census also covers
/// the glyph leaf — which is NOT aligned and therefore answers differently
/// from the labels beside it. A census over labels alone would have missed
/// that the strip is mixed.
fn header_strip(align: TextAlign, sorted: Option<usize>) -> Vec<Scene> {
    header_strip_selected(align, sorted, SectionSelection::Unselected)
}

/// R1510 — the same strip with a selection published, so the census can ask what
/// the arm does with the WEIGHT the highlight declares.
fn header_strip_selected(
    align: TextAlign,
    sorted: Option<usize>,
    selection: SectionSelection,
) -> Vec<Scene> {
    let theme = Theme::light();
    let style = ColumnHeaderStyle::new();
    let mut x = 0;
    let mut cells = Vec::new();
    for (visual, (label, size)) in HEADERS.iter().zip(WIDTHS).enumerate() {
        let placement = SectionPlacement {
            visual,
            logical: visual,
            x,
            size,
        };
        let section = HeaderSection {
            label,
            align,
            sort: (sorted == Some(visual)).then_some(true),
            dragged: false,
            focused: false,
            selection,
        };
        cells.push(view_header_cell(
            "colhdr", &placement, &section, &style, &theme,
        ));
        x += size;
    }
    cells
}

/// `(served, declined)` over every `Scene::Text` leaf the walk reaches.
fn census(scenes: &[Scene], engine: &SelfHostedTextEngine) -> (usize, usize) {
    fn walk(
        scene: &Scene,
        engine: &SelfHostedTextEngine,
        served: &mut usize,
        declined: &mut usize,
    ) {
        match scene {
            Scene::Text(t) => {
                if engine.serves(&t.content, &t.style, &t.runs, t.caret_bearing) {
                    *served += 1;
                } else {
                    *declined += 1;
                }
            }
            Scene::Container(c) => {
                for child in &c.children {
                    walk(child, engine, served, declined);
                }
            }
            _ => {}
        }
    }
    let (mut served, mut declined) = (0, 0);
    for scene in scenes {
        walk(scene, engine, &mut served, &mut declined);
    }
    (served, declined)
}

fn engine() -> SelfHostedTextEngine {
    SelfHostedTextEngine::from_font(Font::from_bytes(NOTO.to_vec()).expect("parse NotoSans"))
}

/// A `Center` header — what R1504 made the default — hands every label to
/// parley, and the same strip under `Start` hands every one back.
#[test]
fn the_arms_share_of_a_header_strip_is_a_function_of_the_declaration() {
    let engine = engine();

    let (served, declined) = census(&header_strip(TextAlign::Center, None), &engine);
    assert_eq!(
        (served, declined),
        (0, HEADERS.len()),
        "under the toolkit's Center default the arm paints NONE of the five labels",
    );

    let (served, declined) = census(&header_strip(TextAlign::Start, None), &engine);
    assert_eq!(
        (served, declined),
        (HEADERS.len(), 0),
        "and under Start it paints all five — so the shift R1504 caused is \
         the declaration's doing, not an unrelated ineligibility",
    );
}

/// R1510 — the arm holds ONE face, so a leaf that declares a weight the face
/// does not have must not be served: it would be painted Regular and the
/// declaration would be silently lost. Measured before the predicate was
/// taught this: `self_hosted_text_eligible` read alignment, line height,
/// decoration and runs, and never `font_weight`, so a `Start`-aligned bold
/// header label WAS the arm's — the R1505 defect (a declaration that does not
/// reach the glyphs) in a second channel.
///
/// `Start` is the load-bearing half of the fixture: under the toolkit's `Center`
/// default these labels leave the arm over the alignment anyway, so the
/// alignment has to be the one the arm accepts for the weight to be what
/// decides.
#[test]
fn a_weight_the_single_face_cannot_serve_leaves_the_arm() {
    let engine = engine();
    let (plain, _) = census(
        &header_strip_selected(TextAlign::Start, None, SectionSelection::Unselected),
        &engine,
    );
    assert_eq!(
        plain,
        HEADERS.len(),
        "an unhighlighted Start strip is entirely the arm's — the baseline the \
         next assertion moves away from",
    );
    for selection in [SectionSelection::Partial, SectionSelection::Full] {
        let (served, declined) = census(
            &header_strip_selected(TextAlign::Start, None, selection),
            &engine,
        );
        assert_eq!(
            (served, declined),
            (0, HEADERS.len()),
            "{selection} bolds every label, and a bold leaf must defer to parley, \
             which selects a real bold face",
        );
    }
}

/// ★★★★★ R1952 — **sorting a strip no longer changes what the text arm is
/// asked for, because the indicator stopped being text.**
///
/// This test used to be `a_sorted_strip_is_mixed_because_the_arrow_declares_
/// nothing`, and it was right about a real thing: the arrow was a leaf that
/// declared no alignment, so it stayed the self-hosted arm's while every label
/// beside it went to parley, and a census counting only labels would have
/// reported a uniform strip that was not uniform.
///
/// R1952 asked the face this tree ships whether it has a glyph for `U+25B2`
/// and `U+25BC`. It does not — `NotoSans-Regular` is a Latin/Greek/Cyrillic
/// text face — so that leaf had been painting a `.notdef` box in every sorted
/// header in this tree, and it is a drawn mark now
/// (`pinion_widget_paint::indicator`). A path is not text, so it reaches
/// neither shaper.
///
/// The assertion is kept, pointing the other way, because the property it
/// protects is the same one: *a sorted strip and an unsorted strip must give
/// the same answer*, and the way to be wrong about that is for a header to
/// grow a leaf nobody counted.
#[test]
fn sorting_a_strip_does_not_change_what_the_arm_is_asked_for() {
    let engine = engine();
    let sorted = census(&header_strip(TextAlign::Center, Some(2)), &engine);
    assert_eq!(
        sorted,
        (0, HEADERS.len()),
        "a Center strip is entirely parley's, and the indicator is not text",
    );
    assert_eq!(
        sorted,
        census(&header_strip(TextAlign::Center, None), &engine),
        "and the sorted strip asks for exactly what the unsorted one does",
    );
}

/// Every non-`Start` alignment leaves the arm, `Justify` included — it reads
/// like `Start` on the single line the arm renders and is the easy one to wave
/// through.
#[test]
fn no_alignment_but_start_is_the_arms() {
    let engine = engine();
    for align in [TextAlign::Center, TextAlign::End, TextAlign::Justify] {
        let (served, _) = census(&header_strip(align, None), &engine);
        assert_eq!(served, 0, "{align:?} must leave the arm");
    }
}
