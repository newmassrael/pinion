//! R1591 §5.32 §2 #7 — a region of the painted surface is a **value**.
//!
//! # Why this module exists
//!
//! Selecting by dragging a shape over what is drawn is one gesture that every
//! canvas has: a node editor's marquee and lasso, a timeline's range, a chart's
//! brush, a diagram editor's rubber band. The framework had exactly one of the
//! shapes — [`Scene::hit_test_region`](crate::scene::Scene::hit_test_region)
//! takes a rectangle — and exactly one of the two things you can mean by
//! "covered": *touches*. R1590 measured the absence from the other end, against
//! Blender's `NODE_OT_select_circle` and `NODE_OT_select_lasso`, and recorded
//! that those are not node-graph capabilities at all: they test a region
//! against `node->runtime->draw_bounds`, the **drawn** rectangle, which is a
//! question for the layer that knows what was painted where. This is that
//! layer.
//!
//! # A value, not a pen
//!
//! Qt's floor is `QGraphicsScene::items(const QPainterPath &, Qt::ItemSelectionMode)`
//! and its `QPolygonF` overload, so arbitrary-shape queries exist there. What
//! is different here is that a [`Region`] is a **value** — comparable, copyable,
//! with no interior state — and therefore expressible on a wire: `scene/locate`
//! takes one, so something with no pointer at all can ask what a lasso covers.
//! A `QPainterPath` is an opaque mutable object that can only be built
//! in-process, which is why no Qt application can be asked that from outside it.
//! (The wire spelling lives in `pinion-rpc`, beside every other scene type's —
//! [`Rect`] itself is not `serde` either.)
//!
//! # The fit belongs to the question
//!
//! [`RegionFit`] is an argument. Qt's rubber band takes its mode from
//! `QGraphicsView::rubberBandSelectionMode`, a **view property**, so two
//! selections in one view cannot mean different things and nothing records which
//! mode a given selection used.
//!
//! Qt's `Qt::ItemSelectionMode` has four arms because it crosses
//! contains/intersects with shape/bounding-rect. Here there are two, and that is
//! a fact about this framework rather than an omission: a [`Scene`] node's
//! extent *is* a [`Rect`] — every hit test in the tree, including the pointer's,
//! is rectangular — so an item's shape and its bounding rectangle are the same
//! thing and the other two arms would be aliases.
//!
//! # Integer arithmetic, on purpose
//!
//! Everything here is `i64` over pixel coordinates. A selection is a question
//! about which pixels a shape covers, and floating point would make the answer
//! depend on rounding at the edges — the one place a user notices.
//!
//! [`Scene`]: crate::scene::Scene
//! [`Rect`]: crate::scene::Rect

use std::fmt;

use crate::scene::Rect;

/// A point in window-absolute logical pixels.
///
/// Signed, because a lasso drawn from inside a scrolled view resolves against
/// content that may sit at a negative offset from the window origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// Horizontal position.
    pub x: i64,
    /// Vertical position.
    pub y: i64,
}

impl Point {
    /// A point.
    #[must_use]
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }
}

/// Whether a node has to be *inside* the region or merely to *touch* it.
///
/// Qt's `Qt::ItemSelectionMode`, minus the two arms that would be aliases here —
/// see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegionFit {
    /// The node shares at least one pixel with the region. Qt's
    /// `IntersectsItemShape`, and what
    /// [`Scene::hit_test_region`](crate::scene::Scene::hit_test_region) has
    /// always meant.
    #[default]
    Intersects,
    /// The region covers the whole node. Qt's `ContainsItemShape`.
    Contains,
}

impl fmt::Display for RegionFit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Intersects => "intersects",
            Self::Contains => "contains",
        })
    }
}

