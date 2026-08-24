//! R1656 §5.32 §5.36 §2 #7 — **did what the frame painted stay inside the box
//! it was promised?**
//!
//! # The half-fact this closes
//!
//! A rectangle in a scene is a *promise*: this mark will be drawn here, in this
//! much room. Every read in this tree reports the promise. Nothing reported
//! whether it was kept, so a screen could paint a label across the row below it
//! — or past the edge of the card that owns it — and describe itself as
//! correct to every gate, every test and every agent.
//!
//! That is not hypothetical. Measured on the analysis-tool screen at the size
//! it opens in: **seven of its eight node cards paint their last field row
//! outside the card**, three to five pixels below the border, and one link
//! label reaches eighty-two pixels past the right edge. A person saw it
//! immediately. The tree had four checks that could each have caught it and
//! none of them asked this question:
//!
//! * `scene/snapshot` reports the promised rectangles, which are all correct —
//!   the *ink* is what left the box, and the ink is not in the scene.
//! * `scene/text_painted`'s `overflows` compares a run's ink against **its own**
//!   box, which is a different question and answers `true` for 124 of 157 runs
//!   on that screen: an authored line box a few pixels shorter than the shaped
//!   line box is near-universal and benign, so the flag cannot discriminate.
//! * the smear gate groups runs by their nearest **tagged ancestor**, and a
//!   painter that places a card's contents as *siblings* of the card (which is
//!   what an absolutely-positioned canvas painter does) gives every one of them
//!   the canvas as their ancestor. "Is this run inside its owner" then means
//!   "is it inside the canvas", which is true of everything.
//! * `scene/pointer_reach` asks whether a widget can be pressed, which a run
//!   that escapes its card usually still can be.
//!
//! # The owner is the parent, and that is a demand on the tree
//!
//! A node is drawn inside its parent, because that is what a box means. That
//! is the whole rule, and it is only as honest as the tree: a painter that
//! emits `[Container(card), Text(id), Text(field), …]` as one flat sibling list
//! has thrown the containment away, and no walk can recover it.
//!
//! ★ A second rule was written and then **measured and deleted**. Tags in this
//! tree are addresses, so `lab.node.T-01.id` appears to say in its own name
//! that it is part of `lab.node.T-01`, and judging a mark against its longest
//! tag-prefix would have caught a flat painter without any restructuring. Run
//! against the real screen it produced two findings and **both were false**:
//! `lab.toolbar.zoom.out` is the button *beside* the readout `lab.toolbar.zoom`,
//! not content inside it. A dotted tag expresses grouping, not containment, and
//! a rule built on that reads a naming habit as a geometric promise.
//!
//! So the repair for a flat painter is to stop being one — to put the
//! containment back in the scene, where §2 #7 says the description of the
//! screen lives. This module then sees it. The limit is stated rather than
//! papered over: **a scene that lies about its structure gets a clean report
//! here**, and the round that found this fixed the painter rather than adding a
//! second channel to work around it.
//!
//! # Clipped is not better than smeared
//!
//! [`Fate::Smeared`] is a mark drawn on top of whatever is next to it.
//! [`Fate::Clipped`] is a mark whose overhang an enclosing clip cut away. They
//! are reported separately because the repairs differ — one is a layout fix,
//! the other is a policy choice (elide, wrap, scroll) — but neither is
//! acceptable silently: a clip turns "this label is too long" into "this label
//! ends here", and the reader cannot tell the difference.
//!
//! # What the reference toolkit can answer, measured at 6.11
//!
//! The geometry aggregates exist and the ink one does not. A widget's
//! `childrenRect` is the union of its children's **geometry** (measured: a
//! label given a 60x14 box reports 60x14 there while its text measures 251x17),
//! and a scene item's `childrenBoundingRect` likewise reports boxes. Ink is
//! available only one call at a time, from a font-metrics helper the caller has
//! to remember to invoke against a rectangle it has to fetch separately.
//! Nothing compares the two, nothing warns when a child leaves its parent, and
//! a child that does is silently clipped to it — `visibleRegion` reports the
//! *survivor*, not the loss. An external driver cannot ask at all.
//!
//! Here the whole answer is a pure function of the painted scene plus one
//! measurement closure, so it is one wire read, a paint-time warning, and a
//! gate every surface pays at boot.
//!
//! # Why the measurement is a closure
//!
//! The same reason [`crate::text_elide`] takes one: how wide a string is has
//! two legitimate answers in this project — shaped pixels on the GPU path,
//! terminal cells on the §2 #6 dual — and they cannot be reconciled. The
//! *policy* (which box owns which mark, and what counts as escaping it) is
//! shared here; the metric is handed in.

use std::collections::HashMap;

use crate::scene::{Rect, Scene, TextNode};
use crate::style::{Border, BorderPlacement, Chrome, ChromeEdge, ChromeRole};

/// The height a box must have to hold one line of a `px` face without the
/// glyphs leaving it — a **reservation**, not a measurement.
///
/// ★ R1656 — this exists because the commonest authoring mistake this module
/// catches is a box authored at the *font size*. A shaped line box is taller
/// than the face it holds (ascent, descent and leading are all above and below
/// the em), so a 12px label in a 12px box overflows by construction, and that
/// is why `scene/text_painted`'s "the ink is bigger than the box" answered
/// `true` for 124 of 157 runs on the first screen it was pointed at.
///
/// Deliberately **conservative** — it reserves more than any face this project
/// ships needs. That direction is the safe one: reserving too much wastes a
/// pixel, reserving too little paints over the neighbour. A caller that wants
/// the exact number for a real face asks the shaper
/// (`pinion_text::LayoutCache::ink_size`), which the wire read does; this is
/// for a view function, which is sync and has no cache.
#[must_use]
pub const fn line_box(px: u32) -> u32 {
    px * 3 / 2 + 2
}

/// How many pixels short of holding its own text a run's box is. Zero when it
/// is tall enough.
///
/// # This is a different question from the rest of this module
///
/// Everything else here asks **did a mark leave the box that owns it** — a
/// mark against its *parent*. This asks whether a run's own box was authored
/// tall enough for the face the run is set in, which is a mark against
/// *itself*, and the two do not imply each other: a toolbar is easily big
/// enough to hold a 13-pixel button whose own label box is five pixels too
/// short, so `scene/containment` answers *escapes 0* while the descender of
/// every `g` in it is destroyed. A reader reported exactly that twice, eleven
/// days apart, and between the two reports this tree had every number needed
/// to answer them and no predicate that asked.
///
/// # It needs no font, and that is the point
///
/// `line_box` is a reservation computed from the face size alone, so this is a
/// pure function of the scene: no shaper, no host font, no measured ink. That
/// makes it usable where the escape check is not — in a sync `view` function,
/// at boot, and in a gate that cannot disagree between this machine and CI
/// because there is nothing machine-dependent in it.
///
/// # Multi-line
///
/// A box must hold one line box per visual line. `lines` is the measured
/// sidecar and is `0` before any shape pass, which is read as one line: the
/// floor of the demand rather than a guess at it, so an un-laid-out tree is
/// judged conservatively instead of arbitrarily.
#[must_use]
pub const fn short_by(text: &TextNode) -> u32 {
    let lines = if text.line_count == 0 {
        1
    } else {
        text.line_count
    };
    let needs = line_box(text.style.font_size_px).saturating_mul(lines);
    needs.saturating_sub(text.rect.h)
}

/// A box that holds one line of a `px` face — so [`short_by`] of a run placed
/// in it is `0` by construction.
///
/// ★★★★★ R1800 — the rule and the way to satisfy it, in one module, because
/// **the measurement said the rule was the problem**. Pointed at the screen
/// whose clipped descender a reader reported, [`short_boxes`] answered **289 of
/// 290 runs**: not 289 authoring slips but one convention, applied almost
/// everywhere, that never consulted the face. The framework has owned
/// `line_box` since R1656 and exactly one production site in this tree sizes
/// anything with it.
///
/// ⚠ That denominator was measured only because the gate was made to print it.
/// This doc said "289 of 289" first — a numerator with a guessed denominator,
/// written into five files before the closing audit caught it. Two runs on that
/// screen do hold their text.
///
/// A constant nobody can reach for is a constant nobody uses. Reaching for this
/// is easier than writing a number, which is the only reliable way a rule gets
/// kept — the alternative is a gate that scolds 289 times and a person who
/// turns it off.
#[must_use]
pub const fn line_rect(x: u32, y: u32, w: u32, px: u32) -> Rect {
    Rect::new(x, y, w, line_box(px))
}

