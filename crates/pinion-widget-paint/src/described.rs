//! ★★★★★ R1918 §5.16 §5.38 §5.40 — **a screen's description surface, composed
//! once**: the box a sentence is drawn in, and the announcement that ties it to
//! the mark it belongs to.
//!
//! # What forced this, measured
//!
//! R1916 built [`pinion_core::describe`] — a screen's tag→sentence register and
//! the one derivation saying which sentence a reader is being shown. It left
//! the *surface* to the two screens that consumed it, and both wrote the same
//! four steps:
//!
//! 1. resolve the described mark's rectangle out of the paint register,
//! 2. place the box with [`pinion_core::describe::beside`],
//! 3. draw a filled, outlined box with one run inside it, deferring the run's
//!    voice to the box,
//! 4. find the anchor in the accessibility list, point it at the region with
//!    `aria-describedby`, and emit the region.
//!
//! Steps 2 and 4 already had one owner. Steps 1 and 3 did not, and R1918 needed
//! them at **three more bindings** — the assembled tool has six pages, four of
//! which said nothing at all under a resting cursor, and three of those four
//! are separate screens with their own paint. Five call sites of one shape is
//! not a rule-of-three question.
//!
//! # ⚠ What this deliberately does NOT own
//!
//! **Which mark carries which sentence** is the screen's, and must stay there:
//! the lab composes a pin's from its model, a list screen derives a column
//! header's from the specification the column is declared in. A substrate that
//! authored sentences would be a substrate that knows what a screen is about.
//!
//! **The inks and the face** are the screen's too, passed in. A description is
//! chrome drawn over a screen's own surface, and the palette it has to sit
//! legibly on is the screen's palette.
//!
//! # ⚠ Why the anchor is synthesised when it is missing
//!
//! [`announce_description`] emits the region even when the described mark
//! publishes no accessibility node of its own. The alternative — dropping the
//! description — would make the announced half silently disagree with the
//! painted half, and a reader resting on a mark would see a sentence no
//! assistive technology mentions. A synthesised anchor is a weaker node than a
//! screen's own; a missing one is a lie.

use pinion_a11y::{AccessNode, AriaRole};
use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Border, BoxStyle, Color, TextStyle};
use pinion_core::voice::Silence;

/// ★ R1918 — the inks a description is drawn in, which are the screen's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptionInk {
    /// The box's fill.
    pub surface: Color,
    /// Its hairline, or `None` for a box that carries no outline.
    pub outline: Option<Color>,
    /// The sentence's colour.
    pub ink: Color,
}

/// ★ R1918 — how big a description is drawn, in the screen's own units.
///
/// `face` is load-bearing rather than cosmetic: it is what
/// [`pinion_core::describe::beside`] derives the reserved width and the line box
/// from, so a caller that draws at one size and places at another would place a
/// box its own sentence does not fit in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptionStyle {
    /// The face the sentence is set in.
    pub face: u32,
    /// The clearance between the box's edge and its run.
    pub pad: u32,
    /// The box's corner radius.
    pub corner_radius: u32,
}

impl DescriptionStyle {
    /// The shape both R1916 consumers had written by hand.
    ///
    /// A `const` rather than a `Default` impl so a caller naming it is naming
    /// *this* shape rather than accepting whatever a later edit makes default.
    pub const COMPACT: Self = Self {
        face: 10,
        pad: 6,
        corner_radius: 6,
    };
}