/// The shape a region select was drawn with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    /// A rectangle. The marquee every canvas starts with.
    Rect(Rect),
    /// A disc. Blender's `NODE_OT_select_circle`, and the shape a brush tool
    /// paints a selection with.
    Circle {
        /// Centre.
        centre: Point,
        /// Radius, in pixels. Zero covers nothing.
        radius: u32,
    },
    /// A closed polygon, in the order it was drawn. Blender's
    /// `NODE_OT_select_lasso`.
    ///
    /// **Closed by derivation**: the last vertex joins the first, so a caller
    /// never repeats a point to close the loop — the way Qt's `QPolygonF` and
    /// Blender's own lasso buffer both require. Repeating it is harmless; it
    /// adds a zero-length edge.
    ///
    /// May be concave and may cross itself; the interior is decided by the
    /// even-odd rule, which is what a hand-drawn lasso means.
    Lasso(Vec<Point>),
}

/// Why a region could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegionError {
    /// A lasso with fewer than three vertices, which bounds no area.
    ///
    /// Named rather than answered with an empty result. Qt's
    /// `QGraphicsScene::items(QPolygonF, ..)` returns a `QList`, which has no
    /// channel for this — so there, "your lasso was degenerate" and "nothing is
    /// there" are the same value.
    LassoTooShort {
        /// How many vertices it had.
        vertices: usize,
    },
    /// A shape that bounds no pixels: a zero-area rectangle, or a circle of
    /// radius zero.
    Empty,
}

impl fmt::Display for RegionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LassoTooShort { vertices } => write!(
                f,
                "a lasso needs three vertices to bound an area, and this has {vertices}"
            ),
            Self::Empty => f.write_str("the region covers no pixels"),
        }
    }
}

impl std::error::Error for RegionError {}

impl Region {
    /// A rectangular region.
    #[must_use]
    pub const fn rect(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self::Rect(Rect::new(x, y, w, h))
    }

    /// A circular region.
    #[must_use]
    pub const fn circle(x: i64, y: i64, radius: u32) -> Self {
        Self::Circle {
            centre: Point::new(x, y),
            radius,
        }
    }

    /// A lasso from a drawn path.
    pub fn lasso(points: impl IntoIterator<Item = (i64, i64)>) -> Self {
        Self::Lasso(points.into_iter().map(|(x, y)| Point::new(x, y)).collect())
    }

    /// Whether this region can be asked about at all.
    ///
    /// # Errors
    ///
    /// See [`RegionError`]. Checked once, by the caller that owns the gesture,
    /// rather than folded into an empty answer — see [`RegionError::LassoTooShort`].
    pub fn validate(&self) -> Result<(), RegionError> {
        match self {
            Self::Rect(r) if r.w == 0 || r.h == 0 => Err(RegionError::Empty),
            Self::Circle { radius: 0, .. } => Err(RegionError::Empty),
            Self::Lasso(points) if points.len() < 3 => Err(RegionError::LassoTooShort {
                vertices: points.len(),
            }),
            _ => Ok(()),
        }
    }