/// The same box, centred vertically inside `outer`.
///
/// The second half of the same defect: a run's vertical position in this tree
/// is a hand-picked offset too, so a box can be tall enough and still sit low
/// enough to look wrong. Five chips measured on one screen were placed with a
/// `+4` where centring the box wanted `3` and centring the ink wanted `2`, and
/// the reader's words for it were "the text is all pushed to the bottom".
///
/// Takes `outer` rather than a height so the caller cannot centre in the wrong
/// thing by transposing two arguments.
#[must_use]
pub const fn line_rect_in(outer: Rect, x: u32, w: u32, px: u32) -> Rect {
    let h = line_box(px);
    Rect::new(x, outer.y + (outer.h.saturating_sub(h)) / 2, w, h)
}

/// One run whose own box cannot hold it, as [`short_boxes`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortBox {
    /// The run's tag, when it carries one.
    pub tag: Option<String>,
    /// The path to it, for a reader who has to find it.
    pub path: Vec<String>,
    /// What it says.
    pub content: String,
    /// The box as authored, in its scroll frame.
    pub rect: Rect,
    /// The face size the run is set in.
    pub px: u32,
    /// Visual lines as the scene records them, `0` when no shape pass has run.
    /// Reported verbatim rather than normalised, so a reader can tell "one
    /// line" from "nobody has measured yet".
    pub lines: u32,
    /// The height the box needed.
    pub needs: u32,
    /// `needs - rect.h`, always positive here.
    pub short_by: u32,
}

/// Every run in the scene whose own box is too short for the face it is set in.
///
/// Reports the *amount* per run rather than a count, for the reason
/// [`Overhang`] carries four numbers instead of a boolean: R1656 measured the
/// nearest available flag — `scene/text_painted`'s `overflows` — as true for
/// 124 of 157 runs on the first screen it was aimed at, and abandoned the axis
/// because a signal that fires on four fifths of a screen cannot discriminate.
/// That measurement was of *ink against the box*, where a one-pixel overshoot
/// is the shaper being one pixel more generous than the author reserved. This
/// asks the authoring question instead, so a run is short only when its box
/// could not have held the line under any shaping.
///
/// No clip is folded in, deliberately: a box authored too short is authored too
/// short whether or not something downstream then hides the evidence.
#[must_use]
pub fn short_boxes(scene: &Scene) -> Vec<ShortBox> {
    let mut found = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Scene::Text(t) = visit.node else {
            return;
        };
        // A run with no box makes no promise about holding anything.
        if t.rect.h == 0 || t.rect.w == 0 {
            return;
        }
        let short = short_by(t);
        if short == 0 {
            return;
        }
        let lines = if t.line_count == 0 { 1 } else { t.line_count };
        found.push(ShortBox {
            tag: visit.node.tag().map(str::to_owned),
            path: visit.path.to_vec(),
            content: t.content.clone(),
            rect: t.rect,
            px: t.style.font_size_px,
            lines: t.line_count,
            needs: line_box(t.style.font_size_px).saturating_mul(lines),
            short_by: short,
        });
    });
    found
}

/// How far a mark reached past the box that owns it, per edge, in pixels.
///
/// Four numbers rather than one boolean because the boolean was measured
/// useless: on the screen this module was written for, "the ink is bigger than
/// the box" is true of 79% of runs. *How much* and *which edge* is what tells a
/// three-pixel line-box rounding from a row painted over its neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overhang {
    /// Pixels past the owner's left edge.
    pub left: u32,
    /// Pixels past the owner's top edge.
    pub top: u32,
    /// Pixels past the owner's right edge.
    pub right: u32,
    /// Pixels past the owner's bottom edge.
    pub bottom: u32,
}

impl Overhang {
    /// Nothing escaped.
    pub const NONE: Self = Self {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    /// How far `inner` reaches past `outer` on each edge.
    #[must_use]
    pub fn of(inner: Rect, outer: Rect) -> Self {
        Self {
            left: outer.x.saturating_sub(inner.x),
            top: outer.y.saturating_sub(inner.y),
            right: (inner.x + inner.w).saturating_sub(outer.x + outer.w),
            bottom: (inner.y + inner.h).saturating_sub(outer.y + outer.h),
        }
    }

    /// True when the mark stayed inside.
    #[must_use]
    pub const fn is_contained(&self) -> bool {
        self.left == 0 && self.top == 0 && self.right == 0 && self.bottom == 0
    }

    /// The largest single-edge overhang — what a budget counts and what a
    /// tolerance compares against.
    #[must_use]
    pub const fn worst(&self) -> u32 {
        let a = if self.left > self.top {
            self.left
        } else {
            self.top
        };
        let b = if self.right > self.bottom {
            self.right
        } else {
            self.bottom
        };
        if a > b { a } else { b }
    }
}

/// What happened to the part that did not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// Nothing cut it: it is painted on top of whatever is beside the owner.
    Smeared,
    /// An enclosing clip removed it. The reader loses the content with no mark
    /// that anything was removed — which is why this is reported rather than
    /// forgiven.
    Clipped,
}

impl Fate {
    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Smeared => "smeared",
            Self::Clipped => "clipped",
        }
    }
}

/// One painted mark that did not stay inside the box that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escape {
    /// The mark's own tag, when it has one. A text run usually does not, which
    /// is exactly why this class stayed invisible: every other gate here is
    /// tag-keyed.
    pub tag: Option<String>,
    /// The mark's address, as `scene/locate` reports it.
    pub path: Vec<String>,
    /// The tag of the box it escaped, or `<untagged>` when that box has no tag.
    pub owner: String,
    /// What the mark holds, for a text run — the string a reader is losing.
    pub content: Option<String>,
    /// Window-absolute box the scene promised the mark.
    pub promised: Rect,
    /// Window-absolute extent actually painted: the shaped ink for a text run,
    /// the promised box for anything else.
    pub painted: Rect,
    /// Window-absolute box of the owner.
    pub owner_rect: Rect,
    /// How far past each edge of the owner the paint reached.
    pub over: Overhang,
    /// Whether the overhang is cut by a clip or drawn over the neighbour.
    pub fate: Fate,
    /// R1674 — **which parts of the owner** the mark landed on: its outer edge,
    /// its border, or a named chrome band. Never empty for an escape, because
    /// leaving the content rectangle means landing on at least one of them.
    pub trespass: Vec<Trespass>,
}

/// How wide and tall a text run's glyphs actually are, in the caller's unit.
///
/// Handed in rather than computed here for the reason the module header gives:
/// the GPU path measures shaped pixels and the §2 #6 terminal measures cells,
/// and no single answer is right for both.
pub type InkOf<'a> = &'a mut dyn FnMut(&TextNode) -> (u32, u32);

/// A node's **content** rectangle: its box, less any border it draws inside it.
///
/// The distinction CSS calls border box versus content box. A child is judged
/// against this rather than against the box, because a border is ink the owner
/// owns and a child that covers it has left the region it was given even though
/// it is inside the outer rectangle.
///
/// ★ R1672 — **public, because the placement side needs the same answer.** A
/// pane that hands its scrolling body its own rectangle puts the body over its
/// outline, and a caller computing the inset itself would be a second copy of
/// this rule free to disagree with the check that reports it. Two screens and a
/// widget were doing exactly that, measured at 13 escapes the moment this
/// channel learned the distinction.
///
/// # Against the floor, measured by running it
///
/// The reference toolkit at 6.11 **has** this concept and derives it the same
/// way: a framed widget with a 2px line reports content margins of `2,2,2,2`
/// and a content rect of `(2, 2, 96, 36)` inside a `100x40` box, and a titled
/// group box reports `3,23,3,3` — its caption band included. So this is parity
/// on the *rectangle*, and one limit of ours is worth stating: only the border
/// is subtracted here, because a caption band is a widget's own layout decision
/// and the scene has no vocabulary for it.
///
/// What the floor has no answer for is the question this module exists to ask.
/// Probed there: nothing reports whether a mark actually **left** the content
/// rect. A painter is free to draw outside it and no API says so; there is no
/// per-edge overhang, no owner attribution, and `childrenRect` answers about
/// child *widgets* rather than about painted ink. Here the content rectangle
/// and the report that a mark crossed it are the same function's two halves.
#[must_use]
pub fn content_rect(node: &Scene, box_rect: Rect) -> Rect {
    let (border, chrome) = box_chrome(node);
    content_of(box_rect, border, chrome)
}

