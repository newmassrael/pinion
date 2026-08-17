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
//! [`Reach::Scrollable`]. A hidden box publishes no range, so nothing brings its
//! overflow back: a mark past it entirely is [`Reach::Lost`], and one hanging
//! over its edge is [`Reach::Clipped`] (R1713) — the reader keeps what is inside
//! and nothing reaches the rest.
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
//! Measured against the reference toolkit 6.11.1 (probe built and run
//! out-of-tree, so nothing here cites it): a scroll area derives its range from
//! the laid-out child and keeps it live — parity, and this tree has had that
//! since R55.C via `update_scroll_state_bounds`. What it has **no** surface for
//! is the question this module answers. Overflow can only be *inferred* by a
//! consumer reading the range's maximum; there is no per-mark reachability, no
//! "which offset shows it", and an area that never sets its range reports the
//! same maximum as a pane whose content genuinely fits. That last one is
//! precisely the analysis-tool panes' defect, and on the reference it is
//! undiagnosable from outside the widget.
//!
//! ★★★★★ R1713 re-measured it for the chain, and **the capability is parity —
//! this time we were the short ones.** Probed on the exact shape this round is
//! about (a 282-wide holder clipping a 310-wide scrolling pane whose content
//! carries a full-width row and an 8-pixel glyph at x=286): the reference's
//! per-widget visible-region answer folds the WHOLE chain, reporting the
//! scrolled-in row as `281x20` rather than its own 310. That is exactly what
//! `walk_marks` did not do until this round.
//!
//! What it does not have, each proven by a compile error naming the member
//! rather than by a search that found nothing (6/6):
//!
//! * a *reason* — the not-yet-scrolled row and the unreachable glyph both answer
//!   "empty region", and telling a clip from a loss is left to the caller
//!   comparing a width against a width;
//! * a way to ask about an offset it is **not** at, so a caller must move first
//!   and look (this module's `at` parameter is §2 #3);
//! * the offset that *would* show something — its "show me this" call acts and
//!   reports nothing;
//! * any enumeration of what a pane holds that nothing reaches;
//! * **any answer at all for a mark that is not a widget** — an item view has no
//!   per-item version, and the marks this round found were text runs inside
//!   buttons;
//! * a success answer from that "show me this" call. ★ Measured: asking it to
//!   show the unreachable glyph left the offsets untouched and the glyph
//!   invisible, and said so nowhere.
//!
//! # Nesting: reachability composes down the chain (R1713)
//!
//! A mark is judged against its innermost enclosing viewport — but that
//! viewport is judged against **what the chain above it can ever bring into
//! view**, not against its own declared box. [`Viewport::size`] is therefore the
//! *aperture* the chain leaves, and [`Viewport::declared`] is the box the node
//! asked for; the two differ exactly when something above narrows it.
//!
//! ★★★★★ R1712.1 measured what the missing fold costs. The analysis tool's node
//! lab at 1506 wide: `lost` was **zero** while **nine tagged marks sat entirely
//! outside the window**, every one of them an action — five row remove buttons,
//! two spin steppers, `+ key` and `delete` — and the pane holding them has no
//! horizontal range, so no gesture anywhere reaches them. They were invisible
//! here because they fit *the inspector pane*, and the pane is what the window
//! slices. A window floor was published 89 pixels too low on the strength of it.
//!
//! Two rules keep the fold from reporting the same break many times:
//!
//! * Each level is intersected with the level above's [`Viewport::reachable`],
//!   not with what it happens to be showing now — so a pane scrolled out of an
//!   outer pane still offers its children everything the outer range covers.
//! * A level the chain leaves **nothing** of *seals*: the marks below it are not
//!   judged at all, because that level is itself a mark one frame up and carries
//!   the single report. A pane sitting wholly off the window is one
//!   [`Reach::Lost`] on the pane, never one per row — the attribution this
//!   module always claimed, now true rather than achieved by not looking.
//!
//! What is *not* folded, and is pinned by a test rather than left as prose: the
//! offset in [`Reach::Scrollable`] is the one belonging to the row's own
//! viewport. On a chain deeper than one clip an ancestor may have to move too,
//! and this answer does not say so ⇒
//! `debt-a-scroll-offset-is-named-for-one-viewport-of-a-chain`. Measured on all
//! three analysis-tool screens: every clip chain there is **one** level deep, so
//! no reader can meet it today.
//!
//! # Whether a mark is on screen is the walk's own answer
//!
//! "Is the reader looking at it right now" is asked of
//! [`NodeVisit::absolute_rect_of`](crate::scene::NodeVisit::absolute_rect_of) —
//! the same fold `absolute_rects_by_tag` publishes and every demo picks press
//! points from. Before R1713 this module re-derived it from the innermost
//! viewport alone, which is the second half of how a mark inside a sliced pane
//! read as visible. R1676 made the argument for the tag map: *reported* and
//! *visible* have to be one fact. It applies here too.

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
    ///
    /// ★ R1713 — this is the **aperture's** origin, so a chain that clips this
    /// viewport's near edge moves it: content before it can never be brought
    /// into view, because scrolling only carries content *towards* the near edge.
    pub origin: (u32, u32),
    /// The viewport's size — the window it shows onto the content.
    ///
    /// ★★★★★ R1713 — the window it *actually* shows, after everything above it
    /// has had its say. A pane 312 wide whose right 119 pixels the window cuts
    /// off offers its children 193, and before this fold it offered them 312 —
    /// which is how content the window had removed from existence read as
    /// perfectly placed. [`Self::declared`] is the box the node asked for, so the
    /// narrowing is a subtraction a reader can do rather than a fact only one of
    /// the two numbers knows.
    pub size: (u32, u32),
    /// (R1713) The box this viewport's own node declares, in the frame its marks
    /// use — `[0, 0, w, h]` for a [`Scene::Scroll`] (its content has its own
    /// frame) and the node's own rect for a box that clips by declaration.
    ///
    /// Equal to `[origin, size]` exactly when nothing above narrows this
    /// viewport. `Overhang::of(declared, aperture)` is what the chain took.
    pub declared: Rect,
    /// The content's extent along each axis, as the laid-out subtree reports
    /// it. This is the value the reference exposes nowhere.
    pub content: (u32, u32),
    /// The offset the scene carries right now.
    pub at: (i32, i32),
    /// The largest offset each axis can take: `max(0, content - declared)`,
    /// through the same [`max_scroll_offset`] the layout pass publishes with, so
    /// this derivation and the runtime's clamp cannot disagree.
    ///
    /// ★ R1713 — measured against [`Self::declared`] rather than
    /// [`Self::size`]. The runtime clamps offsets against the node's own box and
    /// knows nothing about who clips it, so deriving the range from a narrowed
    /// aperture would publish offsets the scene refuses to take.
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
    ///
    /// ★ R1713 — the same arithmetic over the **aperture**, which is what makes
    /// reachability compose: this is also the box the next level down is
    /// intersected with, so one function answers "what can this level ever show"
    /// for a mark and for a nested viewport alike. `max` stays derived from the
    /// node's own box (see [`Self::max`]), so narrowing the aperture never
    /// invents range the runtime would refuse to scroll to.
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
}