    /// The smallest rectangle holding the whole region, clamped into the
    /// unsigned pixel space a [`Rect`] uses.
    ///
    /// This is what the scene walk prunes and translates with, so a shape query
    /// descends exactly the subtrees a rectangular one would — which is why the
    /// rectangle path's behaviour is unchanged by this module existing.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        let (min_x, min_y, max_x, max_y) = match self {
            Self::Rect(r) => {
                return *r;
            }
            Self::Circle { centre, radius } => {
                let r = i64::from(*radius);
                (centre.x - r, centre.y - r, centre.x + r, centre.y + r)
            }
            Self::Lasso(points) => {
                let Some(first) = points.first() else {
                    return Rect::new(0, 0, 0, 0);
                };
                points.iter().fold(
                    (first.x, first.y, first.x, first.y),
                    |(lx, ty, rx, by), p| (lx.min(p.x), ty.min(p.y), rx.max(p.x), by.max(p.y)),
                )
            }
        };
        clamped_rect(min_x, min_y, max_x, max_y)
    }

    /// Whether `rect` satisfies `fit` against this region.
    ///
    /// `rect` is in the same window-absolute space the region is. A zero-area
    /// rect is covered by nothing, which is the rule
    /// [`Scene::hit_test_region`](crate::scene::Scene::hit_test_region) has
    /// always followed.
    #[must_use]
    pub fn covers(&self, rect: Rect, fit: RegionFit) -> bool {
        self.covers_at(rect, (0, 0), fit)
    }

    /// The same question for a rect that is **stored** somewhere else: `rect`
    /// placed at `offset` from the origin the region is stated in.
    ///
    /// This is what a scene walk needs. Geometry inside a
    /// [`Scene::Scroll`](crate::scene::Scene::Scroll) is stored scroll-local, so
    /// a node scrolled up sits at a NEGATIVE offset from the window origin —
    /// which is why the arithmetic here is signed and why the offset is applied
    /// rather than baked into a [`Rect`] that could not hold it.
    #[must_use]
    pub fn covers_at(&self, rect: Rect, offset: (i64, i64), fit: RegionFit) -> bool {
        if rect.w == 0 || rect.h == 0 {
            return false;
        }
        let Some(left) = i64::from(rect.x).checked_add(offset.0) else {
            return false;
        };
        let Some(top) = i64::from(rect.y).checked_add(offset.1) else {
            return false;
        };
        let (right, bottom) = (left + i64::from(rect.w), top + i64::from(rect.h));
        match self {
            Self::Rect(r) => {
                if r.w == 0 || r.h == 0 {
                    return false;
                }
                let (rl, rt) = (i64::from(r.x), i64::from(r.y));
                let (rr, rb) = (rl + i64::from(r.w), rt + i64::from(r.h));
                match fit {
                    RegionFit::Intersects => left < rr && rl < right && top < rb && rt < bottom,
                    RegionFit::Contains => rl <= left && rt <= top && rr >= right && rb >= bottom,
                }
            }
            Self::Circle { centre, radius } => {
                let r = i64::from(*radius);
                if r == 0 {
                    return false;
                }
                let r2 = r * r;
                match fit {
                    // The nearest point of the rect to the centre. A half-open
                    // rect's last covered pixel is `right - 1`.
                    RegionFit::Intersects => {
                        let nx = centre.x.clamp(left, right - 1);
                        let ny = centre.y.clamp(top, bottom - 1);
                        squared(centre.x - nx, centre.y - ny) <= r2
                    }
                    // The farthest corner, which decides the whole rect.
                    RegionFit::Contains => [
                        (left, top),
                        (right - 1, top),
                        (left, bottom - 1),
                        (right - 1, bottom - 1),
                    ]
                    .into_iter()
                    .all(|(x, y)| squared(centre.x - x, centre.y - y) <= r2),
                }
            }
            Self::Lasso(points) => {
                if points.len() < 3 {
                    return false;
                }
                let corners = [
                    Point::new(left, top),
                    Point::new(right - 1, top),
                    Point::new(right - 1, bottom - 1),
                    Point::new(left, bottom - 1),
                ];
                let crosses = edges(points).any(|(a, b)| {
                    corners
                        .iter()
                        .zip(corners.iter().cycle().skip(1))
                        .any(|(c, d)| segments_cross(a, b, *c, *d))
                });
                match fit {
                    // Touching means any of three things, and all three are
                    // needed: an edge crossing catches a lasso drawn straight
                    // through, a corner inside catches one that swallows a
                    // part, and a vertex inside catches a lasso drawn wholly
                    // within one node.
                    RegionFit::Intersects => {
                        crosses
                            || corners.iter().any(|c| self.holds(*c))
                            || points
                                .iter()
                                .any(|p| p.x >= left && p.x < right && p.y >= top && p.y < bottom)
                    }
                    // Every corner inside AND no edge crossing: the second half
                    // is what a CONCAVE lasso needs, since a bite taken out of
                    // the middle leaves all four corners inside.
                    RegionFit::Contains => !crosses && corners.iter().all(|c| self.holds(*c)),
                }
            }
        }
    }

    /// Whether a point is inside the region.
    ///
    /// Even-odd for a lasso, which is what a hand-drawn loop that crosses itself
    /// means, and what SVG's `fill-rule: evenodd` and Blender's own lasso both
    /// use.
    #[must_use]
    pub fn holds(&self, point: Point) -> bool {
        match self {
            Self::Rect(r) => {
                let (l, t) = (i64::from(r.x), i64::from(r.y));
                r.w > 0
                    && r.h > 0
                    && point.x >= l
                    && point.x < l + i64::from(r.w)
                    && point.y >= t
                    && point.y < t + i64::from(r.h)
            }
            Self::Circle { centre, radius } => {
                let r = i64::from(*radius);
                r > 0 && squared(centre.x - point.x, centre.y - point.y) <= r * r
            }
            Self::Lasso(points) => {
                if points.len() < 3 {
                    return false;
                }
                // Even-odd ray casting along +x. The half-open `y` comparison
                // counts a vertex exactly once, which is what keeps a ray that
                // grazes a vertex from double-counting it.
                let mut inside = false;
                for (a, b) in edges(points) {
                    if (a.y > point.y) == (b.y > point.y) {
                        continue;
                    }
                    // The crossing's x, compared without dividing: multiply
                    // through by (b.y - a.y) and flip when it is negative.
                    let dy = b.y - a.y;
                    let lhs = (point.x - a.x) * dy;
                    let rhs = (point.y - a.y) * (b.x - a.x);
                    if (dy > 0 && lhs < rhs) || (dy < 0 && lhs > rhs) {
                        inside = !inside;
                    }
                }
                inside
            }
        }
    }

    /// A description of the shape, for a wire that publishes what was asked.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Rect(_) => "rect",
            Self::Circle { .. } => "circle",
            Self::Lasso(_) => "lasso",
        }
    }
}