/// ★★★★★ R1918 — **the description a reader is being shown, drawn beside the
/// mark it belongs to.**
///
/// `anchor` and `within` are in one space — whatever space the screen's paint
/// register answers in — and `origin` is the top-left of the space the returned
/// scene is laid out in. The two are the same for a screen that lays its chrome
/// out in window coordinates and differ for one that draws inside a pane, and
/// separating them is what lets both call this rather than one of them
/// re-deriving the placement in its own coordinates.
///
/// The run inside the box defers its voice to the box: the box *is* the region
/// in the accessibility tree ([`announce_description`] names it), so a run that
/// announced the same sentence would say it twice.
#[must_use]
pub fn view_description(
    region_tag: &str,
    sentence: &str,
    anchor: Rect,
    within: Rect,
    origin: (u32, u32),
    style: DescriptionStyle,
    ink: DescriptionInk,
) -> Scene {
    let (x, y, w, h) = pinion_core::describe::beside(
        (anchor.x, anchor.y, anchor.w, anchor.h),
        (within.x, within.y, within.w, within.h),
        sentence,
        style.face,
    );
    let box_rect = Rect::new(x.saturating_sub(origin.0), y.saturating_sub(origin.1), w, h);
    let mut box_style = BoxStyle::filled(ink.surface).with_corner_radius(style.corner_radius);
    if let Some(colour) = ink.outline {
        box_style = box_style.with_border(Border::new(colour, 1));
    }
    // ⚠★★★★★ The run is a CHILD of the box, so its rectangle is in the BOX's
    // space and starts at the padding — not at `box_rect.x + pad`.
    //
    // Worth the sentence, because the first draft of this lift wrote the
    // latter and the crate's frame gate caught it: both screens this module
    // was lifted from were correct and they were correct DIFFERENTLY. The node
    // lab painted a childless box with the run as its SIBLING, where
    // `box_rect.x + pad` is right; the shell nested the run and passed `pad`.
    // Lifting took the shell's nesting and the lab's coordinates, which double
    // the offset — measured as a 261px overhang at 900x600.
    //
    // ⚠ And neither screen's own suite could have found it: a description is
    // painted only while a reader rests on something, and no screen test rests
    // on anything. The crate gate runs the painter directly, which is exactly
    // the hit rate its module header claims for it.
    let run = crate::run::text_run(
        format!("{region_tag}.text"),
        sentence,
        Rect::new(
            style.pad,
            style.pad / 2,
            box_rect.w.saturating_sub(style.pad * 2),
            h.saturating_sub(style.pad / 2),
        ),
        TextStyle::new().with_size_px(style.face).with_fg(ink.ink),
    )
    .silenced(Silence::name_of(region_tag.to_owned()));
    Scene::Container(
        ContainerNode::new(vec![run])
            .with_tag(region_tag.to_owned())
            .with_style(box_style)
            .with_layout(
                pinion_core::style::LayoutStyle::new()
                    .with_absolute_position(box_rect.x, box_rect.y)
                    .with_size(pinion_core::style::Size::px(box_rect.w, box_rect.h)),
            ),
    )
}

