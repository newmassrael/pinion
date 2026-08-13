//! Whether a reader can ever SEE a mark, and what it would take.
//!
//! [`containment`](crate::containment) asks whether a mark stayed inside the
//! box that owns it. This module asks the question underneath that one: *can
//! the person at the screen get their eyes on it at all?* The two answers come
//! apart in both directions, which is why this is a separate derivation and not
//! another field on [`Escape`](crate::containment::Escape):
//!
//! - A row 240 pixels down a 300-pixel-tall pane whose viewport is 100 tall is
//!   perfectly contained in its owner and **cannot be seen right now**. Nothing
//!   is wrong; the reader scrolls.
//! - A row placed 400 pixels down that same pane is *also* only ever reported
//!   as an ordinary clip, and **no scrolling will ever reach it** — the pane's
//!   content extent stops at 300. That one is a defect of a kind the reader
//!   experiences as content that simply does not exist.
//!
//! Before this module those two got the same word. Measured on a synthetic pane
//! (`explore` in R1662's working notes, now the first two tests below): the
//! reachable row produced **no report at all**, and the unreachable one
//! produced `Fate::Clipped` — the same verdict a two-pixel line-box rounding
//! gets.
//!
//! # The window is a viewport with no range
//!
//! A mark with no clipping ancestor is judged against the window, whose scroll
//! range is zero. That is not a special case bolted on: it is the same
//! arithmetic with `max == (0, 0)`, and it is what makes "this pane does not
//! scroll, so its content is lost" and "this pane scrolls, so its content is
//! merely off-view" two values of one function instead of two unrelated
//! checks. Making a pane scrollable moves its overflow from [`Reach::Lost`] to
//! [`Reach::Scrollable`], and that move is the thing a gate can watch.
//!
//! # The node you chose is the declaration (R1685)
//!
//! Since R1685 a box can clip without scrolling —
//! [`Overflow::Hidden`](crate::style::Overflow::Hidden) — and the two kinds of
//! clip get different verdicts here, which is the point of having both. A
//! [`Scene::Scroll`] publishes a range a reader can move, so its overflow is
//! [`Reach::Scrollable`]. A hidden box publishes no range, so its overflow is
//! [`Reach::Lost`]: nothing brings it back.
//!
//! That is also why this module is the reason the workaround was worth
//! refusing. Before the declaration existed, a region that had to clip could
//! only do it by becoming a `Scroll` with a pinned offset — and this module,
//! which reads the range off the geometry, would have called every cut mark
//! "scrollable to y=152" while no gesture in the application could move it. A
//! false *reachable* is the exact error this module was written to end, and the
//! workaround would have manufactured it.
//!
//! # The reference cannot answer this
//!
//! Measured against the reference toolkit 6.11.1 (probe run out-of-tree, so
//! nothing here cites it): a scroll area derives its range from the laid-out
//! child and keeps it live — parity, and this tree has had that since R55.C via
//! `update_scroll_state_bounds`. What it has **no** surface for is the question
//! this module answers. Overflow can only be *inferred* by a consumer reading
//! `maximum() > 0`; there is no per-mark reachability, no "which offset shows
//! it", and an area that never sets its range reports `maximum() == 0` — the
//! byte-identical answer to a pane whose content genuinely fits. That last one
//! is precisely the analysis-tool panes' defect, and on the reference it is
//! undiagnosable from outside the widget.
//!
//! # Nesting
//!
//! A mark is judged against its **innermost** enclosing viewport only. Nested
//! scrolls compose by the chain being walked too: an inner scroll node is
//! itself a mark judged against *its* enclosing viewport, so a pane that can
//! reach all of its own content while sitting off the window reports one
//! [`Reach::Lost`] on the pane rather than one per row. The alternative —
//! folding every ancestor into one verdict per leaf — was not chosen because it
//! reports the same break N times and names the leaf rather than the pane,
//! which is the node the repair belongs to.

use std::collections::HashMap;

use crate::containment::{InkOf, Overhang, UNTAGGED};
use crate::scene::{Rect, Scene};
use crate::widgets::scroll::max_scroll_offset;

/// The name [`Viewport::name`] carries when the window itself is the viewport.
///
/// Spelled once so a caller filtering on it and this module producing it cannot
/// drift — the same reason [`UNTAGGED`] exists.
pub const WINDOW: &str = "<window>";