/// What it would take to see a mark that is not on screen.
///
/// "It is on screen" is unrepresentable here because [`out_of_sight`] does not
/// report those marks at all. A record that could say `Shown` would be a record
/// every caller has to filter, and the filter is the thing that gets forgotten.
///
/// # ★★★★★ R1713 — why there are three arms and were two
///
/// [`shrink::audit`](crate::shrink::audit) reads this to decide whether a
/// window's declared concession is honest, and its rule is *a concession may
/// clip and may never lose*. Its own doc states the split it needs — "`cut` says
/// what the reader cannot see at once, `lost` says what the reader cannot see at
/// all" — and until R1713 [`Self::Lost`] did not mean that. It meant *not fully
/// containable*, so a form row 310 pixels wide inside a 282-pixel aperture came
/// back `Lost` with 28 pixels of overhang, and a rule written to fail on content
/// nobody can reach failed on content nearly all of which is reachable.
///
/// Measured on the analysis tool's node lab at 1595x360 the day the arm was
/// added: **19 lost**, of which **6** were marks no pixel of which can ever be
/// seen (five row `×` glyphs and one `+`) and **13** were wide rows a reader
/// reaches all but the right edge of. One number, two facts, and the severe
/// verdict was reading the wrong one.
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
    ///
    /// ⚠ R1713 — the offset belongs to the viewport named in the row. On a clip
    /// chain deeper than one level an ancestor may have to move as well, and this
    /// answer does not say which or how far ⇒
    /// `debt-a-scroll-offset-is-named-for-one-viewport-of-a-chain`, whose
    /// measurement is that every clip chain on the three analysis-tool screens is
    /// one level deep. `r1713_a_deeper_chain_names_only_its_own_offset` pins the
    /// behaviour so the gap stays a known one.
    Scrollable {
        /// The offset to scroll to, clamped into `0..=max`.
        to: (i32, i32),
    },
    /// (R1713) Some offset shows part of it and no offset shows all of it: this
    /// much lies outside everything the viewport's range can ever show.
    ///
    /// What a window below its layout's comfortable size does to the pane at its
    /// edge, and what a concession is allowed to buy — the reader still reaches
    /// the row, minus its right edge. Named for the word
    /// [`crate::shrink`]'s rule is written in (*a concession may **clip** and may
    /// never **lose***) so the rule reads off the type. Nothing to do with
    /// [`Fate::Clipped`](crate::containment::Fate::Clipped), which is about a
    /// mark leaving its owner's box; [`cut`] reports the marks of **both** this
    /// arm and [`Self::Lost`], because the question it asks — can this be shown
    /// whole — is answered no by each.
    Clipped {
        /// How far past the reachable box the mark reaches, per edge.
        short_by: Overhang,
    },
    /// No offset shows any of it: the mark and everything the viewport's range
    /// can ever show are disjoint.
    ///
    /// On a pane that does not scroll — including every mark judged against the
    /// window — the range is zero, so this is simply "outside the box, and
    /// there is no way in". This is the arm a gate fails on, and the one
    /// [`crate::shrink`] refuses to let a concession excuse.
    Lost {
        /// How far past the reachable box the mark reaches, per edge. At least
        /// one edge exceeds the mark's own extent — that is what makes it a
        /// loss rather than a cut.
        short_by: Overhang,
    },
}