/// ★★★★★ R1918 — **the announced half**: the described mark points at the
/// region, and the region carries the sentence.
///
/// Mutates `nodes` in place because the anchor is usually already in it and
/// must be *replaced* rather than duplicated — a second node under the same tag
/// is a tree with two answers for one mark. When the screen publishes no node
/// for the described mark, one is synthesised (see the module header for why
/// that is preferred to dropping the description).
pub fn announce_description(
    nodes: &mut Vec<AccessNode>,
    anchor_tag: &str,
    region_tag: &str,
    sentence: &str,
) {
    let control = match nodes.iter().position(|node| node.tag == anchor_tag) {
        Some(at) => nodes.remove(at),
        None => AccessNode::new(anchor_tag.to_owned(), AriaRole::Button),
    };
    nodes.extend(pinion_a11y::describedby_region(
        control,
        region_tag.to_owned(),
        AriaRole::Tooltip,
        Some(sentence.to_owned()),
        true,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::describe::{Descriptions, Resting};

    fn ink() -> DescriptionInk {
        DescriptionInk {
            surface: Color::rgb(20, 20, 20),
            outline: Some(Color::rgb(60, 60, 60)),
            ink: Color::rgb(230, 230, 230),
        }
    }

    fn tagged(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        collect(scene, &mut out);
        out
    }

    fn collect(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Container(node) => {
                if let Some(tag) = &node.tag {
                    out.push(tag.to_string());
                }
                for child in &node.children {
                    collect(child, out);
                }
            }
            Scene::Text(node) => {
                if let Some(tag) = &node.tag {
                    out.push(tag.to_string());
                }
            }
            _ => {}
        }
    }

    #[test]
    fn r1918_the_region_carries_the_tag_and_holds_one_run() {
        let scene = view_description(
            "s.tip",
            "what this does",
            Rect::new(100, 100, 20, 20),
            Rect::new(0, 0, 800, 600),
            (0, 0),
            DescriptionStyle::COMPACT,
            ink(),
        );
        assert_eq!(tagged(&scene), vec!["s.tip", "s.tip.text"]);
    }

    #[test]
    fn r1918_the_origin_moves_the_box_into_the_callers_space() {
        let anchor = Rect::new(300, 200, 20, 20);
        let within = Rect::new(200, 100, 400, 400);
        let window = view_description(
            "s.tip",
            "sentence",
            anchor,
            within,
            (0, 0),
            DescriptionStyle::COMPACT,
            ink(),
        );
        let local = view_description(
            "s.tip",
            "sentence",
            anchor,
            within,
            (within.x, within.y),
            DescriptionStyle::COMPACT,
            ink(),
        );
        let Scene::Container(window) = &window else {
            panic!("a description is a container");
        };
        let Scene::Container(local) = &local else {
            panic!("a description is a container");
        };
        let at = |node: &ContainerNode| {
            node.layout
                .absolute_position
                .expect("the box is placed absolutely")
        };
        let (wx, wy) = at(window);
        let (lx, ly) = at(local);
        assert_eq!(
            (wx - lx, wy - ly),
            (within.x, within.y),
            "the two spaces differ by exactly the origin"
        );
    }

    #[test]
    fn r1918_the_run_defers_its_voice_to_the_region() {
        let scene = view_description(
            "s.tip",
            "sentence",
            Rect::new(10, 10, 20, 20),
            Rect::new(0, 0, 800, 600),
            (0, 0),
            DescriptionStyle::COMPACT,
            ink(),
        );
        let Scene::Container(node) = &scene else {
            panic!("a description is a container");
        };
        let Scene::Text(run) = &node.children[0] else {
            panic!("the box holds one run");
        };
        assert!(
            run.layout.silence.is_some(),
            "the run must defer, or the sentence is announced twice"
        );
    }

    #[test]
    fn r1918_an_existing_anchor_is_replaced_and_points_at_the_region() {
        let mut nodes = vec![
            AccessNode::new("other", AriaRole::Button),
            AccessNode::new("grip", AriaRole::Button).with_name("Move"),
        ];
        announce_description(&mut nodes, "grip", "s.tip", "drag to move it");
        let grips: Vec<_> = nodes.iter().filter(|n| n.tag == "grip").collect();
        assert_eq!(grips.len(), 1, "one mark, one node");
        assert_eq!(
            grips[0].name.as_deref(),
            Some("Move"),
            "its own name is kept"
        );
        assert_eq!(grips[0].described_by.as_deref(), Some("s.tip"));
        let region = nodes
            .iter()
            .find(|n| n.tag == "s.tip")
            .expect("the region is emitted");
        assert_eq!(region.role, AriaRole::Tooltip);
        assert_eq!(region.name.as_deref(), Some("drag to move it"));
    }

    #[test]
    fn r1918_a_mark_with_no_node_of_its_own_still_gets_a_region() {
        // ★ The alternative is a description a sighted reader sees and an
        // assistive technology never hears, which is a worse state than a
        // weakly-named anchor.
        let mut nodes = Vec::new();
        announce_description(&mut nodes, "grip", "s.tip", "drag to move it");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].tag, "grip");
        assert_eq!(nodes[0].described_by.as_deref(), Some("s.tip"));
    }

    /// ★★ R1674 — the sentence stays inside the box this painter strokes, at
    /// both sizes. The crate gate ([`crate::frame_gate`]), which caught this
    /// module on the first suite run after it was written: R1918 wrote a
    /// bordered painter and did not think to ask, and the gate's population is
    /// derived from this crate's own sources rather than from a roster, so it
    /// noticed.
    ///
    /// ⚠ Run at sizes of the caller's choosing, because a description is
    /// **anchored rather than bound**: it hangs off a mark and is allowed to
    /// extend past whatever is behind it, so a window narrower than the box
    /// would report that a 200px sentence does not fit 180px — true, and not
    /// this gate's question. The two sizes still differ, which is the axis
    /// R1656 recorded as never having been one.
    ///
    /// Long sentences and short ones, because the box's width is DERIVED from
    /// the sentence and its padding is not: the case that would break is a
    /// short sentence in a box floored at 60 pixels.
    #[test]
    fn r1674_a_description_keeps_its_sentence_inside_its_box() {
        for sentence in [
            "a",
            "Drag to move this widget to another place on the board",
        ] {
            crate::frame_gate::assert_frame_contained_at(
                &format!("described {:?}", &sentence[..1]),
                &[(900, 600), (700, 420)],
                &mut |w, h| {
                    view_description(
                        "s.tip",
                        sentence,
                        Rect::new(w / 3, h / 3, 20, 20),
                        Rect::new(0, 0, w, h),
                        (0, 0),
                        DescriptionStyle::COMPACT,
                        ink(),
                    )
                },
            );
        }
    }

    #[test]
    fn r1918_the_register_and_the_surface_agree_on_one_derivation() {
        // The composition a screen performs, end to end: the register answers
        // WHICH sentence, and this module draws and announces THAT one.
        let mut described = Descriptions::new();
        described.describe("grip", "drag to move it");
        let shown = described
            .shown(&Resting::hovering("grip"))
            .expect("the register answers for a described mark");
        let mut nodes = Vec::new();
        announce_description(&mut nodes, shown.tag, "s.tip", shown.sentence);
        assert_eq!(nodes[1].name.as_deref(), Some("drag to move it"));
    }
}
