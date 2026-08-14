//! R1688 — **where the canvas is pointed**: the zoom and the pan, and the two
//! questions every node editor asks of them.
//!
//! [`layout`](crate::layout) and [`arrange`](crate::arrange) decide where the
//! nodes *are*. This module decides what the reader is *looking at*, which is a
//! different fact with a different lifetime — it is not in the document, it is
//! not undoable, and two windows onto one graph have one each.
//!
//! # Why this is here rather than in each editor
//!
//! Measured across this tree before it was written: two node canvases, two
//! hand-rolled copies of the same affine and the same fold.
//! `hello-node-editor` frames the graph by taking the union box of its nodes,
//! dividing the window by it, clamping into a zoom range and pinning the box
//! centre at the window centre; `hello-node-lab` was about to write the fourth
//! coordinate conversion of its own to do it again. Both also anchor a zoom —
//! keep the point under the cursor still while the scale changes — and both had
//! written that arithmetic inline.
//!
//! None of that is a matter of taste, and all of it is one page of algebra that
//! is wrong in a way nobody sees: a fit that clamps at the zoom floor still
//! *reports success* while showing a fraction of the graph, and an anchored zoom
//! whose inverse projection has drifted from its forward one moves the graph out
//! from under the cursor by a pixel per notch.
//!
//! # Past the reference, in four places
//!
//! The reference toolkit's graphics view has `fitInView` and a transform. This
//! module says four things it cannot:
//!
//! * **The margin declares its own frame of reference.** [`Margin::Canvas`] is
//!   padding added to the graph's box *in graph units*, so it scales with the
//!   diagram; [`Margin::Screen`] is a gutter kept *in pixels*, so it is the same
//!   width at every zoom. The two produce different scales for the same graph
//!   and both are wanted — the behaviour canon this tree reproduces uses the
//!   first, the DCC/engine "frame selection" idiom uses the second. The
//!   reference has one, unnamed, and it is neither: it insets by the view's
//!   frame width, which is why its own documentation tells callers to call it
//!   twice.
//! * **A fit says whether it fitted.** [`Fitted::complete`] is false when the
//!   zoom range would not stretch far enough to hold the graph, which is the one
//!   case where the button a person pressed did not do what it says. The
//!   reference returns `void`.
//! * **The zoom range is a value.** [`ZoomRange`] is constructed validated, so
//!   there is no path on which a clamp is asked to order two numbers that are
//!   not ordered, and the same range governs [`Fit`] and
//!   [`Camera::zoomed_at`] — one declaration rather than a constant re-typed at
//!   each call site.
//! * **Revealing is minimal and idempotent.** [`Camera::reveal`] moves the pan
//!   by exactly as much as it takes and answers the camera unchanged when the
//!   box is already on screen — the semantics `pinion-core`'s `reach` module
//!   already publishes for a scrolling pane, held here for a scaling one. (This
//!   crate is pure data and depends on neither, which is why that is a sentence
//!   rather than a link.)
//!
//! # The convention
//!
//! One affine, stated once and inverted once:
//!
//! ```text
//! screen = world * zoom + pan
//! world  = (screen - pan) / zoom
//! ```
//!
//! `pan` is where the world's origin lands on the screen, in screen pixels. A
//! consumer that stores a scroll *offset* instead (the opposite sign) converts
//! at its own boundary; both of this tree's canvases do, in one place each.

use crate::layout::Extent;
use crate::model::{Document, Node, NodeKind, TreeId};

/// The scales a canvas may be shown at.
///
/// A validated pair rather than two loose numbers, because every consumer of a
/// zoom needs the same two and the failure mode of passing them the wrong way
/// round is a clamp that panics ([`f64::clamp`]) or silently inverts. Built
/// through [`new`](Self::new), which refuses anything that is not a usable
/// range, so nothing downstream has to re-check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomRange {
    min: f64,
    max: f64,
}