impl Reach {
    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Scrollable { .. } => "scrollable",
            Self::Clipped { .. } => "clipped",
            Self::Lost { .. } => "lost",
        }
    }

    /// True for the arm a gate fails on: nothing brings any of this mark into
    /// view.
    #[must_use]
    pub const fn is_lost(self) -> bool {
        matches!(self, Self::Lost { .. })
    }

    /// (R1713) How far past the reachable box the mark reaches, where it does.
    ///
    /// `None` for [`Self::Scrollable`], which is the arm that has no overhang by
    /// definition — so a caller reading the number cannot read it off a mark
    /// that fits.
    #[must_use]
    pub const fn short_by(self) -> Option<Overhang> {
        match self {
            Self::Scrollable { .. } => None,
            Self::Clipped { short_by } | Self::Lost { short_by } => Some(short_by),
        }
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
    let mut found = Vec::new();
    walk_marks(scene, window, ink_of, &mut |mark| {
        if mark.on_screen {
            return; // some of it is on screen: the reader has it
        }
        let reachable = mark.viewport.reachable();
        let short_by = Overhang::of(mark.rect, reachable);
        // ★★★★★ R1713 — three answers, and the third is the one a safety rule
        // needs: whether ANY pixel of this mark is inside what the viewport's
        // range can ever show. Asking only "does all of it fit" gave a wide row
        // whose right edge is unreachable the same word as a glyph nothing can
        // ever bring back.
        let reach = if short_by.is_contained() {
            Reach::Scrollable {
                to: least_move(mark.rect, &mark.viewport),
            }
        } else if mark.rect.intersect(reachable).is_some() {
            Reach::Clipped { short_by }
        } else {
            Reach::Lost { short_by }
        };
        found.push(OutOfSight {
            tag: mark.tag,
            path: mark.path,
            content: mark.content,
            rect: mark.rect,
            viewport: mark.viewport,
            reach,
        });
    });
    found
}

/// Every mark with the viewport it is judged against, handed to `visit` in
/// paint order.
///
/// ★ R1711 — lifted out of [`out_of_sight`] when [`cut`] became its second
/// caller. What the two share is not a convenience: it is the definition of a
/// mark's rectangle (shaped ink for a run, the promised box otherwise) and of
/// its innermost enclosing viewport, and two copies of those would be two
/// answers to "where is this" for one screen.
fn walk_marks(
    scene: &Scene,
    window: (u32, u32),
    ink_of: InkOf<'_>,
    visit_mark: &mut dyn FnMut(Mark),
) {
    let window_rect = Rect::new(0, 0, window.0, window.1);
    let window_viewport = Viewport {
        name: WINDOW.to_owned(),
        // The window's own frame is the one top-level marks are already in.
        origin: (0, 0),
        size: window,
        declared: window_rect,
        content: window,
        at: (0, 0),
        max: (0, 0),
    };

    scene.for_each_node(&mut |node_visit| {
        let visit = &node_visit;
        if visit.ancestors.is_empty() {
            return; // the root is the surface; it is not shown inside anything
        }
        // ★★★★★ R1713 — the innermost enclosing clip, judged against everything
        // above it. `None` means some level of the chain is sealed, and then the
        // marks below it are NOT this report's business: that level is itself a
        // mark one frame up and carries the one report the break deserves.
        let Some(viewport) = innermost_viewport(visit.ancestors, &window_viewport) else {
            return;
        };

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
        visit_mark(Mark {
            tag: visit.node.tag().map(str::to_owned),
            path: visit.path.to_vec(),
            content,
            // ★ R1713 — asked of the walk, so this and the tag index answer one
            // question. The window is intersected in explicitly because a scene
            // whose root declares no clip carries none: the walk narrows by what
            // the tree declares, and the window is the caller's fact.
            on_screen: visit
                .absolute_rect_of(rect)
                .and_then(|on| on.intersect(window_rect))
                .is_some(),
            rect,
            viewport,
        });
    });
}