/// The border and the declared chrome bands of a node that has a
/// [`BoxStyle`](crate::style::BoxStyle),
/// or `(None, &[])` for one that does not.
///
/// One place reads the style, so [`content_rect`] and the trespass attribution
/// cannot disagree about which nodes have chrome.
fn box_chrome(node: &Scene) -> (Option<&Border>, &[Chrome]) {
    match node {
        Scene::Box(n) => (n.style.border.as_ref(), &n.style.chrome),
        Scene::Container(n) => (n.style.border.as_ref(), &n.style.chrome),
        _ => (None, &[]),
    }
}

/// The content rectangle of a box that strokes `border` inside itself and keeps
/// `chrome` bands of itself for itself.
///
/// The arithmetic half of [`content_rect`], for the side that is **placing**
/// children and so has the style before it has the node. Pass `None` and `&[]`
/// for a box that draws no frame and reserves nothing, and the rectangle comes
/// back unchanged.
///
/// ★★ R1673 — lifted at the tenth consumer, and the count is the argument. Three
/// screens had written the same `const fn panel_content(rect) -> Rect` by hand
/// with the inset spelled `1`, and a full re-measurement then found seven more
/// surfaces owing the same repair. A rule with ten independent implementations
/// is ten chances for one of them to disagree with the check that reports it —
/// and this one is *already* the check, which is the strongest case for a lift
/// there is: the placement and the judgement are now the same arithmetic.
///
/// ★★ R1674 — `chrome` joined it as a **required** argument rather than a second
/// entry point. A `content_of_with_chrome` beside this would let a caller
/// holding a style with bands ask the question that ignores them and get an
/// answer that looks right, which is the two-copies failure the paragraph above
/// records, re-created by an API shape. Every caller now states its chrome, and
/// `&[]` is a statement.
///
/// The bands are subtracted **after** the border, because that is where a
/// painter draws them: a titled frame strokes its outline on the box and then
/// lays its caption inside it. Two bands on one edge sum.
#[must_use]
pub fn content_of(box_rect: Rect, border: Option<&Border>, chrome: &[Chrome]) -> Rect {
    let inset = border.map_or(0, border_inset);
    let mut rect = Rect::new(
        box_rect.x + inset,
        box_rect.y + inset,
        box_rect.w.saturating_sub(inset * 2),
        box_rect.h.saturating_sub(inset * 2),
    );
    for band in chrome {
        rect = split_band(rect, *band).1;
    }
    rect
}

/// A band's own rectangle inside `rect`, and what is left for the content.
///
/// The single implementation of "where does this band sit": [`content_of`]
/// takes the remainder and [`trespasses`] takes the band, so the rectangle a
/// trespass is attributed to is by construction the rectangle the content was
/// denied. A band wider than what is left takes all of it and leaves an empty
/// rectangle **on the far side**, which is where a caller placing children next
/// would want the origin.
const fn split_band(rect: Rect, band: Chrome) -> (Rect, Rect) {
    let taken_h = if band.extent < rect.h {
        band.extent
    } else {
        rect.h
    };
    let taken_w = if band.extent < rect.w {
        band.extent
    } else {
        rect.w
    };
    match band.edge {
        ChromeEdge::Top => (
            Rect::new(rect.x, rect.y, rect.w, taken_h),
            Rect::new(rect.x, rect.y + taken_h, rect.w, rect.h - taken_h),
        ),
        ChromeEdge::Bottom => (
            Rect::new(rect.x, rect.y + rect.h - taken_h, rect.w, taken_h),
            Rect::new(rect.x, rect.y, rect.w, rect.h - taken_h),
        ),
        ChromeEdge::Left => (
            Rect::new(rect.x, rect.y, taken_w, rect.h),
            Rect::new(rect.x + taken_w, rect.y, rect.w - taken_w, rect.h),
        ),
        ChromeEdge::Right => (
            Rect::new(rect.x + rect.w - taken_w, rect.y, taken_w, rect.h),
            Rect::new(rect.x, rect.y, rect.w - taken_w, rect.h),
        ),
    }
}

/// The [`ChromeRole`] a node claims to be, or `None` for ordinary content.
///
/// Read from [`LayoutStyle::chrome_slot`](crate::style::LayoutStyle::chrome_slot)
/// through the node's layout sidecar, which every kind carries — a caption can
/// be a bare [`Scene::Text`] as easily as a container of one.
fn chrome_slot_of(node: &Scene) -> Option<ChromeRole> {
    node.layout_style().and_then(|layout| layout.chrome_slot)
}

/// The band `node` was given, when it claims one its parent actually reserved.
///
/// `None` for a node that claims nothing — the ordinary case — and also for one
/// whose claimed role the parent never declared. The second is deliberate: a
/// band that was not reserved was not taken from the content, so the content
/// rectangle is still the honest thing to judge that node against, and silently
/// exempting it instead would make a typo'd role into an exemption.
fn chrome_band_of(
    node: &Scene,
    owner_box: Rect,
    border: Option<&Border>,
    chrome: &[Chrome],
) -> Option<Rect> {
    let role = chrome_slot_of(node)?;
    let mut rect = content_of(owner_box, border, &[]);
    for band in chrome {
        let (band_rect, remainder) = split_band(rect, *band);
        if band.role == role {
            return Some(band_rect);
        }
        rect = remainder;
    }
    None
}

/// Whether two rectangles share at least one pixel. Zero-extent rectangles
/// cover no pixels, so they intersect nothing — a zero-height band was never
/// taken from the content and cannot be trespassed on.
const fn overlaps(a: Rect, b: Rect) -> bool {
    a.w > 0
        && a.h > 0
        && b.w > 0
        && b.h > 0
        && a.x < b.x + b.w
        && b.x < a.x + a.w
        && a.y < b.y + b.h
        && b.y < a.y + a.h
}

/// What a mark that left the content rectangle actually landed on.
///
/// The floor conflates all of these into "outside the content rect": probed at
/// 6.11, a widget publishes its reservation as four integers and reading them
/// back cannot say which pixels were frame and which were caption. Naming the
/// part is what turns *"this label is out of bounds"* into *"this label is over
/// the title"*, and the two have different repairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trespass {
    /// Past the owner's outer edge entirely — the mark is not in the box at all.
    Outside,
    /// On the border the owner strokes inside its own box.
    Border,
    /// In a band the owner reserved for its own chrome, named by its role.
    Chrome(ChromeRole),
}

impl Trespass {
    /// The word that rides on the wire. A chrome band is `chrome:<role>`, so
    /// one string sorts the three cases and a reader never has to join two
    /// fields to know what was hit.
    #[must_use]
    pub fn wire_word(self) -> String {
        match self {
            Self::Outside => "outside".to_owned(),
            Self::Border => "border".to_owned(),
            Self::Chrome(role) => format!("chrome:{}", role.wire_word()),
        }
    }
}

