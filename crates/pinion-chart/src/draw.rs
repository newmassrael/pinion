//! Shared retained-`Scene` drawing primitives for the chart builders.
//!
//! These are the chart-agnostic leaves every chart type emits — a filled
//! box (a bar, a legend swatch, a background), a stroked polyline (an axis,
//! a gridline, a series line), a filled area, a text label — plus the two
//! layout policies (`absolute` / `fill_parent`) and the `u32`/`f32` pixel
//! conversions the geometry rides on. Lifted out of `line.rs` (R1374) so the
//! bar chart shares one definition of "a bar is a `box_node`, an axis is a
//! `stroke_path`" rather than re-deriving them — the "same scale / ticks /
//! palette core" the crate's follow-up chart types were promised.
//!
//! Every primitive takes the tag the caller assigns, so the chart's §2 #7
//! introspection ownership stays with the chart builder, not here.

use pinion_core::Scene;
use pinion_core::scene::{BoxNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    BoxStyle, Color, LayoutStyle, PathStyle, Size, SizeValue, Stroke, TextAlign, TextStyle,
};

use crate::style::Margin;

/// The plotting area inside `rect` after the [`Margin`] insets — `(left, right,
/// top, bottom)` in device pixels (each edge `+1`-clamped so a zero-inset side
/// still leaves a paintable span). Pure margin→pixel geometry, independent of
/// whether the x-axis is numeric (line) or categorical (bar), so both builders
/// share this one definition (R1374).
pub(crate) fn plot_rect(rect: Rect, margin: Margin) -> (f32, f32, f32, f32) {
    let x0 = rect.x + margin.left;
    let y0 = rect.y + margin.top;
    let x1 = (rect.x + rect.w).saturating_sub(margin.right).max(x0 + 1);
    let y1 = (rect.y + rect.h).saturating_sub(margin.bottom).max(y0 + 1);
    (to_f32(x0), to_f32(x1), to_f32(y0), to_f32(y1))
}

/// A stroked polyline path from plot-space points.
pub(crate) fn stroke_path(points: &[(f32, f32)], stroke: Stroke, tag: String) -> Scene {
    let bbox = bbox_of(points, stroke.width);
    let commands = polyline_commands(&rebased(points, bbox), false);
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::stroked(stroke))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

/// A filled area path: the polyline dropped to `baseline_y` and closed.
pub(crate) fn area_path(points: &[(f32, f32)], baseline_y: f32, fill: Color, tag: String) -> Scene {
    // The bbox must be resolved BEFORE the commands: the baseline union can
    // move the box's origin (a baseline above every point lifts `bbox.y`), and
    // R1358 rebases the commands onto that final origin.
    let mut bbox = bbox_of(points, 0);
    bbox = bbox.union(Rect::new(bbox.x, to_u32(baseline_y), 1, 1));
    let (ox, oy) = (to_f32(bbox.x), to_f32(bbox.y));
    let mut commands = polyline_commands(&rebased(points, bbox), false);
    if let (Some(&(last_x, _)), Some(&(first_x, _))) = (points.last(), points.first()) {
        commands.push(PathCommand::LineTo(PathPoint::new(
            last_x - ox,
            baseline_y - oy,
        )));
        commands.push(PathCommand::LineTo(PathPoint::new(
            first_x - ox,
            baseline_y - oy,
        )));
        commands.push(PathCommand::Close);
    }
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::filled(fill))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

/// R1358 — rebase plot-space points onto `bbox`'s origin so the emitted
/// [`PathCommand`]s are relative to the path node's own rect, which is what
/// positions it. Subtracting exactly the origin the node carries makes the
/// rebase pixel-exact: the paint adapter translates by the same value, and a
/// `bbox_of` origin clamped at 0 stays consistent with the commands built
/// from it.
fn rebased(points: &[(f32, f32)], bbox: Rect) -> Vec<(f32, f32)> {
    let (ox, oy) = (to_f32(bbox.x), to_f32(bbox.y));
    points.iter().map(|&(x, y)| (x - ox, y - oy)).collect()
}

fn polyline_commands(points: &[(f32, f32)], close: bool) -> Vec<PathCommand> {
    let mut commands = Vec::with_capacity(points.len() + usize::from(close));
    for (i, &(x, y)) in points.iter().enumerate() {
        let p = PathPoint::new(x, y);
        if i == 0 {
            commands.push(PathCommand::MoveTo(p));
        } else {
            commands.push(PathCommand::LineTo(p));
        }
    }
    if close && !points.is_empty() {
        commands.push(PathCommand::Close);
    }
    commands
}

/// A filled, tagged box placed at its own rect — a bar, a legend swatch, or
/// a chart background.
pub(crate) fn box_node(rect: Rect, fill: Color, tag: String) -> Scene {
    Scene::Box(
        BoxNode::new(rect, BoxStyle::filled(fill))
            .with_tag(tag)
            .with_layout(absolute(rect)),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "a label is intrinsically a box + text + alignment tuple; grouping them into a struct would not reduce the real parameter count"
)]
pub(crate) fn label_node(
    text: impl Into<String>,
    x: u32,
    y: u32,
    width: u32,
    align: TextAlign,
    color: Color,
    size: u32,
    tag: String,
) -> Scene {
    Scene::Text(
        TextNode::styled(
            text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(size)
                .with_fg(color)
                .with_align(align),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, y)
                .with_size(Size::px(width.max(1), size + 4)),
        ),
    )
}

/// A layout that pins a node to `rect` (parent-relative absolute position + size).
pub(crate) fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w.max(1), rect.h.max(1)))
}

/// R1360 — a chart root that FILLS its layout slot (both axes 100%), so
/// taffy sizes it from its parent and the `build_fill` children's
/// parent-relative `absolute_position`s resolve against the placed origin.
pub(crate) fn fill_parent() -> LayoutStyle {
    LayoutStyle::new().with_size(
        Size::auto()
            .with_width(SizeValue::Percent(100))
            .with_height(SizeValue::Percent(100)),
    )
}

/// The pixel bounding box of `points`, padded by `pad` on every side (+1 so a
/// zero-extent run still has a paintable rect).
pub(crate) fn bbox_of(points: &[(f32, f32)], pad: u32) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &(x, y) in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if !min_x.is_finite() {
        return Rect::default();
    }
    let pad_f = to_f32(pad);
    let x = to_u32(min_x - pad_f);
    let y = to_u32(min_y - pad_f);
    let w = to_u32(max_x - min_x) + pad * 2 + 1;
    let h = to_u32(max_y - min_y) + pad * 2 + 1;
    Rect::new(x, y, w, h)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "pixel coordinate u32 -> f32; display-bounded magnitudes"
)]
pub(crate) fn to_f32(v: u32) -> f32 {
    v as f32
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "pixel f32 -> u32; rounded and clamped non-negative"
)]
pub(crate) fn to_u32(v: f32) -> u32 {
    v.round().max(0.0) as u32
}