/// The box a mark was judged against, and how far it can move.
///
/// Published in full rather than reduced to a boolean because the numbers are
/// what a repair needs: a pane that is 40 pixels short of its content is a
/// different job from one that is 400 short, and "which pane" is the first
/// thing anyone asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewport {
    /// The enclosing scroll node's tag, or [`WINDOW`], or [`UNTAGGED`] for a
    /// scroll node that carries no tag.
    pub name: String,
    /// (R1685) Where this window sits **in the frame the marks it judges are
    /// expressed in**.
    ///
    /// `(0, 0)` for a [`Scene::Scroll`], whose content is stored in its own
    /// frame with the origin at the top-left — that is why every box below
    /// used to start at zero and why nothing needed this field.
    ///
    /// A box that clips because it declares
    /// [`Overflow::Hidden`](crate::style::Overflow::Hidden) introduces **no
    /// frame**: its children keep the coordinates its own rect is in, so its
    /// window starts where it does. Assuming zero there judged every label in
    /// a toolbar control against a box at the far left of the window and
    /// reported six of them lost — measured, on two real screens, by the
    /// ratchet that exists for it.
    pub origin: (u32, u32),
    /// The viewport's size — the window it shows onto the content.
    pub size: (u32, u32),
    /// The content's extent along each axis, as the laid-out subtree reports
    /// it. This is the value the reference exposes nowhere.
    pub content: (u32, u32),
    /// The offset the scene carries right now.
    pub at: (i32, i32),
    /// The largest offset each axis can take: `max(0, content - size)`, through
    /// the same [`max_scroll_offset`] the layout pass publishes with, so this
    /// derivation and the runtime's clamp cannot disagree.
    pub max: (i32, i32),
}

impl Viewport {
    /// True when the content needs no scrolling at all.
    ///
    /// The predicate the reference makes a consumer infer from `maximum() > 0`,
    /// which is also the answer it gives for an area that never set its range.
    #[must_use]
    pub const fn fits(&self) -> bool {
        self.max.0 == 0 && self.max.1 == 0
    }

    /// The box every offset in range can, between them, bring into view:
    /// `[origin, origin + max + size]` on each axis.
    ///
    /// Not `[0, content]`: when the content is smaller than the viewport the
    /// range is zero and the reachable box is the viewport, so a mark parked in
    /// the empty space below short content is still visible. Reading the bound
    /// off the content instead reported those as lost.
    #[must_use]
    pub fn reachable(&self) -> Rect {
        #[allow(
            clippy::cast_sign_loss,
            reason = "max is produced by max_scroll_offset, which clamps at 0"
        )]
        Rect::new(
            self.origin.0,
            self.origin.1,
            self.max.0 as u32 + self.size.0,
            self.max.1 as u32 + self.size.1,
        )
    }

    /// The box on screen at the current offset, in the frame its marks use.
    #[must_use]
    pub fn shown(&self) -> Rect {
        #[allow(
            clippy::cast_sign_loss,
            reason = "an offset is clamped into 0..=max before it reaches a scene"
        )]
        Rect::new(
            self.origin.0.saturating_add(self.at.0.max(0) as u32),
            self.origin.1.saturating_add(self.at.1.max(0) as u32),
            self.size.0,
            self.size.1,
        )
    }
}

/// What it would take to see a mark that is not on screen.
///
/// Two arms and not three: "it is on screen" is unrepresentable here because
/// [`out_of_sight`] does not report those marks at all. A record that could say
/// `Shown` would be a record every caller has to filter, and the filter is the
/// thing that gets forgotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The viewport's range covers it. This offset brings it in, moving as
    /// little as possible — the semantics measured off the reference's
    /// `ensureVisible`, which for a point 900 into a 198-tall viewport answers
    /// 702 rather than 900.
    ///
    /// For a mark larger than the viewport this is the offset that shows its
    /// leading edge; nothing can show all of it at once, and starting at the
    /// beginning is what every scroller does.
    Scrollable {
        /// The offset to scroll to, clamped into `0..=max`.
        to: (i32, i32),
    },
    /// No offset covers it: this much of it lies outside everything the
    /// viewport's range can ever show.
    ///
    /// On a pane that does not scroll — including every mark judged against the
    /// window — the range is zero, so this is simply "outside the box, and
    /// there is no way in".
    Lost {
        /// How far past the reachable box the mark reaches, per edge.
        short_by: Overhang,
    },
}