/// Every part of `owner_box` this mark landed on that was not content, in the
/// order a painter laid them: the outer edge first, then the border, then the
/// chrome bands in declaration order.
///
/// A mark can hit more than one — a header label drawn full-bleed sits on the
/// caption band *and* on the border beside it — and the list says so rather
/// than picking a winner, because the repairs are independent.
fn trespasses(
    painted: Rect,
    owner_box: Rect,
    border: Option<&Border>,
    chrome: &[Chrome],
) -> Vec<Trespass> {
    let mut found = Vec::new();
    if !Overhang::of(painted, owner_box).is_contained() {
        found.push(Trespass::Outside);
    }
    let inside_border = content_of(owner_box, border, &[]);
    // The border ring is what the box has and the inside does not. Testing the
    // four strips separately rather than "in the box but not in the ring"
    // because a mark can sit entirely within one strip.
    if inside_border != owner_box {
        let ring = [
            Rect::new(
                owner_box.x,
                owner_box.y,
                owner_box.w,
                inside_border.y - owner_box.y,
            ),
            Rect::new(
                owner_box.x,
                inside_border.y + inside_border.h,
                owner_box.w,
                (owner_box.y + owner_box.h).saturating_sub(inside_border.y + inside_border.h),
            ),
            Rect::new(
                owner_box.x,
                owner_box.y,
                inside_border.x - owner_box.x,
                owner_box.h,
            ),
            Rect::new(
                inside_border.x + inside_border.w,
                owner_box.y,
                (owner_box.x + owner_box.w).saturating_sub(inside_border.x + inside_border.w),
                owner_box.h,
            ),
        ];
        if ring.iter().any(|strip| overlaps(painted, *strip)) {
            found.push(Trespass::Border);
        }
    }
    let mut rect = inside_border;
    for band in chrome {
        let (band_rect, remainder) = split_band(rect, *band);
        if overlaps(painted, band_rect) {
            found.push(Trespass::Chrome(band.role));
        }
        rect = remainder;
    }
    found
}

/// How many pixels of the box a border's own stroke covers, per edge.
///
/// A `match` over every placement rather than a test for one of them, so a
/// placement added to [`BorderPlacement`] lands here as a compile error instead
/// of silently taking the "nothing" branch. R1672's first draft *was* that test
/// (`if placement != Inside { return box_rect }`) and it got
/// [`BorderPlacement::Center`] wrong: a centred stroke straddles the edge, so
/// half of it is inside the box and a child laid at the box covers that half.
const fn border_inset(border: &Border) -> u32 {
    match border.placement {
        // The whole stroke is inside the box.
        BorderPlacement::Inside => border.width,
        // Half in, half out — and a partially covered pixel is covered, so the
        // half that is inside rounds UP.
        BorderPlacement::Center => border.width.div_ceil(2),
        // Drawn beyond the box; it takes nothing from the content.
        BorderPlacement::Outside => 0,
    }
}

/// Every painted mark that left the box that owns it, in paint order.
///
/// The walk is [`Scene::for_each_node`], so the geometry fold — enclosing
/// scroll offsets and clips — is the same one the tag resolver and the hit test
/// use. A caller cannot get a different answer by doing the arithmetic itself,
/// which is the failure R1653 recorded when three descents each folded their
/// own.
///
/// # Precondition
///
/// The scene has been through `pinion_runtime::compute_layout`: every node's
/// `rect` is in its enclosing scroll frame, not relative to its parent. That is
/// what the renderer and the hit test read, so it is the only frame in which
/// "inside" is the question a reader is asking. Handed an un-laid-out tree this
/// reports whatever the author happened to write down, which is why the
/// consumer-side tests here drive `view()` through the real layout pass rather
/// than asserting against hand-written rectangles.
///
/// `ink_of` is asked only about [`Scene::Text`] nodes. Every other kind paints
/// its own rectangle, so its promise and its paint are the same value and it
/// can only escape by being placed outside its owner — which is still worth
/// reporting, and is how a badge drawn past its card is caught.
#[must_use]
pub fn escapes(scene: &Scene, ink_of: InkOf<'_>) -> Vec<Escape> {
    // Pass 1 — every node's window-absolute rectangle, keyed by identity.
    //
    // A parent is always visited before its children, so a one-pass version
    // would work for the LOOKUP; it is two passes because reading a parent's
    // rectangle out of the child's own fold is what the first draft did and it
    // was wrong. `NodeVisit::offset` accumulates only what enclosing
    // [`Scene::Scroll`] nodes contribute — post-layout rectangles are already
    // in their scroll frame, not their parent's — so subtracting the parent's
    // origin back out (which is what "the parent's rect is in its parent's
    // frame" would require) double-counted it and reported overhangs in the
    // thousands of pixels for a glyph inside a button. Measured, on the first
    // run against a real screen.
    let mut absolute: HashMap<*const Scene, Rect> = HashMap::new();
    scene.for_each_node(&mut |visit| {
        absolute.insert(
            std::ptr::from_ref(visit.node),
            translate(visit.node.rect(), visit.offset),
        );
    });

    // Pass 2 — the judgment.
    let mut found = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Some(parent) = visit.ancestors.last() else {
            return; // the root answers to nothing
        };
        let Some(&owner_rect) = absolute.get(&std::ptr::from_ref(*parent)) else {
            return;
        };
        if owner_rect.w == 0 || owner_rect.h == 0 {
            // A parent with no extent is a grouping node, not a box: it makes
            // no promise about where its children go, so it cannot be broken.
            return;
        }
        // ★★ R1672 — against the owner's CONTENT rectangle: its box less the
        // border it draws inside that box. A border is ink the owner owns, so a
        // child painted at the owner's full width covers the outline and leaves
        // a gap in it — and until this round that was reported as contained,
        // because "inside the box" was the whole of the question.
        //
        // Found by a person looking at a window twice in one session, on two
        // different bands of the same card, and neither `scene/containment` nor
        // any screen's own gate could see either: CSS has had border box versus
        // content box from the beginning and nothing here expressed the second.
        //
        // `Outside` placement draws the border beyond the box, so it takes
        // nothing from the content; only an inside border does.
        let owner_box = owner_rect;
        let (border, chrome) = box_chrome(parent);
        // ★★ R1674 — WHICH rectangle this child was promised depends on what it
        // says it is. A node carrying a `chrome_slot` is the band itself, so it
        // is judged against the band; every other child is judged against what
        // is left once the bands are taken out. Two questions, because a single
        // one has to be wrong for one of the two populations: judging the title
        // against the content rectangle reports every titled frame in the tree
        // as broken, and exempting whatever is drawn in the band lets a label
        // that really did land on the caption through.
        let owner_rect = chrome_band_of(visit.node, owner_box, border, chrome)
            .unwrap_or_else(|| content_of(owner_box, border, chrome));
        if matches!(parent, Scene::Scroll(_)) {
            // A scroll's content is SUPPOSED to be bigger than the viewport —
            // that is what makes it scrollable. Judging it here reported a
            // world surface as a 4,476-pixel escape on the first real run, and
            // a check that fires on the normal case is a check nobody keeps.
            // Marks INSIDE that content are still judged against their own
            // boxes, which is where the question is meaningful.
            //
            // ★★ R1685 — and a box that clips because it declares
            // `Overflow::Hidden` gets NO such exemption, deliberately. The two
            // look alike (a child taller than its parent, by design in both
            // cases) and they differ in the only thing this module is about:
            // under a scroll the reader can still get to it, and under a
            // hidden box the content is GONE. So the scroll case is normal and
            // the hidden case is a loss, even when it is an intended one — the
            // module's own rule is that a clip must not silently swallow the
            // report, because "this label ends here" and "this label is too
            // long" look identical on screen. `reach` then says which marks
            // actually went, which is the actionable half.
            return;
        }
        // Where this mark sits with no clip folded in: `absolute_rect` answers
        // where it can be SEEN, and a mark whose overhang is entirely clipped
        // away has still been mis-placed. The clip is read separately below,
        // to decide the fate rather than to hide the escape.
        let promised = translate(visit.node.rect(), visit.offset);
        let (painted, content) = match visit.node {
            Scene::Text(t) => {
                let (w, h) = ink_of(t);
                (
                    Rect::new(promised.x, promised.y, w, h),
                    Some(t.content.clone()),
                )
            }
            _ => (promised, None),
        };
        if painted.w == 0 || painted.h == 0 {
            return; // nothing was drawn, so nothing left anything
        }
        let over = Overhang::of(painted, owner_rect);
        if over.is_contained() {
            return;
        }
        let fate = match visit.clip {
            Some(clip) if !Overhang::of(painted, clip).is_contained() => Fate::Clipped,
            _ => Fate::Smeared,
        };
        // ★ R1674 — attributed against the owner's BOX, because that is the
        // rectangle the parts divide up: the border ring and every chrome band
        // are inside it, and the content rectangle is what is left after both.
        //
        // A chrome node's own band is not a trespass by it — it was given that
        // band — so the band it fills is dropped from its own list. What
        // remains is what it reached beyond its band: the border it covered, or
        // a neighbouring band, or the outside of the box.
        let mut trespass = trespasses(painted, owner_box, border, chrome);
        if let Some(role) = chrome_slot_of(visit.node) {
            trespass.retain(|t| *t != Trespass::Chrome(role));
        }
        found.push(Escape {
            tag: visit.node.tag().map(str::to_owned),
            path: visit.path.to_vec(),
            owner: parent
                .tag()
                .map_or_else(|| UNTAGGED.to_owned(), str::to_owned),
            content,
            promised,
            painted,
            owner_rect,
            over,
            fate,
            trespass,
        });
    });
    found
}