impl ZoomRange {
    /// The range `min..=max`, or `None` when it is not one.
    ///
    /// Refused: a non-finite bound, a non-positive scale (a zoom of zero shows
    /// nothing and a negative one mirrors the graph), and `max < min`.
    #[must_use]
    pub fn new(min: f64, max: f64) -> Option<Self> {
        (min.is_finite() && max.is_finite() && min > 0.0 && max >= min).then_some(Self { min, max })
    }

    /// The smallest scale.
    #[must_use]
    pub const fn min(&self) -> f64 {
        self.min
    }

    /// The largest scale.
    #[must_use]
    pub const fn max(&self) -> f64 {
        self.max
    }

    /// `zoom` brought into the range. A non-finite input answers
    /// [`min`](Self::min) rather than propagating a `NaN` into a camera.
    #[must_use]
    pub fn clamp(&self, zoom: f64) -> f64 {
        if zoom.is_finite() {
            zoom.clamp(self.min, self.max)
        } else {
            self.min
        }
    }
}

/// How much clear space a [`Fit`] keeps around the graph — **and in whose
/// units**, which is the half a single number cannot say.
///
/// The two are not interchangeable and neither is a scaling of the other: a
/// canvas margin is part of the box being fitted, so it shrinks on screen as the
/// graph grows; a screen margin is taken off the viewport, so it is the same
/// gutter whatever is being shown. Consumers of this crate want both, and a
/// consumer that got the one it did not mean would find out from the pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Margin {
    /// Padding added to the graph's bounding box, in the canvas units
    /// [`Node::x`] is in.
    Canvas(i32),
    /// A gutter kept between the graph and the viewport's edge, in screen
    /// pixels.
    ///
    /// A gutter wider than half the viewport would leave nothing to draw in;
    /// rather than refuse, the fit keeps one pixel of viewport and answers
    /// [`Fitted::complete`] `false`, which is the same report it gives for any
    /// other graph the range cannot hold.
    Screen(i32),
}

/// Where a canvas is pointed: `screen = world * zoom + pan`.
///
/// A value, not a handle — a camera is computed, compared and stored by the
/// application, which owns the signal it lives in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Screen pixels per canvas unit.
    pub zoom: f64,
    /// Where the canvas origin lands on the screen, in screen pixels.
    pub pan: (f64, f64),
}

impl Camera {
    /// A camera at `zoom`, with the canvas origin at `pan`.
    #[must_use]
    pub const fn new(zoom: f64, pan: (f64, f64)) -> Self {
        Self { zoom, pan }
    }

    /// A canvas point, in screen pixels.
    #[must_use]
    pub fn project(&self, world: (f64, f64)) -> (f64, f64) {
        (
            world.0.mul_add(self.zoom, self.pan.0),
            world.1.mul_add(self.zoom, self.pan.1),
        )
    }

    /// A screen pixel, in canvas units — the exact inverse of
    /// [`project`](Self::project).
    ///
    /// One function rather than an affine each consumer re-derives, because the
    /// two directions drifting apart is the defect that makes a graph slide
    /// under the cursor: R1653 found three copies of this conversion in one
    /// screen and R1183 found the same asymmetry in the other.
    #[must_use]
    pub fn unproject(&self, screen: (f64, f64)) -> (f64, f64) {
        (
            (screen.0 - self.pan.0) / self.zoom,
            (screen.1 - self.pan.1) / self.zoom,
        )
    }

    /// The camera at `zoom` that puts canvas point `world` exactly under screen
    /// pixel `anchor`.
    ///
    /// The solve every viewport write is: "place this graph point there".
    /// [`zoomed_at`](Self::zoomed_at) is this with the world point read off the
    /// current camera, and a canvas that scrolls to a coordinate is this with
    /// the anchor at the viewport's corner — one derivation with two call
    /// shapes, rather than the same algebra typed out at each.
    #[must_use]
    pub fn pinned(zoom: f64, world: (f64, f64), anchor: (f64, f64)) -> Self {
        Self {
            zoom,
            pan: (
                (-world.0).mul_add(zoom, anchor.0),
                (-world.1).mul_add(zoom, anchor.1),
            ),
        }
    }