/// The viewport a mark is judged against: the innermost clip in `ancestors`,
/// with the aperture everything above it leaves — or `None` when some level of
/// the chain leaves nothing at all.
///
/// Outermost-first, each level intersected with the level above's
/// [`Viewport::reachable`] rather than with what it is showing now, so a pane
/// scrolled out of an outer pane still offers its children every row the outer
/// range covers. `ancestors` excludes the node itself, which is what makes a
/// scroll node get judged against the viewport ABOVE it rather than its own.
fn innermost_viewport(ancestors: &[&Scene], window: &Viewport) -> Option<Viewport> {
    let mut from_above = window.reachable();
    let mut innermost = None;
    for ancestor in ancestors {
        if !ancestor.clips_subtree() {
            continue;
        }
        let viewport = viewport_of(ancestor, from_above)?;
        from_above = viewport.reachable();
        innermost = Some(viewport);
    }
    Some(innermost.unwrap_or_else(|| window.clone()))
}

/// One mark that cannot be seen WHOLE at this size, however the reader scrolls.
///
/// The sibling of [`OutOfSight`], and the difference between them is the
/// difference between two questions:
///
/// * [`out_of_sight`] asks *what is the reader not looking at right now* — a
///   question about the current offsets, whose answer changes when they scroll.
/// * [`cut`] asks *what can this size never show in full* — a question about
///   the size alone, which is what a window's floor is made of.
///
/// ★★★★★ R1711 measured why the second cannot be spelled with the first. At
/// 1506 pixels wide the analysis tool's node lab reports **nothing** out of
/// sight, because one pixel of a 100-pixel status chip is still on screen and
/// its 312-pixel inspector pane still starts at 1313. A floor derived from that
/// answer is a floor at which the inspector is sliced and the chip is a line.
/// "Some of it is visible" is the right answer to the first question and the
/// wrong one to the second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    /// The mark's own tag, when it has one.
    pub tag: Option<String>,
    /// The mark's address, as `scene/locate` reports it.
    pub path: Vec<String>,
    /// What the mark holds, for a text run.
    pub content: Option<String>,
    /// Where the mark sits in its viewport's content coordinates, using the
    /// shaped ink for a text run and the promised box for anything else.
    pub rect: Rect,
    /// The viewport it was judged against.
    pub viewport: Viewport,
    /// How far past everything that viewport can EVER show it reaches, per
    /// edge. Never fully zero — that is what makes it a cut.
    pub short_by: Overhang,
}

/// Every mark this size can never show whole, at any scroll offset.
///
/// The same walk and the same viewport arithmetic [`out_of_sight`] uses, with
/// one predicate changed: a mark is judged against what its viewport's range
/// can bring into view, and it has to fit **entirely**. Scrolling is not a
/// defect, so a row inside a pane that can reach it is not reported; a row past
/// the end of that pane's content is, and so is a pane sliced by the window.
///
/// # Precondition
///
/// [`out_of_sight`]'s, unchanged: the scene has been through
/// `pinion_runtime::compute_layout`.
#[must_use]
pub fn cut(scene: &Scene, window: (u32, u32), ink_of: InkOf<'_>) -> Vec<Cut> {
    let mut found = Vec::new();
    walk_marks(scene, window, ink_of, &mut |mark| {
        let short_by = Overhang::of(mark.rect, mark.viewport.reachable());
        if short_by.is_contained() {
            return;
        }
        found.push(Cut {
            tag: mark.tag,
            path: mark.path,
            content: mark.content,
            rect: mark.rect,
            viewport: mark.viewport,
            short_by,
        });
    });
    found
}

/// One mark, with the viewport it is judged against — the shared input of both
/// public derivations here, so the two cannot come to disagree about what a
/// mark's rectangle or its enclosing viewport is.
struct Mark {
    tag: Option<String>,
    path: Vec<String>,
    content: Option<String>,
    rect: Rect,
    /// (R1713) Whether any pixel of this mark is on screen right now, as the
    /// walk's own clip fold answers it — the question [`out_of_sight`] opens
    /// with, and the one [`cut`] deliberately does not ask.
    on_screen: bool,
    viewport: Viewport,
}