impl Reach {
    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Scrollable { .. } => "scrollable",
            Self::Lost { .. } => "lost",
        }
    }

    /// True for the arm a gate fails on.
    #[must_use]
    pub const fn is_lost(self) -> bool {
        matches!(self, Self::Lost { .. })
    }
}

/// One mark the reader cannot currently see, and whether that is recoverable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutOfSight {
    /// The mark's own tag, when it has one.
    pub tag: Option<String>,
    /// The mark's address, as `scene/locate` reports it.
    pub path: Vec<String>,
    /// What the mark holds, for a text run — the string a reader is losing.
    pub content: Option<String>,
    /// Where the mark sits in its viewport's content coordinates, using the
    /// shaped ink for a text run and the promised box for anything else.
    pub rect: Rect,
    /// The viewport it was judged against.
    pub viewport: Viewport,
    /// Whether scrolling reaches it.
    pub reach: Reach,
}

/// Every mark that is not on screen right now, with the offset that would show
/// it or the reason nothing will.
///
/// # Precondition
///
/// The same one [`crate::containment::escapes`] states: the scene has been
/// through `pinion_runtime::compute_layout`, so every node's `rect` is in its
/// enclosing scroll frame. That frame is exactly the content coordinate system
/// this module judges in, which is why the arithmetic is a subtraction rather
/// than a fold.
///
/// `window` is the outermost viewport — the size a mark with no scroll ancestor
/// is judged against.
///
/// `ink_of` is asked only about [`Scene::Text`] nodes, for the reason
/// [`crate::containment`] gives: a run's promise and its paint are different
/// rectangles, and the paint is the one a reader sees.
#[must_use]
pub fn out_of_sight(scene: &Scene, window: (u32, u32), ink_of: InkOf<'_>) -> Vec<OutOfSight> {
    // Pass 1 — every node's window-absolute rectangle, so a scroll node's
    // viewport can be named without re-folding the walk by hand. Same shape as
    // `containment::escapes`, and for the same reason: the second draft of that
    // function read a parent's rectangle out of the child's fold and was wrong.
    let mut absolute: HashMap<*const Scene, Rect> = HashMap::new();
    scene.for_each_node(&mut |visit| {
        let r = visit.node.rect();
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "clamped into u32's range before the cast"
        )]
        let fold =
            |v: u32, by: i64| -> u32 { (i64::from(v) + by).clamp(0, i64::from(u32::MAX)) as u32 };
        absolute.insert(
            std::ptr::from_ref(visit.node),
            Rect::new(
                fold(r.x, visit.offset.0),
                fold(r.y, visit.offset.1),
                r.w,
                r.h,
            ),
        );
    });

    let window_viewport = Viewport {
        name: WINDOW.to_owned(),
        // The window's own frame is the one top-level marks are already in.
        origin: (0, 0),
        size: window,
        content: window,
        at: (0, 0),
        max: (0, 0),
    };

    let mut found = Vec::new();
    scene.for_each_node(&mut |visit| {
        if visit.ancestors.is_empty() {
            return; // the root is the surface; it is not shown inside anything
        }
        // The innermost enclosing scroll, or the window. `ancestors` excludes
        // the node itself, which is what makes a scroll node get judged against
        // the viewport ABOVE it rather than against its own — the composition
        // the module header describes.
        let viewport = visit
            .ancestors
            .iter()
            .rev()
            .find_map(|a| {
                a.clips_subtree()
                    .then(|| viewport_of(a, absolute.get(&std::ptr::from_ref(*a))))
            })
            .unwrap_or_else(|| window_viewport.clone());

        let promised = visit.node.rect();
        let (rect, content) = match visit.node {
            Scene::Text(t) => {
                let (w, h) = ink_of(t);
                (
                    Rect::new(promised.x, promised.y, w, h),
                    Some(t.content.clone()),
                )
            }
            _ => (promised, None),
        };
        if rect.w == 0 || rect.h == 0 {
            return; // nothing was drawn, so nothing is being missed
        }
        if intersects(rect, viewport.shown()) {
            return; // some of it is on screen: the reader has it
        }
        let short_by = Overhang::of(rect, viewport.reachable());
        let reach = if short_by.is_contained() {
            Reach::Scrollable {
                to: least_move(rect, &viewport),
            }
        } else {
            Reach::Lost { short_by }
        };
        found.push(OutOfSight {
            tag: visit.node.tag().map(str::to_owned),
            path: visit.path.to_vec(),
            content,
            rect,
            viewport,
            reach,
        });
    });
    found
}