/// ★★★★★ R1811 — **a box far larger than the one thing it holds**, which is
/// the question this module's other two do not ask.
///
/// [`escapes`] asks whether the ink left its box and [`short_boxes`] whether
/// the box is too small for the face. Both are "is the box big enough?" from
/// opposite sides. **Nothing asked whether it is too big**, and a reader looking
/// at the running application did: a status message reading *"Node Lab section"*
/// sat in a box 560 pixels wide because the width was a constant, and the
/// complaint was that the box was strangely wide — not that anything was lost.
///
/// # What this does NOT do, and the three measurements that settled it
///
/// It does not decide **which** boxes should be snug. A box larger than its
/// content is usually correct — a panel, a card, a canvas are all bigger than
/// what is in them — so the interesting population is "boxes whose size is a
/// claim about their content", and R1811 tried three times to derive that from
/// the scene and failed each time:
///
/// 1. *a box whose whole content is one text run.* Measured on the assembled
///    analysis tool: it reported a tree row 203px wider than its label and hex
///    cells 10px wider than their bytes — all correct, because a cell in a
///    column is sized by the column.
/// 2. *…and absolutely positioned, so the width was authored.* Measured: it
///    narrowed nothing. This tree's screens paint almost everything at absolute
///    rectangles by convention, so that flag does not separate an authored
///    width from a laid-out one **here**.
/// 3. *the box a reader actually complained about* — a status toast — is not a
///    one-run box at all. It holds a tone bullet and a label, so rule 1 never
///    reached the case it was invented for.
///
/// ⇒ **intent is not recoverable from geometry**, which is this repository's
/// recurring finding in its own shape: what a box's size MEANS is a thing an
/// author knows and the scene does not record. So this answers the measurable
/// half — *how much of this box does its content not use* — for every box, and
/// leaves choosing to the caller, who has the intent. A caller asks about the
/// boxes it means, and the reason it means them lives at that call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slack {
    /// The box's tag, when it has one.
    pub tag: Option<String>,
    /// Window-absolute box.
    pub box_rect: Rect,
    /// What the box holds, as text, when any of it is a run — the words a
    /// reader is looking at when they say the box is the wrong size.
    pub content: String,
    /// Window-absolute union of everything the box holds: the shaped ink for a
    /// text run, the painted rectangle for anything else.
    pub ink: Rect,
    /// Width the box holds beyond its content rectangle's ink.
    pub spare_w: u32,
    /// Height the box holds beyond its content rectangle's ink.
    pub spare_h: u32,
}

/// Every box that holds something, with how much of it that something leaves
/// unused.
///
/// # Precondition
///
/// [`escapes`]'s: the scene has been through `compute_layout`, so a rectangle
/// is where the renderer will put it. Handed an un-laid-out tree this reports
/// what somebody wrote down rather than what a reader will see.
///
/// The spare is measured against the **content** rectangle
/// ([`content_rect`]), not the box, so a border and a declared chrome band are
/// not counted as room the run failed to fill — they were never its to use.
///
/// A run wider than its box reports `0` spare rather than an underflow; that
/// direction is [`escapes`]'s question and is already answered there.
#[must_use]
pub fn slack(scene: &Scene, ink_of: InkOf<'_>) -> Vec<Slack> {
    let mut found = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Scene::Container(container) = visit.node else {
            return;
        };
        if container.children.is_empty() {
            return; // a spacer holds nothing, so it leaves nothing unused
        }
        // The same fold [`escapes`] uses — `NodeVisit::offset` carries only what
        // enclosing scroll nodes contribute, because a post-layout rectangle is
        // already in its scroll frame.
        let box_rect = translate(visit.node.rect(), visit.offset);
        let content = content_rect(visit.node, box_rect);
        let mut held: Option<Rect> = None;
        let mut said = String::new();
        for child in &container.children {
            // A child's rectangle is already in this box's frame; its INK is
            // the shaped extent for a run and the rectangle itself otherwise —
            // the same distinction `escapes` draws, for the same reason.
            let (w, h) = match child {
                Scene::Text(text) => {
                    if !said.is_empty() {
                        said.push(' ');
                    }
                    said.push_str(&text.content);
                    ink_of(text)
                }
                other => (other.rect().w, other.rect().h),
            };
            if w == 0 || h == 0 {
                continue;
            }
            let at = child.rect();
            let here = Rect::new(box_rect.x + at.x, box_rect.y + at.y, w, h);
            held = Some(match held {
                Some(so_far) => so_far.union(here),
                None => here,
            });
        }
        let Some(ink) = held else {
            return; // nothing was drawn, so the box holds no claim to check
        };
        found.push(Slack {
            tag: container.tag.as_ref().map(ToString::to_string),
            box_rect,
            content: said,
            ink,
            spare_w: content.w.saturating_sub(ink.w),
            spare_h: content.h.saturating_sub(ink.h),
        });
    });
    found
}

/// What an escape names as its owner when the box that broke its promise
/// carries no tag. Spelled once so a caller filtering on it and this module
/// producing it cannot drift.
pub const UNTAGGED: &str = "<untagged>";