/// Read a clipping node as a viewport, given what the chain above it can ever
/// bring into view — `None` when that leaves nothing of it.
///
/// `from_above` is in the frame this node's own rect is expressed in, which is
/// the frame the level above judges its marks in: between two clips nothing
/// shifts the frame except a [`Scene::Scroll`] boundary, and that boundary is
/// where the level above was replaced.
fn viewport_of(node: &Scene, from_above: Rect) -> Option<Viewport> {
    let window = node.clip_window().unwrap_or_default();
    // ★★★★★ R1713 — the aperture, and the whole fold is this one line. A pane
    // the window slices offers its children the slice; a pane nothing narrows
    // offers its own box, which is every case this module could express before.
    let aperture = window.intersect(from_above)?;
    // ★★ R1685 — WHICH FRAME the marks under this window are expressed in, and
    // the two clipping kinds answer differently.
    //
    // A scroll's content is stored in the scroll's own frame with its origin at
    // the top-left, so the aperture is carried in by SUBTRACTING the scroll's own
    // position. A container introduces no frame at all: its children keep the
    // coordinates the container's own rect is in, so the aperture is already in
    // their frame and rides through unchanged.
    //
    // ★ R1713 — before this the scroll arm read its size out of a map of
    // window-absolute rectangles and a comment claimed that folded in "an outer
    // clip narrowing it". It did not: that map only translated, so the number it
    // produced was the node's own `w`/`h` every time and the whole prepass was
    // dead weight dressed as a fold. This is the fold it described.
    let declared = match node {
        Scene::Scroll(_) => Rect::new(0, 0, window.w, window.h),
        _ => window,
    };
    let (origin, size) = match node {
        Scene::Scroll(_) => (
            (aperture.x - window.x, aperture.y - window.y),
            (aperture.w, aperture.h),
        ),
        _ => ((aperture.x, aperture.y), (aperture.w, aperture.h)),
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
        _ => ((declared.w, declared.h), (0, 0)),
    };
    Some(Viewport {
        name: node
            .tag()
            .map_or_else(|| UNTAGGED.to_owned(), ToString::to_string),
        origin,
        size,
        declared,
        content,
        at,
        // ★ R1713 — against the DECLARED box, not the aperture: the runtime
        // clamps offsets against the node's own window and knows nothing about
        // who clips it, so a range read off a narrowed aperture would name
        // offsets the scene refuses to take.
        max: (
            max_scroll_offset(content.0, declared.w),
            max_scroll_offset(content.1, declared.h),
        ),
    })
}