/// Read a scroll node as a viewport.
fn viewport_of(node: &Scene, absolute: Option<&Rect>) -> Viewport {
    let window = node.clip_window().unwrap_or_default();
    // ★★ R1685 — WHICH FRAME the marks under this window are expressed in, and
    // the two clipping kinds answer differently.
    //
    // A scroll's content is stored in the scroll's own frame with its origin at
    // the top-left, and its on-screen size is read from the absolute map so an
    // outer clip narrowing it is folded in. A container introduces no frame at
    // all: its children keep the coordinates the container's own rect is in, so
    // its window starts where that rect starts and is that rect's size. Reading
    // an ancestor's narrowing into it here would mix the two frames; the
    // container is itself a mark judged against ITS enclosing window, which is
    // the composition this module already documents for nested scrolls.
    let (origin, size) = match node {
        Scene::Scroll(_) => (
            (0, 0),
            absolute.map_or((window.w, window.h), |r| (r.w, r.h)),
        ),
        _ => ((window.x, window.y), (window.w, window.h)),
    };
    // ★★ R1685 — the two clipping kinds answer this differently, and the
    // difference IS the module's subject.
    //
    // A [`Scene::Scroll`] has a range: its content rect is scroll-local with
    // its origin at (0, 0), so `w`/`h` already ARE the intrinsic extent — the
    // same reading `update_scroll_state_bounds` does when it publishes the
    // bound. A mark past the viewport is [`Reach::Scrollable`]; the reader
    // moves.
    //
    // A box that clips because it declares `Overflow::Hidden` has NO range,
    // and saying so is not a special case bolted on here: CSS's own rule is
    // that content overflowing a hidden box does not contribute to a scroll
    // region, so the box's own extent *is* its scroll region. Feeding that
    // through the same arithmetic yields `max == (0, 0)`, and a mark past it
    // comes out [`Reach::Lost`] — which is the true statement. Deriving the
    // range from geometry alone (what this did before R1685, when a scroll was
    // the only thing that could clip) would answer "scroll to y=152" about a
    // window nothing can move, and this module exists to end exactly that
    // class of answer.
    let (content, at) = match node {
        Scene::Scroll(s) => {
            let content = s.content.rect();
            ((content.w, content.h), (s.offset_x, s.offset_y))
        }
        _ => (size, (0, 0)),
    };
    Viewport {
        name: node
            .tag()
            .map_or_else(|| UNTAGGED.to_owned(), ToString::to_string),
        origin,
        size,
        content,
        at,
        max: (
            max_scroll_offset(content.0, size.0),
            max_scroll_offset(content.1, size.1),
        ),
    }
}