/// `x² + y²`, saturating rather than wrapping at the `i64` ceiling.
fn squared(x: i64, y: i64) -> i64 {
    x.saturating_mul(x).saturating_add(y.saturating_mul(y))
}

/// The polygon's closing edges: each vertex to the next, and the last to the
/// first. The closure is derived here so no caller repeats a point.
fn edges(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| (*a, *b))
}

/// Whether segments `a-b` and `c-d` share a point.
///
/// Orientation tests over `i64` cross products, with the collinear case handled
/// by a bounding-box containment test.
///
/// ★**The collinear branch cannot change an answer [`Region::covers`] gives**,
/// measured at R1591 by a counterfactual that deleted it and was not caught: a
/// point where a lasso edge overlaps a rect's corner-cycle segment lies *on that
/// rect*, so the corner-inside or vertex-inside clause has already fired. It is
/// kept because this is a general predicate and its contract includes the case,
/// and it is tested **directly** rather than through `covers`, since a test that
/// cannot fail is not a test.
fn segments_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (o1, o2, o3, o4) = (
        orientation(a, b, c),
        orientation(a, b, d),
        orientation(c, d, a),
        orientation(c, d, b),
    );
    if o1 != o2 && o3 != o4 {
        return true;
    }
    (o1 == 0 && on_segment(a, b, c))
        || (o2 == 0 && on_segment(a, b, d))
        || (o3 == 0 && on_segment(c, d, a))
        || (o4 == 0 && on_segment(c, d, b))
}