/// The offset that shows `rect`, moving as little as possible from where the
/// viewport already is.
///
/// ★ R1713 — worked in the aperture's coordinates rather than the declared box's:
/// an offset has to land the mark inside what the chain above actually leaves
/// visible, and the two differ by exactly [`Viewport::origin`].
fn least_move(rect: Rect, viewport: &Viewport) -> (i32, i32) {
    let axis = |lo: u32, len: u32, at: i32, from: u32, size: u32, max: i32| -> i32 {
        let lo = i64::from(lo) - i64::from(from);
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
            viewport.origin.0,
            viewport.size.0,
            viewport.max.0,
        ),
        axis(
            rect.y,
            rect.h,
            viewport.at.1,
            viewport.origin.1,
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

    fn cut_tags(found: &[Cut]) -> Vec<String> {
        let mut v: Vec<String> = found.iter().filter_map(|c| c.tag.clone()).collect();
        v.sort();
        v
    }

    /// ★★★★★ R1711 — the case that made [`cut`] a separate derivation, taken
    /// from the screen that produced it.
    ///
    /// A 100-wide status chip whose last pixel is inside the window, and a
    /// 312-wide pane sliced by 119: [`out_of_sight`] reports **neither**, and
    /// it is right not to — the reader is looking at part of both. A floor
    /// derived from that answer is a floor at which the chip is a line.
    #[test]
    fn r1711_a_mark_the_window_slices_is_cut_though_it_is_not_out_of_sight() {
        let screen = boxed(
            Rect::new(0, 0, 1625, 100),
            "appbar",
            vec![
                text("state", Rect::new(1505, 10, 100, 12), "appbar.state"),
                boxed(Rect::new(1313, 20, 312, 60), "inspector", vec![]),
                text("ok", Rect::new(10, 10, 16, 12), "appbar.title"),
            ],
        );
        let window = (1506, 100);
        assert_eq!(
            out_of_sight(&screen, window, &mut stub_ink),
            Vec::new(),
            "nothing is out of SIGHT: a pixel of each is on screen",
        );
        let cuts = cut(&screen, window, &mut stub_ink);
        // The appbar itself is the root here, and the root is the surface —
        // it is not shown inside anything, so it is not judged (the rule
        // `out_of_sight` states and shares).
        assert_eq!(cut_tags(&cuts), ["appbar.state", "inspector"]);
        let chip = cuts
            .iter()
            .find(|c| c.tag.as_deref() == Some("appbar.state"))
            .expect("the chip is cut");
        assert_eq!(chip.short_by.right, 39, "5 characters of ink past 1506");
        assert_eq!((chip.short_by.left, chip.short_by.top), (0, 0));
    }

    /// ★★ R1711 — and scrolling is not a defect: a row the pane's range can
    /// bring fully into view is NOT cut, while the row past the end of the
    /// content is. Same fixture as the R1685 pair above, so the two
    /// derivations are read against one geometry.
    #[test]
    fn r1711_a_row_a_pane_can_reach_is_not_cut_and_one_past_its_content_is() {
        assert_eq!(
            cut_tags(&cut(&pane(0), (400, 400), &mut stub_ink)),
            ["row.c"]
        );
        // The hidden box publishes no range, so both overflowing rows are cut
        // — the same split R1685 pinned for the other question — and so is the
        // 300-tall content block itself, which the scrolling twin can reach.
        assert_eq!(
            cut_tags(&cut(&hidden_pane(), (400, 400), &mut stub_ink)),
            ["pane.content", "row.b", "row.c"]
        );
    }

    /// ★★ R1711 — the answer does not move when the reader does. A floor is a
    /// property of the size, so scrolling the pane changes what is out of sight
    /// and must change nothing here.
    #[test]
    fn r1711_what_is_cut_does_not_depend_on_where_the_reader_scrolled_to() {
        let parked = cut(&pane(0), (400, 400), &mut stub_ink);
        let scrolled = cut(&pane(200), (400, 400), &mut stub_ink);
        assert_eq!(cut_tags(&parked), cut_tags(&scrolled));
        assert_ne!(
            out_of_sight(&pane(0), (400, 400), &mut stub_ink),
            out_of_sight(&pane(200), (400, 400), &mut stub_ink),
            "while the other question's answer moves with the offset",
        );
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

    /// ★★★★★ A pane that sits off the window reports **once**, on the pane — the
    /// node whose placement is the repair — and R1713 is what made that true
    /// rather than merely claimed.
    ///
    /// Before the chain was folded, the rows inside were judged against the pane
    /// alone: `row.b` came back `Scrollable { to: (0, 152) }`, an offset that
    /// moves a pane no pixel of which is on screen. This module's own header
    /// calls a false *reachable* the error it exists to end, and this was one —
    /// the attribution rule was being satisfied by not looking rather than by
    /// the arithmetic. Now the pane's aperture is empty, the chain **seals**, and
    /// the rows below it are not this report's business at all.
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
        // ★ The report is the pane and nothing else. Counted rather than named,
        // so a row appearing under any other spelling fails this too.
        assert_eq!(
            found
                .iter()
                .filter_map(|o| o.tag.as_deref())
                .collect::<Vec<_>>(),
            ["pane"],
            "{found:?}"
        );
        // And `cut` seals at the same level, for the same reason.
        assert_eq!(cut_tags(&cut(&root, (200, 200), &mut stub_ink)), ["pane"]);
    }

    /// ★★★★★ R1713 — the fold, on the shape that paid for it.
    ///
    /// The analysis tool's node lab at 1506 wide, reduced to the three nodes that
    /// matter: an inspector pane declared 312 wide starting at 1313, so the
    /// window keeps 193 of it, and two marks inside — one in the part that
    /// survives and one in the 119 pixels that do not. The pane does not scroll
    /// horizontally, so nothing brings the second one back.
    ///
    /// Measured on the real screen before this fold existed: `lost` was **zero**
    /// with nine such marks on it, every one an action. A window floor went out
    /// 89 pixels too low on the strength of that answer.
    #[test]
    fn r1713_a_mark_inside_a_pane_the_window_slices_is_lost() {
        let screen = boxed(
            Rect::new(0, 0, 1625, 400),
            "screen",
            vec![Scene::Scroll(
                ScrollNode::new(
                    Rect::new(1313, 0, 312, 400),
                    boxed(
                        Rect::new(0, 0, 312, 400),
                        "inspector.body",
                        vec![
                            // Local x 20 -> window 1333: inside the 193 that survive.
                            text("keep", Rect::new(20, 10, 32, 12), "inspector.field"),
                            // Local x 281 -> window 1594: past the window entirely.
                            text("x", Rect::new(281, 10, 8, 12), "inspector.remove"),
                        ],
                    ),
                )
                .with_tag("inspector"),
            )],
        );
        let window = (1506, 400);
        let found = out_of_sight(&screen, window, &mut stub_ink);

        let gone = by_tag(&found, "inspector.remove")
            .expect("a mark outside the window is out of sight, whatever pane holds it");
        let Reach::Lost { short_by } = gone.reach else {
            panic!("that pane has no horizontal range, so nothing brings it back: {gone:?}");
        };
        // The aperture is 0..193 of the pane's own frame; the ink ends at 289.
        assert_eq!(short_by.right, 96, "{gone:?}");
        assert_eq!(gone.viewport.name, "inspector");
        assert_eq!(gone.viewport.size, (193, 400), "the aperture, not the box");
        assert_eq!(
            gone.viewport.declared,
            Rect::new(0, 0, 312, 400),
            "and the box it asked for, so the narrowing is readable"
        );
        assert_eq!(
            gone.viewport.max,
            (0, 0),
            "range from the box, not the slice"
        );

        // ★ The other half: the fold must not swallow the pane's surviving part.
        // Without this the assertion above passes for a fold that reports
        // everything inside a narrowed pane, which is the opposite error.
        assert!(
            by_tag(&found, "inspector.field").is_none(),
            "the field at window x=1333 is on screen: {found:?}"
        );
        assert_eq!(
            cut_tags(&cut(&screen, window, &mut stub_ink)),
            ["inspector", "inspector.body", "inspector.remove"],
            "the pane and its content block are sliced too, and say so"
        );
    }

    /// ★★★★★ R1713 — the arm the safety rule needed, on the geometry that
    /// exposed its absence.
    ///
    /// Two marks in one narrowed pane, both scrolled out of view: a row as wide
    /// as the pane, of which the reader reaches all but the right edge, and a
    /// glyph sitting in the strip the window removed. Before this arm existed
    /// both answered `lost`, and [`crate::shrink`]'s rule — *a concession may
    /// clip and may never lose* — therefore failed on the row.
    ///
    /// Measured on the analysis tool's node lab at 1595x360: 19 `lost`, of which
    /// 6 were marks no pixel of which is reachable and 13 were rows like the
    /// first one here.
    #[test]
    fn r1713_a_row_whose_edge_is_unreachable_is_cut_and_a_glyph_beyond_it_is_lost() {
        let screen = narrowed_pane();
        let found = out_of_sight(&screen, (400, 400), &mut stub_ink);
        let row = by_tag(&found, "row").expect("the row is scrolled out of view");
        let Reach::Clipped { short_by } = row.reach else {
            panic!("the reader reaches all but its right edge: {row:?}");
        };
        assert_eq!(short_by.right, 28, "310 wide in a 282 aperture: {row:?}");
        assert_eq!(row.reach.wire_word(), "clipped");
        assert!(!row.reach.is_lost(), "a clip is not a loss");

        let glyph = by_tag(&found, "row.remove.glyph").expect("the glyph is off screen");
        assert!(
            glyph.reach.is_lost(),
            "286..294 and 0..282 are disjoint, so nothing reaches any of it: {glyph:?}"
        );
        // ★ Both are `cut()` — that predicate asks "can this be shown whole",
        // which is one question with one answer for the pair, and having only it
        // is what made the rule unenforceable. The pane and its body are cut on
        // the same axis and for the same reason: 310 of width in 282 of aperture.
        assert_eq!(
            cut_tags(&cut(&screen, (400, 400), &mut stub_ink)),
            ["body", "pane", "row", "row.remove.glyph"]
        );
    }

    /// A pane 310 wide inside a box that clips it to 282, holding a full-width
    /// row scrolled out of view and the glyph of that row's right-hand button.
    ///
    /// The analysis tool's inspector at a conceded width, reduced to two marks.
    fn narrowed_pane() -> Scene {
        let pane = Scene::Scroll(
            ScrollNode::new(
                Rect::new(0, 0, 310, 100),
                boxed(
                    Rect::new(0, 0, 310, 400),
                    "body",
                    vec![
                        // Scrolled out of view (the pane shows 0..100), 310 wide.
                        Scene::Box(
                            crate::scene::BoxNode::filled(
                                Rect::new(0, 200, 310, 20),
                                crate::style::Color::rgb(0, 0, 0),
                            )
                            .with_tag("row"),
                        ),
                        // The `×` in that row's remove button: 8 of ink at 286.
                        text("x", Rect::new(286, 200, 8, 12), "row.remove.glyph"),
                    ],
                ),
            )
            .with_tag("pane"),
        );
        let mut outer = ContainerNode::new(vec![pane]);
        outer.rect = Rect::new(0, 0, 282, 100);
        outer.tag = Some("outer".into());
        outer.layout =
            crate::style::LayoutStyle::new().with_overflow(crate::style::Overflow::Hidden);
        Scene::Container(outer)
    }

    /// ⚠ R1713 — what the fold does NOT do, pinned rather than described.
    ///
    /// The offset in [`Reach::Scrollable`] belongs to the row's own viewport. Here
    /// an inner pane sits past the right edge of an outer scrolling pane, so
    /// seeing the inner row needs the OUTER pane moved as well — and the answer
    /// names only the inner offset. It is not a false reachable (some pair of
    /// offsets does show the row) but it is an incomplete recipe ⇒
    /// `debt-a-scroll-offset-is-named-for-one-viewport-of-a-chain`.
    ///
    /// Measured, and the reason this is a pin rather than a repair: every clip
    /// chain on all three analysis-tool screens is **one** level deep, so no
    /// reader can meet this today.
    #[test]
    fn r1713_a_deeper_chain_names_only_its_own_offset() {
        let inner = Scene::Scroll(
            ScrollNode::new(
                // Starts past the outer pane's 100-wide window, inside its
                // 300-wide content: the outer range is what brings it into view.
                Rect::new(150, 0, 100, 100),
                boxed(
                    Rect::new(0, 0, 100, 300),
                    "inner.content",
                    vec![text("b", Rect::new(0, 240, 60, 12), "inner.row")],
                ),
            )
            .with_tag("inner"),
        );
        let outer = Scene::Scroll(
            ScrollNode::new(
                Rect::new(0, 0, 100, 100),
                boxed(Rect::new(0, 0, 300, 100), "outer.content", vec![inner]),
            )
            .with_tag("outer"),
        );
        let found = out_of_sight(&outer, (400, 400), &mut stub_ink);
        let row = by_tag(&found, "inner.row").expect("the row is off screen");
        assert_eq!(
            row.reach,
            Reach::Scrollable { to: (0, 152) },
            "the inner offset — and the outer pane also has to move to 150, \
             which this answer does not say: {row:?}"
        );
        assert_eq!(row.viewport.name, "inner");
        // The chain is folded, though: the inner pane is offered only the part of
        // itself the outer RANGE covers, which is all of it here (150..250 lies
        // inside the outer's reachable 0..300) — so nothing is falsely lost.
        assert_eq!(row.viewport.size, (100, 100));
        assert_eq!(row.viewport.declared, Rect::new(0, 0, 100, 100));
    }

    /// ★★★ R1713 — the offset is solved in the APERTURE's coordinates, not the
    /// declared box's.
    ///
    /// Every other fixture here has an aperture starting at zero, where the two
    /// are the same number and any arithmetic that confuses them passes. Here an
    /// outer box clips the pane's left 50 pixels away, so the offset that shows a
    /// row has to account for the reader only being able to see the pane's right
    /// 100 — and an answer that ignores that lands the row in the strip nobody
    /// can see.
    #[test]
    fn r1713_an_offset_is_solved_in_the_apertures_coordinates() {
        let pane = Scene::Scroll(
            ScrollNode::new(
                Rect::new(0, 0, 150, 100),
                boxed(
                    Rect::new(0, 0, 300, 100),
                    "content",
                    vec![text("xx", Rect::new(200, 10, 20, 12), "row")],
                ),
            )
            .with_tag("pane"),
        );
        let mut outer = ContainerNode::new(vec![pane]);
        outer.rect = Rect::new(50, 0, 150, 100);
        outer.tag = Some("outer".into());
        outer.layout =
            crate::style::LayoutStyle::new().with_overflow(crate::style::Overflow::Hidden);

        let found = out_of_sight(&Scene::Container(outer), (400, 400), &mut stub_ink);
        let row = by_tag(&found, "row").expect("200 is past the pane's 150-wide window");
        assert_eq!(row.viewport.origin, (50, 0), "the aperture starts at 50");
        assert_eq!(row.viewport.size, (100, 100), "and is 100 wide, not 150");
        assert_eq!(
            row.viewport.declared,
            Rect::new(0, 0, 150, 100),
            "while the pane still asked for 150"
        );
        assert_eq!(
            row.viewport.max,
            (150, 0),
            "the range comes off the declared box, so the runtime will take it"
        );
        // ★ The row is judged by its INK, which the stub measures as 16 for two
        // characters — not the 20 the view promised. That is this module's rule
        // and it is load-bearing in the arithmetic below, so it is asserted
        // rather than assumed: the first draft of this test computed 70 from the
        // promised width and the implementation was right.
        assert_eq!(row.rect, Rect::new(200, 10, 16, 12));
        // At 66 the row (content 200..216) is drawn at 150-66+... = inside the
        // 50..150 the reader can see, ending exactly at 150. At 150 — what an
        // offset solved against the DECLARED box answers — it is drawn at
        // 50..66, also visible, so the wrong answer is a WORSE answer rather
        // than an absurd one: it scrolls more than twice as far as it needs to.
        assert_eq!(row.reach, Reach::Scrollable { to: (66, 0) }, "{row:?}");
    }

    /// ★★★ R1713 — a scrolling viewport is offered what the level above can
    /// **ever** show, not what it is showing now. Otherwise every pane scrolled
    /// out of an outer pane would report its whole content lost.
    #[test]
    fn r1713_a_pane_scrolled_out_of_an_outer_pane_still_offers_its_rows() {
        // The outer pane is parked at 0, so the inner pane at 150..250 is
        // entirely off view right now — and entirely reachable.
        let inner = Scene::Scroll(
            ScrollNode::new(
                Rect::new(150, 0, 100, 100),
                boxed(
                    Rect::new(0, 0, 100, 100),
                    "inner.content",
                    vec![text("row", Rect::new(0, 10, 24, 12), "inner.row")],
                ),
            )
            .with_tag("inner"),
        );
        let outer = Scene::Scroll(
            ScrollNode::new(
                Rect::new(0, 0, 100, 100),
                boxed(Rect::new(0, 0, 300, 100), "outer.content", vec![inner]),
            )
            .with_tag("outer"),
        );
        let found = out_of_sight(&outer, (400, 400), &mut stub_ink);
        let row = by_tag(&found, "inner.row").expect("off screen while the outer is at 0");
        assert!(
            !row.reach.is_lost(),
            "the outer range reaches the pane, so its rows are not lost: {row:?}"
        );
        assert!(
            cut_tags(&cut(&outer, (400, 400), &mut stub_ink)).is_empty(),
            "and nothing here is cut: scrolling is not a defect"
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