/// Do the two boxes share any pixel?
fn intersects(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// The offset that shows `rect`, moving as little as possible from where the
/// viewport already is.
fn least_move(rect: Rect, viewport: &Viewport) -> (i32, i32) {
    let axis = |lo: u32, len: u32, at: i32, size: u32, max: i32| -> i32 {
        let lo = i64::from(lo);
        let far = lo + i64::from(len) - i64::from(size);
        let at = i64::from(at);
        // An offset shows the whole mark when it lies in `far..=lo`, so the
        // least move is whichever end of that span `at` is outside. When the
        // mark is bigger than the viewport that span is empty (`far > lo`) and
        // the leading edge is the answer: bringing the end in would push the
        // start out, and every scroller starts at the beginning.
        let want = if far > lo {
            lo
        } else if at < far {
            far
        } else if at > lo {
            lo
        } else {
            at
        };
        #[allow(
            clippy::cast_possible_truncation,
            reason = "clamped into 0..=max, both i32, on this line"
        )]
        let clamped = want.clamp(0, i64::from(max)) as i32;
        clamped
    };
    (
        axis(
            rect.x,
            rect.w,
            viewport.at.0,
            viewport.size.0,
            viewport.max.0,
        ),
        axis(
            rect.y,
            rect.h,
            viewport.at.1,
            viewport.size.1,
            viewport.max.1,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containment::{Fate, escapes};
    use crate::scene::{ContainerNode, ScrollNode, TextNode};

    /// Ink measured as `len * 8` by `12` — a stand-in with no font in it, so
    /// every assertion here is about which box was compared against.
    fn stub_ink(t: &TextNode) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "fixture strings are a handful of characters"
        )]
        let w = (t.content.chars().count() as u32) * 8;
        (w, 12)
    }

    fn text(content: &str, rect: Rect, tag: &'static str) -> Scene {
        Scene::Text(TextNode::new(content, rect).with_tag(tag))
    }

    fn boxed(rect: Rect, tag: &'static str, children: Vec<Scene>) -> Scene {
        let mut c = ContainerNode::new(children);
        c.rect = rect;
        c.tag = Some(tag.into());
        Scene::Container(c)
    }

    /// Three rows in a 300-tall content behind a 100-tall viewport: one on
    /// screen, one the range covers, one past the end of the content.
    fn pane(offset_y: i32) -> Scene {
        let mut node = ScrollNode::new(
            Rect::new(0, 0, 100, 100),
            boxed(
                Rect::new(0, 0, 100, 300),
                "pane.content",
                vec![
                    text("a", Rect::new(0, 0, 60, 12), "row.a"),
                    text("b", Rect::new(0, 240, 60, 12), "row.b"),
                    text("c", Rect::new(0, 400, 60, 12), "row.c"),
                ],
            ),
        )
        .with_tag("pane");
        node.offset_y = offset_y;
        Scene::Scroll(node)
    }

    fn by_tag<'r>(found: &'r [OutOfSight], tag: &str) -> Option<&'r OutOfSight> {
        found.iter().find(|o| o.tag.as_deref() == Some(tag))
    }

    /// (R1685) The same three rows behind a box that clips because it says so
    /// — byte-for-byte the geometry of [`pane`], with the only difference being
    /// which node carries the window.
    fn hidden_pane() -> Scene {
        let mut node = ContainerNode::new(vec![boxed(
            Rect::new(0, 0, 100, 300),
            "pane.content",
            vec![
                text("a", Rect::new(0, 0, 60, 12), "row.a"),
                text("b", Rect::new(0, 240, 60, 12), "row.b"),
                text("c", Rect::new(0, 400, 60, 12), "row.c"),
            ],
        )]);
        node.rect = Rect::new(0, 0, 100, 100);
        node.tag = Some("pane".into());
        node.layout =
            crate::style::LayoutStyle::new().with_overflow(crate::style::Overflow::Hidden);
        Scene::Container(node)
    }

    /// ★★★ R1685 — the headline, and the reason the workaround was refused.
    ///
    /// [`pane`] and [`hidden_pane`] place the same rows at the same coordinates
    /// behind the same 100-tall window. The only difference is the node that
    /// carries the window, and that difference is the whole verdict: a scroll
    /// publishes a range a reader can move, so `row.b` is one scroll away; a
    /// box that clips publishes none, so the same row is lost.
    ///
    /// Before R1685 a region that had to clip could only do it by becoming a
    /// scroll nobody scrolls — and this module would then have answered
    /// "scrollable to y=152" about a window no gesture in the application can
    /// move. A false *reachable* is the error this module exists to end, so the
    /// declaration had to be the thing that decides.
    #[test]
    fn r1685_a_hidden_box_loses_what_a_scroll_of_the_same_shape_only_hides() {
        let scrolled = out_of_sight(&pane(0), (400, 400), &mut stub_ink);
        let hidden = out_of_sight(&hidden_pane(), (400, 400), &mut stub_ink);

        // The fixtures really are the same shape: both hide the same two rows.
        let names = |found: &[OutOfSight]| {
            let mut v: Vec<String> = found.iter().filter_map(|o| o.tag.clone()).collect();
            v.sort();
            v
        };
        assert_eq!(
            names(&scrolled),
            names(&hidden),
            "the two fixtures must hide the same marks, or the verdicts below \
             are about different geometry"
        );

        let b_scrolled = by_tag(&scrolled, "row.b").expect("row.b is off screen");
        assert_eq!(b_scrolled.reach, Reach::Scrollable { to: (0, 152) });

        // ★ The invariant that keeps `least_move` sound, asserted rather than
        // reasoned about: a non-scroll window is the only kind with a non-zero
        // `origin`, and it can never be `Scrollable`, because its content IS
        // its own extent so its range is zero on both axes. `least_move` works
        // in offsets and knows nothing about `origin`; if a viewport with one
        // ever produced an offset, that offset would be in the wrong frame.
        assert!(
            hidden
                .iter()
                .all(|o| matches!(o.reach, Reach::Lost { .. }) || o.viewport.origin == (0, 0)),
            "a viewport with an origin published an offset, which `least_move` \
             expresses in the scroll frame: {hidden:?}"
        );

        let b_hidden = by_tag(&hidden, "row.b").expect("row.b is off screen");
        let Reach::Lost { short_by } = b_hidden.reach else {
            panic!(
                "a box that clips has no range, so nothing brings row.b back: \
                 {b_hidden:?}"
            );
        };
        // The reachable box is the window itself (100 tall); the row ends at 252.
        assert_eq!(short_by.bottom, 152, "{b_hidden:?}");
        assert_eq!(b_hidden.viewport.name, "pane");
        assert_eq!(
            b_hidden.viewport.max,
            (0, 0),
            "a hidden box has no range at all"
        );
        assert!(
            b_hidden.viewport.fits(),
            "and it says so: CSS's rule is that content overflowing a hidden \
             box does not contribute to a scroll region, so the box's own \
             extent IS its scroll region"
        );
    }

    /// ★★★ R1685 — a hidden box away from the origin judges its children in
    /// the frame they are actually in.
    ///
    /// The test above cannot see this: its box sits at `(0, 0)`, where a
    /// window's origin and the frame's origin are the same number, so every
    /// arithmetic that confuses the two passes. A scroll's content IS stored in
    /// its own frame with the origin at the top-left, and this module was built
    /// when a scroll was the only thing that could clip — so the boxes it
    /// compares started at zero, and a container clip inherited that.
    ///
    /// Measured, and not by a unit test: adopting the declaration on the
    /// toolbar's controls reported six labels — one per control past the first
    /// — as marks no gesture can reach, on two real screens. Each was judged
    /// against a window at the far left of the window while sitting at its own
    /// control's x. The ratchet caught it; this pins it.
    #[test]
    fn r1685_a_hidden_box_away_from_the_origin_does_not_lose_what_it_holds() {
        let mut label = ContainerNode::new(vec![text("hi", Rect::new(206, 8, 40, 12), "label")]);
        label.rect = Rect::new(204, 4, 48, 20);
        let mut control = ContainerNode::new(vec![Scene::Container(label)]);
        control.rect = Rect::new(200, 0, 56, 28);
        control.tag = Some("control".into());
        control.layout =
            crate::style::LayoutStyle::new().with_overflow(crate::style::Overflow::Hidden);

        let found = out_of_sight(&Scene::Container(control), (400, 400), &mut stub_ink);
        assert!(
            found.is_empty(),
            "the label is inside its control, which is inside the window — \
             nothing here is out of sight: {found:?}"
        );
    }

    /// (R1685) A container that does NOT declare the clip is not a viewport,
    /// so its children are judged against the window as they always were.
    ///
    /// The counterfactual that keeps the test above honest: if every container
    /// became a zero-range viewport, every mark below any box would report
    /// `Lost` and the assertion would pass for a reason that has nothing to do
    /// with the declaration.
    #[test]
    fn r1685_a_container_that_declares_nothing_is_not_a_viewport() {
        let mut node = ContainerNode::new(vec![text("b", Rect::new(0, 240, 60, 12), "row.b")]);
        node.rect = Rect::new(0, 0, 100, 100);
        node.tag = Some("pane".into());
        let found = out_of_sight(&Scene::Container(node), (400, 400), &mut stub_ink);
        // 240 is inside the 400x400 window, so nothing is out of sight at all —
        // the row hangs out of its parent (which is `containment`'s question),
        // and the reader can see it (which is this module's).
        assert!(
            found.is_empty(),
            "an undeclared container clips nothing, so nothing it holds is out \
             of sight: {found:?}"
        );
    }

    /// ★ The question this module exists for, half one: a mark the range
    /// covers is not a defect, and the report says the offset that shows it.
    #[test]
    fn r1662_a_mark_the_range_covers_is_scrollable_and_names_the_offset() {
        let found = out_of_sight(&pane(0), (400, 400), &mut stub_ink);
        let b = by_tag(&found, "row.b").expect("row.b is off screen: {found:?}");
        // Content 300, viewport 100 -> max 200. The row's bottom is 252, so the
        // least move that shows all of it is 252 - 100.
        assert_eq!(b.reach, Reach::Scrollable { to: (0, 152) }, "{b:?}");
        assert_eq!(b.viewport.name, "pane");
        assert_eq!(b.viewport.max, (0, 200));
    }

    /// ★ Half two: a mark past the content extent is lost, and says by how
    /// much. No offset in `0..=200` puts 400..412 inside a 100-tall window.
    #[test]
    fn r1662_a_mark_past_the_content_extent_is_lost() {
        let found = out_of_sight(&pane(0), (400, 400), &mut stub_ink);
        let c = by_tag(&found, "row.c").expect("row.c is off screen");
        let Reach::Lost { short_by } = c.reach else {
            panic!("row.c is past the extent: {c:?}");
        };
        // Reachable box is 0..=300 (max 200 + viewport 100); the ink ends at 412.
        assert_eq!(short_by.bottom, 112, "{c:?}");
        assert_eq!(short_by.top, 0);
        assert_eq!(c.content.as_deref(), Some("c"));
    }

    /// ★ The mark that is on screen is not in the report at all. `Shown` is
    /// unrepresentable, so no consumer has to remember to filter it.
    #[test]
    fn r1662_a_visible_mark_is_not_reported() {
        let found = out_of_sight(&pane(0), (400, 400), &mut stub_ink);
        assert!(by_tag(&found, "row.a").is_none(), "{found:?}");
    }

    /// ★ Why this is not another field on an `Escape`: containment gives the
    /// lost row the same word it gives a two-pixel line-box rounding, and says
    /// nothing whatsoever about the reachable one.
    ///
    /// Asserting the blindness rather than describing it — if containment ever
    /// grows an answer here, this fails and the overlap gets resolved
    /// deliberately instead of leaving two derivations that disagree.
    #[test]
    fn r1662_containment_cannot_tell_the_two_apart() {
        let found = escapes(&pane(0), &mut stub_ink);
        assert_eq!(
            found.len(),
            1,
            "only the past-extent row escapes: {found:?}"
        );
        assert_eq!(found[0].tag.as_deref(), Some("row.c"));
        assert_eq!(
            found[0].fate,
            Fate::Clipped,
            "the same verdict an ordinary clipped overhang gets"
        );
    }

    /// ★ The move a gate watches: the identical content, once in a pane that
    /// does not scroll and once in a pane that does. The window is a viewport
    /// whose range is zero, so this is one function with two inputs and not two
    /// checks that have to be kept in step.
    #[test]
    fn r1662_making_a_pane_scroll_moves_a_mark_from_lost_to_scrollable() {
        let rows = || {
            vec![
                text("a", Rect::new(0, 0, 60, 12), "row.a"),
                text("b", Rect::new(0, 240, 60, 12), "row.b"),
            ]
        };
        let flat = boxed(Rect::new(0, 0, 100, 100), "pane", rows());
        let found = out_of_sight(&flat, (100, 100), &mut stub_ink);
        let b = by_tag(&found, "row.b").expect("row.b is below the pane");
        assert!(b.reach.is_lost(), "no scroll anywhere: {b:?}");
        assert_eq!(b.viewport.name, WINDOW);
        assert!(b.viewport.fits(), "a window has no range");

        let scrolled = Scene::Scroll(
            ScrollNode::new(
                Rect::new(0, 0, 100, 100),
                boxed(Rect::new(0, 0, 100, 300), "pane.content", rows()),
            )
            .with_tag("pane"),
        );
        let found = out_of_sight(&scrolled, (100, 100), &mut stub_ink);
        let b = by_tag(&found, "row.b").expect("row.b is still off screen");
        assert_eq!(b.reach, Reach::Scrollable { to: (0, 152) }, "{b:?}");
    }

    /// ★ The offset semantics, pinned against the reference's `ensureVisible`:
    /// a point 900 into a 198-tall viewport answers 702, not 900. Measured on
    /// the reference out-of-tree; reproduced here as arithmetic so the rule
    /// survives without it.
    #[test]
    fn r1662_the_offset_moves_as_little_as_it_can() {
        let scene = Scene::Scroll(
            ScrollNode::new(
                Rect::new(0, 0, 200, 198),
                boxed(
                    Rect::new(0, 0, 200, 1000),
                    "content",
                    vec![text("x", Rect::new(0, 900, 8, 1), "mark")],
                ),
            )
            .with_tag("area"),
        );
        // Ink height 12 from the stub, so the mark spans 900..912 and the least
        // move that ends it inside a 198-tall window is 912 - 198.
        let found = out_of_sight(&scene, (400, 400), &mut stub_ink);
        let m = by_tag(&found, "mark").expect("off screen");
        assert_eq!(m.reach, Reach::Scrollable { to: (0, 714) }, "{m:?}");
    }

    /// ★ A mark taller than the viewport shows its leading edge. Bringing its
    /// end in would push its start out, and every scroller in the world starts
    /// at the beginning.
    #[test]
    fn r1662_a_mark_taller_than_the_viewport_shows_its_leading_edge() {
        let scene = Scene::Scroll(
            ScrollNode::new(
                Rect::new(0, 0, 100, 50),
                boxed(
                    Rect::new(0, 0, 100, 400),
                    "content",
                    vec![Scene::Container({
                        let mut c = ContainerNode::new(vec![]);
                        c.rect = Rect::new(0, 100, 100, 200);
                        c.tag = Some("tall".into());
                        c
                    })],
                ),
            )
            .with_tag("area"),
        );
        let found = out_of_sight(&scene, (400, 400), &mut stub_ink);
        let t = by_tag(&found, "tall").expect("off screen");
        assert_eq!(t.reach, Reach::Scrollable { to: (0, 100) }, "{t:?}");
    }

    /// ★ Why [`Viewport::reachable`] is `max + size` and not `content`: with
    /// content shorter than the viewport the range is zero, and the empty space
    /// below the content is on screen. Reading the bound off the content
    /// reported a mark parked there as lost.
    #[test]
    fn r1662_short_content_leaves_the_empty_space_visible() {
        let mut node = ScrollNode::new(
            Rect::new(0, 0, 100, 200),
            boxed(
                Rect::new(0, 0, 100, 40),
                "content",
                vec![text("x", Rect::new(0, 100, 8, 1), "mark")],
            ),
        )
        .with_tag("area");
        node.offset_y = 0;
        let found = out_of_sight(&Scene::Scroll(node), (400, 400), &mut stub_ink);
        assert!(
            by_tag(&found, "mark").is_none(),
            "100..112 is inside a 200-tall viewport: {found:?}"
        );
    }

    /// ★ Nesting composes by the chain being walked, not by folding: a pane
    /// that can reach all its own content while sitting off the window reports
    /// once, on the pane — the node whose placement is the repair.
    #[test]
    fn r1662_an_off_window_pane_reports_once_not_once_per_row() {
        let pane = Scene::Scroll(
            ScrollNode::new(
                Rect::new(500, 0, 100, 100),
                boxed(
                    Rect::new(0, 0, 100, 300),
                    "pane.content",
                    vec![
                        text("a", Rect::new(0, 0, 60, 12), "row.a"),
                        text("b", Rect::new(0, 240, 60, 12), "row.b"),
                    ],
                ),
            )
            .with_tag("pane"),
        );
        let root = boxed(Rect::new(0, 0, 200, 200), "root", vec![pane]);
        let found = out_of_sight(&root, (200, 200), &mut stub_ink);
        let p = by_tag(&found, "pane").expect("the pane is off the window");
        assert!(p.reach.is_lost(), "{p:?}");
        assert_eq!(p.viewport.name, WINDOW);
        // The rows are judged against the pane, which reaches both of them.
        assert!(by_tag(&found, "row.a").is_none(), "{found:?}");
        assert_eq!(
            by_tag(&found, "row.b").map(|o| o.reach),
            Some(Reach::Scrollable { to: (0, 152) }),
            "{found:?}"
        );
    }

    /// ★ The predicate the reference makes a consumer infer from
    /// `maximum() > 0` — the same answer it gives for an area that never set
    /// its range at all.
    #[test]
    fn r1662_a_viewport_says_whether_its_content_fits() {
        let found = out_of_sight(&pane(0), (400, 400), &mut stub_ink);
        let b = by_tag(&found, "row.b").expect("off screen");
        assert!(!b.viewport.fits());
        assert_eq!(b.viewport.content, (100, 300));
        assert_eq!(b.viewport.size, (100, 100));
    }

    /// ★ The current offset is part of the answer: scrolled to the bottom, the
    /// row that was reachable is on screen and the row that was on screen is
    /// now the one behind us — still reachable, and the offset to get back is
    /// reported.
    #[test]
    fn r1662_the_report_follows_the_offset() {
        let found = out_of_sight(&pane(200), (400, 400), &mut stub_ink);
        assert!(by_tag(&found, "row.b").is_none(), "240..252 is in 200..300");
        let a = by_tag(&found, "row.a").expect("row.a is above the viewport now");
        assert_eq!(a.reach, Reach::Scrollable { to: (0, 0) }, "{a:?}");
        // The lost row stays lost whatever the offset is.
        assert!(by_tag(&found, "row.c").is_some_and(|c| c.reach.is_lost()));
    }
}