/// `-1`, `0` or `1` for clockwise, collinear and counter-clockwise.
fn orientation(a: Point, b: Point, c: Point) -> i8 {
    let cross = (b.x - a.x).saturating_mul(c.y - a.y) - (b.y - a.y).saturating_mul(c.x - a.x);
    match cross.cmp(&0) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Whether collinear `p` lies within the bounding box of `a-b`.
fn on_segment(a: Point, b: Point, p: Point) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// A signed bounding box, stated by its **inclusive** extremes, clamped into
/// the unsigned pixel space and returned half-open.
///
/// The `+ 1` is what keeps the far edge: a circle of radius `r` about `cx`
/// covers the pixel at `cx + r`, and a rect of width `2r` would not hold it.
///
/// A shape entirely off the left or top of the window bounds to a zero-area
/// rect, which the walk then prunes — the same answer as "it covers nothing".
fn clamped_rect(min_x: i64, min_y: i64, max_x: i64, max_y: i64) -> Rect {
    let left = min_x.max(0);
    let top = min_y.max(0);
    let right = max_x.saturating_add(1).max(0);
    let bottom = max_y.saturating_add(1).max(0);
    Rect::new(
        u32::try_from(left).unwrap_or(u32::MAX),
        u32::try_from(top).unwrap_or(u32::MAX),
        u32::try_from((right - left).max(0)).unwrap_or(u32::MAX),
        u32::try_from((bottom - top).max(0)).unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::{Point, Region, RegionError, RegionFit};
    use crate::scene::Rect;

    /// Blender's `NODE_OT_select_circle` rule, over these types: a rect is
    /// selected when the disc intersects it (`BLI_rctf_isect_circle` against
    /// `node->runtime->draw_bounds`). Present so the agreement is asserted
    /// rather than assumed — the divergence is the FIT, not the geometry.
    fn blender_circle_hits(centre: Point, radius: i64, rect: Rect) -> bool {
        let (l, t) = (i64::from(rect.x), i64::from(rect.y));
        let (r, b) = (l + i64::from(rect.w), t + i64::from(rect.h));
        let nx = centre.x.clamp(l, r - 1);
        let ny = centre.y.clamp(t, b - 1);
        (centre.x - nx).pow(2) + (centre.y - ny).pow(2) <= radius * radius
    }

    #[test]
    fn a_circles_bounds_hold_its_far_pixel() {
        let disc = Region::circle(100, 100, 10);
        let bounds = disc.bounds();
        assert_eq!(bounds, Rect::new(90, 90, 21, 21), "2r + 1, not 2r");
        // The pixel at the far edge is inside the disc, so a bounding rect that
        // excluded it would prune a node the region really covers.
        assert!(disc.holds(Point::new(110, 100)));
        assert!(disc.covers(Rect::new(110, 100, 1, 1), RegionFit::Intersects));
    }

    #[test]
    fn a_circle_touches_by_its_nearest_pixel_and_contains_by_its_farthest() {
        let disc = Region::circle(50, 50, 20);
        // A rect whose nearest corner is inside and whose far corner is not.
        let straddling = Rect::new(60, 60, 40, 40);
        assert!(disc.covers(straddling, RegionFit::Intersects));
        assert!(
            !disc.covers(straddling, RegionFit::Contains),
            "PAST QT — the fit is an argument. Qt takes it from \
             QGraphicsView::rubberBandSelectionMode, a VIEW property, so two \
             selections in one view cannot mean different things"
        );
        let swallowed = Rect::new(45, 45, 6, 6);
        assert!(disc.covers(swallowed, RegionFit::Contains));
        assert!(
            disc.covers(swallowed, RegionFit::Intersects),
            "and touching too"
        );
        // Agreement with Blender on the geometry itself.
        for rect in [straddling, swallowed, Rect::new(0, 0, 5, 5)] {
            assert_eq!(
                disc.covers(rect, RegionFit::Intersects),
                blender_circle_hits(Point::new(50, 50), 20, rect),
                "{rect:?}"
            );
        }
    }

    #[test]
    fn a_lasso_is_closed_by_derivation() {
        // A triangle stated with three points, never four.
        let lasso = Region::lasso([(0, 0), (100, 0), (0, 100)]);
        assert!(lasso.holds(Point::new(10, 10)), "inside");
        assert!(!lasso.holds(Point::new(90, 90)), "past the hypotenuse");
        // Repeating the first point is harmless — it adds a zero-length edge.
        let repeated = Region::lasso([(0, 0), (100, 0), (0, 100), (0, 0)]);
        for probe in [(10, 10), (90, 90), (50, 49), (50, 51)] {
            assert_eq!(
                lasso.holds(Point::new(probe.0, probe.1)),
                repeated.holds(Point::new(probe.0, probe.1)),
                "{probe:?}"
            );
        }
    }

    #[test]
    fn a_concave_lasso_does_not_contain_what_its_bite_reaches_into() {
        // A "C": the mouth opens to the right, so a rect placed in the mouth has
        // all four corners OUTSIDE, and one placed straddling the mouth's lip
        // has corners inside while an edge cuts through it.
        let c = Region::lasso([
            (0, 0),
            (100, 0),
            (100, 20),
            (20, 20),
            (20, 80),
            (100, 80),
            (100, 100),
            (0, 100),
        ]);
        let in_the_mouth = Rect::new(50, 40, 20, 20);
        assert!(
            !c.covers(in_the_mouth, RegionFit::Intersects),
            "wholly in the bite"
        );
        let across_the_lip = Rect::new(10, 40, 30, 20);
        assert!(
            c.covers(across_the_lip, RegionFit::Intersects),
            "it crosses an edge"
        );
        assert!(!c.covers(across_the_lip, RegionFit::Contains));
        // ★The fixture the `!crosses` guard actually needs: all FOUR corners
        // land in the C's arms (its top band and its bottom band), and the rect
        // still spans the bite between them. Four-corners-inside answers
        // "contained" and is wrong; only the edge-crossing test sees it. Without
        // this rect, a counterfactual deleting that guard PASSES — CF-3 did.
        let spanning_the_bite = Rect::new(10, 10, 40, 80);
        for corner in [(10, 10), (49, 10), (49, 89), (10, 89)] {
            assert!(
                c.holds(Point::new(corner.0, corner.1)),
                "corner {corner:?} must be inside, or this fixture proves nothing"
            );
        }
        assert!(
            !c.covers(spanning_the_bite, RegionFit::Contains),
            "an edge cuts straight through it, which four-corners-inside cannot see"
        );
        assert!(c.covers(spanning_the_bite, RegionFit::Intersects));
        let wholly_inside = Rect::new(4, 30, 10, 10);
        assert!(c.covers(wholly_inside, RegionFit::Contains));
    }

    #[test]
    fn a_lasso_drawn_wholly_inside_one_node_still_touches_it() {
        let node = Rect::new(0, 0, 400, 300);
        let scribble = Region::lasso([(100, 100), (140, 110), (120, 150)]);
        assert!(
            scribble.covers(node, RegionFit::Intersects),
            "no edge crosses the node's border and no corner of it is inside the \
             lasso, so this is the third of the three ways to touch"
        );
        assert!(!scribble.covers(node, RegionFit::Contains));
    }

    #[test]
    fn a_lasso_edge_along_a_nodes_edge_counts_as_touching() {
        let node = Rect::new(100, 100, 50, 50);
        // The lasso's bottom edge runs exactly along the node's top edge.
        let grazing = Region::lasso([(0, 50), (200, 50), (200, 100), (0, 100)]);
        assert!(
            grazing.covers(node, RegionFit::Intersects),
            "a user who drew the line there meant to touch it"
        );
    }

    #[test]
    fn two_segments_that_only_overlap_collinearly_still_cross() {
        use super::segments_cross;
        // Reached through `covers` by nothing — see `segments_cross`'s own doc —
        // so it is asserted here, where it can fail.
        let (a, b) = (Point::new(0, 10), Point::new(100, 10));
        assert!(
            segments_cross(a, b, Point::new(40, 10), Point::new(60, 10)),
            "one lies inside the other, and they share every point of it"
        );
        assert!(
            segments_cross(a, b, Point::new(90, 10), Point::new(200, 10)),
            "and they overlap at one end"
        );
        assert!(
            !segments_cross(a, b, Point::new(120, 10), Point::new(200, 10)),
            "collinear and disjoint is not a crossing"
        );
        assert!(
            !segments_cross(a, b, Point::new(40, 11), Point::new(60, 11)),
            "parallel and apart is not either"
        );
        // And the ordinary proper crossing, so the strict branch is exercised
        // by this test too.
        assert!(segments_cross(a, b, Point::new(50, 0), Point::new(50, 20)));
    }

    #[test]
    fn a_self_crossing_lasso_uses_the_even_odd_rule() {
        // A bow tie: the two lobes are inside, the crossing point is not a lobe.
        let bow = Region::lasso([(0, 0), (100, 100), (100, 0), (0, 100)]);
        assert!(bow.holds(Point::new(10, 50)), "the left lobe");
        assert!(bow.holds(Point::new(90, 50)), "the right lobe");
        assert!(
            !bow.holds(Point::new(50, 10)),
            "above the crossing is outside"
        );
        assert!(!bow.holds(Point::new(50, 90)), "and below it");
    }

    #[test]
    fn a_shape_that_bounds_no_area_is_named_rather_than_answered_with_nothing() {
        assert_eq!(
            Region::lasso([(0, 0), (10, 10)]).validate(),
            Err(RegionError::LassoTooShort { vertices: 2 }),
            "PAST QT — QGraphicsScene::items(QPolygonF, ..) answers with a \
             QList, which has no channel for this: a degenerate lasso and an \
             empty surface are the same value there"
        );
        assert_eq!(Region::circle(0, 0, 0).validate(), Err(RegionError::Empty));
        assert_eq!(
            Region::rect(0, 0, 10, 0).validate(),
            Err(RegionError::Empty)
        );
        assert_eq!(Region::rect(0, 0, 1, 1).validate(), Ok(()));
        assert!(
            Region::lasso([(0, 0), (10, 10)])
                .validate()
                .unwrap_err()
                .to_string()
                .contains("three vertices")
        );
    }

    #[test]
    fn a_zero_area_node_is_covered_by_nothing() {
        // The rule `hit_test_region` has always followed, kept for every shape.
        for region in [
            Region::rect(0, 0, 100, 100),
            Region::circle(50, 50, 100),
            Region::lasso([(0, 0), (100, 0), (100, 100)]),
        ] {
            for fit in [RegionFit::Intersects, RegionFit::Contains] {
                assert!(!region.covers(Rect::new(10, 10, 0, 5), fit), "{region:?}");
                assert!(!region.covers(Rect::new(10, 10, 5, 0), fit), "{region:?}");
            }
        }
    }

    #[test]
    fn a_region_off_the_surface_bounds_to_nothing() {
        // Wholly to the left of the window: no pixel of it is on screen.
        let away = Region::circle(-500, 50, 10);
        assert_eq!(away.bounds().w, 0, "which the scene walk then prunes");
        // Straddling the origin keeps the half that is on screen.
        let straddling = Region::circle(0, 50, 10);
        let bounds = straddling.bounds();
        assert_eq!((bounds.x, bounds.w), (0, 11));
    }

    #[test]
    fn the_rectangle_arm_is_the_rule_the_scene_already_used() {
        let region = Region::rect(10, 10, 20, 20);
        // Half-open on both sides, exactly like `rects_intersect`.
        assert!(region.covers(Rect::new(29, 29, 1, 1), RegionFit::Intersects));
        assert!(!region.covers(Rect::new(30, 30, 1, 1), RegionFit::Intersects));
        assert!(region.covers(Rect::new(10, 10, 20, 20), RegionFit::Contains));
        assert!(!region.covers(Rect::new(10, 10, 21, 20), RegionFit::Contains));
        assert_eq!(region.kind(), "rect");
    }
}