/// Fold a container-local rectangle into the walk root's frame.
///
/// Saturating rather than wrapping: R1653 measured what an underflow here costs
/// — a pan to the left turned a `u32` subtraction into a coordinate near four
/// billion, which is a panic in a debug build and a silently absurd rectangle
/// in a release one.
fn translate(rect: Rect, offset: (i64, i64)) -> Rect {
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "clamped into u32's range on the line above the cast"
    )]
    let fold =
        |v: u32, by: i64| -> u32 { (i64::from(v) + by).clamp(0, i64::from(u32::MAX)) as u32 };
    Rect::new(
        fold(rect.x, offset.0),
        fold(rect.y, offset.1),
        rect.w,
        rect.h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
    use crate::style::BoxStyle;

    /// A text run whose ink the fixture decides, so these tests are about the
    /// POLICY and never about a shaper.
    fn text(content: &str, rect: Rect, tag: Option<&'static str>) -> Scene {
        let node = TextNode::new(content, rect);
        Scene::Text(match tag {
            Some(t) => node.with_tag(t),
            None => node,
        })
    }

    fn boxed(rect: Rect, tag: &str, children: Vec<Scene>) -> Scene {
        let mut c = ContainerNode::new(children);
        c.rect = rect;
        c.tag = Some(tag.to_owned().into());
        Scene::Container(c)
    }

    /// Ink measured as `len * 8` wide by `12` tall — a stand-in with no font in
    /// it, so the assertions are about which box was compared against.
    fn stub_ink(t: &TextNode) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "fixture strings are a handful of characters"
        )]
        let w = (t.content.chars().count() as u32) * 8;
        (w, 12)
    }

    /// R1672 — ★★ a child that covers its owner's BORDER is an ESCAPE.
    ///
    /// R1671 pinned the opposite answer here, with a doc saying the test should
    /// be rewritten on the round that gave this channel a content box. This is
    /// that round, and the test is the rewrite.
    ///
    /// The distinction is CSS's border box versus content box, and it is not
    /// decorative: a person looking at a window reported the same defect twice
    /// in one session, on two bands of one card, and neither this channel nor
    /// any screen's own gate could see either — because "inside the box" was
    /// the whole of the question and a border lives inside the box.
    ///
    /// The overhang is reported per edge, so the repair (inset by the border)
    /// is legible from the report alone.
    #[test]
    fn r1672_a_mark_over_its_owners_border_is_an_escape() {
        use crate::style::{Border, Color};

        let strip = |x: u32, w: u32| {
            Scene::Box(
                BoxNode::new(
                    Rect::new(x, 10, w, 12),
                    BoxStyle::filled(Color::rgb(0x30, 0x30, 0x30)),
                )
                .with_tag("strip"),
            )
        };
        let framed = |child: Scene| {
            let mut frame = ContainerNode::new(vec![child]);
            frame.rect = Rect::new(0, 0, 100, 40);
            frame.tag = Some("frame".to_owned().into());
            frame.style = BoxStyle::filled(Color::rgb(0x10, 0x10, 0x10))
                .with_border(Border::new(Color::rgb(0xEC, 0x5A, 0xA0), 1));
            Scene::Container(frame)
        };

        // Exactly the owner's width: the fill covers both border columns.
        let found = escapes(&framed(strip(0, 100)), &mut stub_ink);
        assert_eq!(found.len(), 1, "the strip covers the frame: {found:?}");
        assert_eq!(found[0].owner, "frame");
        assert_eq!(
            found[0].owner_rect,
            Rect::new(1, 1, 98, 38),
            "the CONTENT box"
        );
        assert_eq!(
            found[0].over.left, 1,
            "one column each side, named per edge"
        );
        assert_eq!(found[0].over.right, 1);
        assert_eq!(found[0].over.top, 0);
        assert_eq!(found[0].over.bottom, 0);

        // Inset by the border: nothing to report. The rule is not "anything
        // touching the edge" — it is the border's own pixels.
        assert!(
            escapes(&framed(strip(1, 98)), &mut stub_ink).is_empty(),
            "a band inset by the frame is contained",
        );

        // And an owner with no border is judged against its box, unchanged:
        // this rule takes nothing away from a surface that draws no frame.
        let mut plain = ContainerNode::new(vec![strip(0, 100)]);
        plain.rect = Rect::new(0, 0, 100, 40);
        plain.tag = Some("plain".to_owned().into());
        assert!(
            escapes(&Scene::Container(plain), &mut stub_ink).is_empty(),
            "no border, no content inset",
        );
    }

    /// R1673 — the placement half and the judging half are one arithmetic.
    ///
    /// [`content_of`] is what a painter calls before it has a node, and
    /// [`content_rect`] is what the check calls after. If they could disagree,
    /// a screen could be laid out correctly by its own rule and reported wrong
    /// by ours — which is the failure three screens' hand-written
    /// `panel_content` was one edit away from at all times.
    #[test]
    fn r1673_placing_and_judging_read_the_same_content_box() {
        use crate::style::{Border, Color};

        let border = Border::new(Color::rgb(0xEC, 0x5A, 0xA0), 3);
        let mut framed = ContainerNode::new(Vec::new());
        framed.rect = Rect::new(10, 20, 100, 40);
        framed.style = BoxStyle::filled(Color::rgb(0x10, 0x10, 0x10)).with_border(border);
        let node = Scene::Container(framed);

        let box_rect = Rect::new(10, 20, 100, 40);
        assert_eq!(
            content_rect(&node, box_rect),
            content_of(box_rect, Some(&border), &[]),
            "the judging half and the placing half are one answer",
        );
        assert_eq!(
            content_of(box_rect, None, &[]),
            box_rect,
            "no border, no chrome, no inset"
        );
        assert_eq!(
            content_of(box_rect, Some(&border), &[]),
            Rect::new(13, 23, 94, 34)
        );

        // ★ R1674 — and the same identity holds once the box declares chrome,
        // which is the half that could have been added to one side only. A
        // titled frame is the case: the caption band is subtracted by the
        // painter placing children AND by the check judging them, or the two
        // disagree about the same twenty pixels.
        let caption = Chrome::caption(20);
        let mut titled = ContainerNode::new(Vec::new());
        titled.rect = box_rect;
        titled.style = BoxStyle::filled(Color::rgb(0x10, 0x10, 0x10))
            .with_border(border)
            .with_chrome(caption);
        assert_eq!(
            content_rect(&Scene::Container(titled), box_rect),
            content_of(box_rect, Some(&border), &[caption]),
            "chrome reaches the judging half too",
        );
        assert_eq!(
            content_of(box_rect, Some(&border), &[caption]),
            Rect::new(13, 43, 94, 14),
            "3px of border on every edge, then 20px of caption off the top",
        );
    }

    /// R1672 — how much of the box a border takes, for **every** placement.
    ///
    /// Written because the first draft of [`content_rect`] asked whether the
    /// placement was [`BorderPlacement::Inside`] and returned the box for
    /// anything else, which is right for [`BorderPlacement::Outside`] and wrong
    /// for [`BorderPlacement::Center`] — a centred stroke straddles the edge, so
    /// half its width is inside the box and a child laid at the box covers it.
    ///
    /// The population is a `match` over the enum, so a fourth placement is a
    /// compile error here rather than a silent fourth answer.
    #[test]
    fn r1672_each_border_placement_takes_its_own_share_of_the_box() {
        use crate::style::{Border, Color};

        let framed = |placement: BorderPlacement, width: u32| {
            let mut frame = ContainerNode::new(Vec::new());
            frame.rect = Rect::new(0, 0, 100, 40);
            frame.style = BoxStyle::filled(Color::rgb(0x10, 0x10, 0x10)).with_border(
                Border::new(Color::rgb(0xEC, 0x5A, 0xA0), width).with_placement(placement),
            );
            Scene::Container(frame)
        };
        let inset_of = |placement, width| {
            let node = framed(placement, width);
            content_rect(&node, Rect::new(0, 0, 100, 40)).x
        };

        let mut covered = 0;
        for placement in [
            BorderPlacement::Inside,
            BorderPlacement::Center,
            BorderPlacement::Outside,
        ] {
            covered += 1;
            let want = match placement {
                // The whole 4px stroke is in the box.
                BorderPlacement::Inside => 4,
                // Half of it is, and an odd width rounds up: a partly covered
                // pixel is covered.
                BorderPlacement::Center => 2,
                // None of it is.
                BorderPlacement::Outside => 0,
            };
            assert_eq!(inset_of(placement, 4), want, "{placement:?} at width 4");
        }
        assert_eq!(covered, 3, "the census covers every placement");
        assert_eq!(
            inset_of(BorderPlacement::Center, 3),
            2,
            "an odd centred stroke rounds its inside half UP",
        );
    }

    /// ★ The defect this module was written for, as a property: a card whose
    /// last row is painted below its own border.
    ///
    /// Stated against the ink rather than the box because that is the half that
    /// was missing — the row's promised rectangle can sit inside the card while
    /// the glyphs it holds do not.
    #[test]
    fn r1656_a_row_painted_past_the_card_border_is_reported() {
        let card = boxed(
            Rect::new(10, 10, 100, 40),
            "card",
            vec![text("row", Rect::new(14, 40, 40, 11), None)],
        );
        let found = escapes(&card, &mut stub_ink);
        assert_eq!(found.len(), 1, "one escape, not two: {found:?}");
        let escape = &found[0];
        assert_eq!(escape.owner, "card");
        assert_eq!(escape.content.as_deref(), Some("row"));
        // Row top at 40, ink 12 tall -> 52; the card's bottom edge is 10+40=50.
        assert_eq!(escape.over.bottom, 2, "{escape:?}");
        assert_eq!(escape.over.right, 0);
        assert_eq!(escape.fate, Fate::Smeared, "nothing clipped it");
    }

    /// ★ The stated limit, as a test: a painter that flattens its containment
    /// gets a CLEAN report, and that is not this module being wrong.
    ///
    /// This is the shape the analysis-tool canvas actually painted — the card
    /// and its parts as siblings — and it is why every check in the tree
    /// answered "contained" while a person could see text outside the border.
    /// The repair is to put the relation back in the scene (§2 #7 says the
    /// scene IS the description of the screen), not to guess it from a naming
    /// habit: a rule that judged a mark against its longest tag-prefix was
    /// written, measured against the real screen, and produced two findings
    /// that were both false — `lab.toolbar.zoom.out` is the button BESIDE the
    /// readout `lab.toolbar.zoom`, not content inside it.
    ///
    /// Asserting the blindness is what keeps it from being rediscovered as a
    /// surprise, and what makes the nesting repair in the consumer load-bearing
    /// rather than cosmetic.
    #[test]
    fn r1656_a_flat_painter_gets_a_clean_report_and_that_is_the_limit() {
        let flat = Scene::Container(ContainerNode::new(vec![
            Scene::Box(
                BoxNode::new(Rect::new(0, 0, 100, 40), BoxStyle::default()).with_tag("card"),
            ),
            // Painted well below the card, as a SIBLING of it.
            text("id", Rect::new(4, 44, 40, 11), Some("card.id")),
        ]));
        let root_rect = Rect::new(0, 0, 1000, 1000);
        let mut rooted = ContainerNode::new(match flat {
            Scene::Container(c) => c.children,
            other => vec![other],
        });
        rooted.rect = root_rect;
        let found = escapes(&Scene::Container(rooted), &mut stub_ink);
        assert!(
            found.is_empty(),
            "the scene says both are children of the root, and they are: {found:?}"
        );

        // The same two marks, with the relation the painter meant present in
        // the tree: now the escape is visible, and it is the only difference.
        let nested = boxed(
            Rect::new(0, 0, 100, 40),
            "card",
            vec![text("id", Rect::new(4, 44, 40, 11), None)],
        );
        let found = escapes(&nested, &mut stub_ink);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].owner, "card");
        assert_eq!(found[0].over.bottom, 16, "{:?}", found[0]);
    }

    /// ★ A mark inside its box is not reported, however tight the fit — the
    /// meter this replaces failed exactly here, reporting 79% of a screen.
    #[test]
    fn r1656_a_mark_that_fits_exactly_is_not_an_escape() {
        let card = boxed(
            Rect::new(0, 0, 24, 12),
            "card",
            vec![text("abc", Rect::new(0, 0, 24, 12), None)],
        );
        assert!(
            escapes(&card, &mut stub_ink).is_empty(),
            "3 chars * 8 = 24 wide, 12 tall, in a 24x12 box"
        );
    }

    /// ★ A clip does not make an escape acceptable, it changes what the reader
    /// loses: the content is gone with nothing saying so.
    #[test]
    fn r1656_a_clipped_overhang_is_reported_as_clipped() {
        let inner = boxed(
            Rect::new(0, 0, 40, 20),
            "card",
            vec![text("a much longer string", Rect::new(0, 0, 40, 12), None)],
        );
        let scroll = Scene::Scroll(crate::scene::ScrollNode::new(
            Rect::new(0, 0, 40, 20),
            inner,
        ));
        let found = escapes(&scroll, &mut stub_ink);
        let cut: Vec<_> = found.iter().filter(|e| e.fate == Fate::Clipped).collect();
        assert!(
            !cut.is_empty(),
            "the scroll's clip cuts the overhang, and that is still a loss: {found:?}"
        );
    }

    /// ★★ R1685 — a clip a *container* declares cuts the same way, and is
    /// reported the same way.
    ///
    /// The fate is read off [`NodeVisit::clip`], which since R1685 is folded
    /// from the node's own declaration rather than from its kind — so this
    /// arrives with no arm here mentioning containers at all. It is asserted
    /// because "it falls out" is a claim about code, and this is the behaviour.
    ///
    /// ★ And the same escape under a container that declares NOTHING is
    /// `Smeared`, not `Clipped`: the two rows differ only in the declaration,
    /// which is what makes this a test of the declaration.
    #[test]
    fn r1685_an_overflow_container_cuts_the_overhang_and_says_so() {
        let long = || text("a much longer string", Rect::new(0, 0, 40, 12), None);
        let cutting = {
            let mut node =
                ContainerNode::new(vec![boxed(Rect::new(0, 0, 40, 20), "card", vec![long()])]);
            node.rect = Rect::new(0, 0, 40, 20);
            node.layout =
                crate::style::LayoutStyle::new().with_overflow(crate::style::Overflow::Hidden);
            Scene::Container(node)
        };
        let smearing = {
            let mut node =
                ContainerNode::new(vec![boxed(Rect::new(0, 0, 40, 20), "card", vec![long()])]);
            node.rect = Rect::new(0, 0, 40, 20);
            Scene::Container(node)
        };

        let cut = escapes(&cutting, &mut stub_ink);
        assert!(
            cut.iter().any(|e| e.fate == Fate::Clipped),
            "the container declares the clip, so the overhang is cut and the \
             reader loses it silently: {cut:?}"
        );
        let smeared = escapes(&smearing, &mut stub_ink);
        assert!(
            smeared.iter().all(|e| e.fate == Fate::Smeared),
            "the same overhang under a container that declares nothing is \
             painted over its neighbours, not cut: {smeared:?}"
        );
        assert_eq!(
            cut.len(),
            smeared.len(),
            "the declaration changes the FATE of the escape, never whether it \
             is reported — a clip that hid the report would be the silence \
             this module exists to end"
        );
    }

    /// ★ A scroll's content is allowed to exceed its viewport, because that is
    /// what makes it scrollable — and the marks inside that content are still
    /// judged.
    #[test]
    fn r1656_scroll_content_is_not_an_escape_but_its_marks_still_are() {
        let card = boxed(
            Rect::new(0, 0, 100, 40),
            "card",
            vec![text("row", Rect::new(0, 44, 40, 11), None)],
        );
        let mut world = ContainerNode::new(vec![card]);
        world.rect = Rect::new(0, 0, 4000, 4000);
        let scroll = Scene::Scroll(crate::scene::ScrollNode::new(
            Rect::new(0, 0, 200, 200),
            Scene::Container(world),
        ));
        let found = escapes(&scroll, &mut stub_ink);
        assert_eq!(
            found.len(),
            1,
            "the 4000px world is not an escape; the row below its card is: {found:?}"
        );
        assert_eq!(found[0].owner, "card");
    }

    /// ★ The overhang is per edge, because which edge it is decides the repair.
    #[test]
    fn r1656_the_overhang_names_the_edge() {
        let over = Overhang::of(Rect::new(5, 5, 100, 100), Rect::new(10, 10, 50, 50));
        assert_eq!(over.left, 5);
        assert_eq!(over.top, 5);
        assert_eq!(over.right, 45);
        assert_eq!(over.bottom, 45);
        assert_eq!(over.worst(), 45);
        assert!(!over.is_contained());
        assert!(Overhang::of(Rect::new(10, 10, 5, 5), Rect::new(10, 10, 50, 50)).is_contained());
    }

    /// ★★ R1674 — a caption band is subtracted from the content, and the parity
    /// case is the floor's own numbers.
    ///
    /// Probed by building and running it: a titled group box at 6.11 reports
    /// content margins of `3,23,3,3` inside a `100x40` box while a plain framed
    /// widget with a 2px line reports `2,2,2,2`. A 3px border plus a 20px
    /// caption is that first answer exactly, which is the point — the RECTANGLE
    /// is parity, and what the floor cannot carry is which twenty of those
    /// twenty-three pixels are the title.
    #[test]
    fn r1674_a_caption_band_comes_out_of_the_content_rectangle() {
        let box_rect = Rect::new(0, 0, 100, 40);
        let border = Border::new(crate::style::Color::rgb(0x33, 0x33, 0x33), 3);
        let caption = Chrome::caption(20);
        assert_eq!(
            content_of(box_rect, Some(&border), &[caption]),
            Rect::new(3, 23, 94, 14),
            "3px of border on every edge, then 20 more off the top",
        );
        assert_eq!(
            content_of(box_rect, Some(&border), &[]),
            Rect::new(3, 23 - 20, 94, 14 + 20),
            "the same box with no caption keeps those twenty pixels",
        );
    }

    /// ★ Every edge, and two bands on one edge sum.
    ///
    /// A `match` over [`ChromeEdge`] decides where a band is taken from, and
    /// the arm that moves the ORIGIN (top, left) is a different arm from the
    /// one that only shortens (bottom, right) — the asymmetry R1672 got wrong
    /// once already on `BorderPlacement`, in a `match` that looked complete.
    #[test]
    fn r1674_a_band_is_taken_from_the_edge_it_names() {
        let r = Rect::new(10, 20, 100, 60);
        let cases = [
            (ChromeEdge::Top, Rect::new(10, 30, 100, 50)),
            (ChromeEdge::Bottom, Rect::new(10, 20, 100, 50)),
            (ChromeEdge::Left, Rect::new(20, 20, 90, 60)),
            (ChromeEdge::Right, Rect::new(10, 20, 90, 60)),
        ];
        for (edge, want) in cases {
            let band = Chrome::new(edge, 10, ChromeRole::Header);
            assert_eq!(content_of(r, None, &[band]), want, "{edge:?}");
        }
        // A panel carrying a tab strip above a toolbar spends both.
        assert_eq!(
            content_of(
                r,
                None,
                &[
                    Chrome::new(ChromeEdge::Top, 10, ChromeRole::TabStrip),
                    Chrome::new(ChromeEdge::Top, 6, ChromeRole::Toolbar),
                ],
            ),
            Rect::new(10, 36, 100, 44),
            "two bands on one edge sum",
        );
    }

    /// ★ A band bigger than what is left takes all of it and leaves the origin
    /// on the far side, rather than underflowing.
    ///
    /// The `.max(0)` shape R1668 measured as a four-billion-pixel underflow, in
    /// the arithmetic that decides where a child goes.
    #[test]
    fn r1674_an_oversized_band_empties_the_content_without_underflowing() {
        let r = Rect::new(0, 0, 40, 30);
        let got = content_of(r, None, &[Chrome::caption(500)]);
        assert_eq!(got, Rect::new(0, 30, 40, 0), "empty, at the bottom edge");
        let got = content_of(
            r,
            None,
            &[Chrome::new(ChromeEdge::Left, 500, ChromeRole::Gutter)],
        );
        assert_eq!(got, Rect::new(40, 0, 0, 30), "empty, at the right edge");
    }

    /// ★★★ R1674 — what the mark landed on, named. The field the floor has no
    /// form for.
    ///
    /// Probed at 6.11: a custom-painted widget publishes its reservation with
    /// its four-integer content-margin setter with `3, 23, 3, 3`, and reading
    /// it back yields four integers indistinguishable from a 3px border with
    /// 20 more on top. So
    /// "this label is over the title" and "this label is out of bounds" arrive
    /// there as the same answer, and the repairs are not the same repair.
    #[test]
    fn r1674_an_escape_says_which_part_of_the_owner_it_landed_on() {
        let border = Border::new(crate::style::Color::rgb(0x33, 0x33, 0x33), 2);
        let owner_style = BoxStyle::filled(crate::style::Color::TRANSPARENT)
            .with_border(border)
            .with_chrome(Chrome::caption(20));
        // A label dropped at the owner's origin: over the border AND the caption.
        let intruder = text("Endpoint", Rect::new(0, 0, 60, 12), Some("intruder"));
        let mut owner = ContainerNode::new(vec![intruder]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = owner_style.clone();
        let found = escapes(&Scene::Container(owner), &mut |t| (t.rect.w, t.rect.h));
        assert_eq!(found.len(), 1, "one intruder, one report");
        assert_eq!(
            found[0].trespass,
            vec![Trespass::Border, Trespass::Chrome(ChromeRole::Caption)],
            "it covered the outline and it landed on the title, and both are said",
        );

        // The same label placed in the CONTENT is not an escape at all.
        let good = text("Endpoint", Rect::new(2, 22, 60, 12), Some("good"));
        let mut owner = ContainerNode::new(vec![good]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = owner_style;
        assert!(
            escapes(&Scene::Container(owner), &mut |t| (t.rect.w, t.rect.h)).is_empty(),
            "the content rectangle starts below the caption",
        );
    }

    /// ★★★ R1674 — a node that IS the chrome is judged against its band, and a
    /// node that merely sits in the band is not.
    ///
    /// The two halves are useless apart. Without the claim, declaring a caption
    /// makes every titled frame in the tree report its own title as an escape;
    /// without the declaration, whatever is drawn up there is exempt and a
    /// label that really did land on the caption goes unreported. This asserts
    /// both directions against one owner, so a change that collapses them into
    /// one answer fails here.
    #[test]
    fn r1674_a_chrome_node_answers_to_its_band_and_content_does_not() {
        let style =
            BoxStyle::filled(crate::style::Color::TRANSPARENT).with_chrome(Chrome::caption(20));
        let ink = &mut |t: &TextNode| (t.rect.w, t.rect.h);

        // Claims the caption, fits the caption: contained.
        let title = Scene::Text(
            TextNode::new("Advanced", Rect::new(0, 2, 60, 14))
                .with_tag("title")
                .with_layout(
                    crate::style::LayoutStyle::new().with_chrome_slot(ChromeRole::Caption),
                ),
        );
        let mut owner = ContainerNode::new(vec![title]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = style.clone();
        assert!(
            escapes(&Scene::Container(owner), &mut *ink).is_empty(),
            "the title is judged against the band it was given",
        );

        // Claims the caption, OUTGROWS the caption: reported. This is the
        // defect R1673 found in a group box's legend by accident, and the
        // reason the claim is not simply an exemption.
        let tall = Scene::Text(
            TextNode::new("Advanced", Rect::new(0, 2, 60, 30))
                .with_tag("tall")
                .with_layout(
                    crate::style::LayoutStyle::new().with_chrome_slot(ChromeRole::Caption),
                ),
        );
        let mut owner = ContainerNode::new(vec![tall]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = style.clone();
        let found = escapes(&Scene::Container(owner), &mut *ink);
        assert_eq!(
            found.len(),
            1,
            "a caption too tall for its band is reported"
        );
        assert_eq!(found[0].over.bottom, 12, "2 + 30 past a 20px band");

        // Claims a role the owner never reserved: judged as ordinary content,
        // NOT exempted. A band that was never taken from the content was never
        // taken from the content, and a typo in a role must not become an
        // exemption.
        let liar = Scene::Text(
            TextNode::new("Advanced", Rect::new(0, 2, 60, 14))
                .with_tag("liar")
                .with_layout(crate::style::LayoutStyle::new().with_chrome_slot(ChromeRole::Footer)),
        );
        let mut owner = ContainerNode::new(vec![liar]);
        owner.rect = Rect::new(0, 0, 100, 60);
        owner.style = style;
        let found = escapes(&Scene::Container(owner), &mut *ink);
        assert_eq!(
            found.len(),
            1,
            "an unreserved role is not an exemption — it is content in the caption",
        );
        assert_eq!(
            found[0].trespass,
            vec![Trespass::Chrome(ChromeRole::Caption)]
        );
    }

    /// ★ The wire words are identity: two reads spell a role, and a rename
    /// would silently move an AI client's key on both.
    #[test]
    fn r1674_the_chrome_vocabulary_is_pinned_on_the_wire() {
        let roles = [
            (ChromeRole::Caption, "caption"),
            (ChromeRole::Header, "header"),
            (ChromeRole::TabStrip, "tab_strip"),
            (ChromeRole::Toolbar, "toolbar"),
            (ChromeRole::Gutter, "gutter"),
            (ChromeRole::Footer, "footer"),
        ];
        for (role, word) in roles {
            assert_eq!(role.wire_word(), word);
            assert_eq!(
                Trespass::Chrome(role).wire_word(),
                format!("chrome:{word}"),
                "one string carries the case and the role",
            );
        }
        for (edge, word) in [
            (ChromeEdge::Top, "top"),
            (ChromeEdge::Bottom, "bottom"),
            (ChromeEdge::Left, "left"),
            (ChromeEdge::Right, "right"),
        ] {
            assert_eq!(edge.wire_word(), word);
        }
        assert_eq!(Trespass::Outside.wire_word(), "outside");
        assert_eq!(Trespass::Border.wire_word(), "border");
    }
}