    /// The camera at `target` zoom that keeps the canvas point currently under
    /// `anchor` — a screen pixel — exactly where it is.
    ///
    /// The wheel-zoom and the framed-zoom idiom, and the one place the pair of
    /// projections has to agree: the anchor is unprojected at the old scale and
    /// re-pinned at the new one, so a change to either direction moves both.
    #[must_use]
    pub fn zoomed_at(&self, target: f64, anchor: (f64, f64), range: &ZoomRange) -> Self {
        Self::pinned(range.clamp(target), self.unproject(anchor), anchor)
    }

    /// The camera that brings `box` — a canvas-unit rectangle `(left, top,
    /// right, bottom)` — onto a `viewport`-sized screen, **moving as little as
    /// possible** and not at all when it is already there.
    ///
    /// The scale is untouched: revealing is a pan. A box larger than the
    /// viewport on an axis is aligned to its leading edge, which is what every
    /// scroller does and what `pinion-core`'s `Reach::Scrollable` answers for
    /// the same case.
    #[must_use]
    pub fn reveal(&self, bounds: (i32, i32, i32, i32), viewport: (u32, u32)) -> Self {
        let (left, top, right, bottom) = bounds;
        let axis = |lo: i32, hi: i32, pan: f64, size: f64| {
            let (lo, hi) = (
                f64::from(lo) * self.zoom + pan,
                f64::from(hi) * self.zoom + pan,
            );
            if lo < 0.0 || hi - lo > size {
                // Off the leading edge, or too big to hold: show the start.
                pan - lo
            } else if hi > size {
                pan + (size - hi)
            } else {
                pan
            }
        };
        Self {
            zoom: self.zoom,
            pan: (
                axis(left, right, self.pan.0, f64::from(viewport.0)),
                axis(top, bottom, self.pan.1, f64::from(viewport.1)),
            ),
        }
    }
}

/// What a [`Fit`] answered.
///
/// ★ Named `Fitted` and not `Framed`, which the first draft called it and the
/// compiler refused: in this crate a **frame** is already a thing — a
/// [`NodeBody::Frame`](crate::NodeBody) region a node can be put inside — so
/// "framed" would have been one word for two ideas in the one crate that owns
/// both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fitted {
    /// Where to point the canvas.
    pub camera: Camera,
    /// The graph's own bounding box, `(left, top, right, bottom)` in canvas
    /// units — **without** the margin, which is a parameter of the fit and not
    /// a property of the graph.
    pub bounds: (i32, i32, i32, i32),
    /// Whether [`bounds`](Self::bounds) is entirely on screen at
    /// [`camera`](Self::camera).
    ///
    /// False when the graph is bigger than [`ZoomRange::min`] can shrink it to.
    /// The reference has no way to say this, so an editor built on it reports a
    /// successful fit and shows a corner of the graph — and the person presses
    /// the button again.
    pub complete: bool,
}

/// Point the canvas at everything: the **fit-to-view** of a node editor.
///
/// Configure, then [`run`](Self::run) or [`boxes`](Self::boxes) — the shape
/// [`Layered`](crate::Layered) and [`Organic`](crate::Organic) have.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// The scales the result may take.
    pub zoom: ZoomRange,
    /// The clear space to keep, and in whose units.
    pub margin: Margin,
}

impl Fit {
    /// Frame a set of `(position, extent)` boxes into a `viewport`-sized
    /// screen. `None` when there is nothing to frame or the viewport has no
    /// area — the two cases where a camera would have to be invented.
    ///
    /// Takes boxes rather than a document because the extent of a card is
    /// frequently *painted* rather than stored: a screen whose card height is a
    /// function of the rows it draws knows the box and the document does not.
    /// [`run`](Self::run) is the same fit for a consumer that does keep them.
    #[must_use]
    pub fn boxes(
        &self,
        boxes: impl IntoIterator<Item = ((i32, i32), Extent)>,
        viewport: (u32, u32),
    ) -> Option<Fitted> {
        let mut bounds: Option<(i32, i32, i32, i32)> = None;
        for ((x, y), extent) in boxes {
            let (right, bottom) = (
                x.saturating_add(extent.width.max(0)),
                y.saturating_add(extent.height.max(0)),
            );
            bounds = Some(match bounds {
                None => (x, y, right, bottom),
                Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(right), b.max(bottom)),
            });
        }
        let bounds = bounds?;
        if viewport.0 == 0 || viewport.1 == 0 {
            return None;
        }
        Some(self.frame(bounds, viewport))
    }

    /// Frame every node of `tree` the `extent` callback answers for.
    ///
    /// `None` from the callback means *this node is not framed* — a frame
    /// region drawn around others, a node the application is hiding — which is
    /// a decision this crate refuses to make on a consumer's behalf: an editor
    /// that draws its group boxes wants them inside the fit, and one whose
    /// frames are derived from their members does not care either way.
    #[must_use]
    pub fn run<K: NodeKind>(
        &self,
        document: &Document<K>,
        tree: TreeId,
        viewport: (u32, u32),
        extent: impl Fn(&Node<K>) -> Option<Extent>,
    ) -> Option<Fitted> {
        let host = document.tree(tree)?;
        self.boxes(
            host.nodes()
                .filter_map(|node| extent(node).map(|e| ((node.x, node.y), e))),
            viewport,
        )
    }

    /// The arithmetic, once: choose the scale, then centre the box under it.
    fn frame(&self, bounds: (i32, i32, i32, i32), viewport: (u32, u32)) -> Fitted {
        let (left, top, right, bottom) = bounds;
        // A box with no area still has a position, and a graph of one
        // zero-sized node should be centred rather than divided by. One unit is
        // the smallest box the arithmetic can be asked about.
        let (own_w, own_h) = (
            f64::from((right - left).max(1)),
            f64::from((bottom - top).max(1)),
        );
        let (view_w, view_h) = (f64::from(viewport.0), f64::from(viewport.1));
        // The margin is applied to whichever side of the division it names, and
        // the padded box is what the SCALE is chosen against. The centring
        // below uses the padded box too, so a canvas margin is clear space
        // inside the viewport rather than a shift.
        let (fit_w, fit_h, pad) = match self.margin {
            Margin::Canvas(pad) => {
                let pad = f64::from(pad.max(0));
                (
                    view_w / pad.mul_add(2.0, own_w),
                    view_h / pad.mul_add(2.0, own_h),
                    pad,
                )
            }
            Margin::Screen(gutter) => {
                let gutter = f64::from(gutter.max(0));
                (
                    (gutter.mul_add(-2.0, view_w)).max(1.0) / own_w,
                    (gutter.mul_add(-2.0, view_h)).max(1.0) / own_h,
                    0.0,
                )
            }
        };
        let zoom = self.zoom.clamp(fit_w.min(fit_h));
        // Centre the padded box: the canon's own arithmetic, and the same thing
        // "pin the box centre at the viewport centre" says — written this way
        // because the pan IS where the origin lands and this is that number
        // directly, with no second subtraction to get it wrong.
        let (box_w, box_h) = (pad.mul_add(2.0, own_w), pad.mul_add(2.0, own_h));
        let camera = Camera::new(
            zoom,
            (
                box_w.mul_add(-zoom, view_w) / 2.0 - (f64::from(left) - pad) * zoom,
                box_h.mul_add(-zoom, view_h) / 2.0 - (f64::from(top) - pad) * zoom,
            ),
        );
        Fitted {
            camera,
            bounds,
            // Judged on the graph's OWN box, at the scale that was chosen: the
            // margin is a nicety and losing it is not the failure this reports.
            // A hair of tolerance because the scale is a ratio of the same two
            // numbers being compared, and an exact fit must not report itself
            // as an overflow of one part in 2^52.
            complete: own_w * zoom <= view_w + 1e-6 && own_h * zoom <= view_h + 1e-6,
        }
    }
}
