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

use crate::style::{ChartStyle, Margin};

/// Fixed width (px) of the inspect [`callout`] tooltip box. Wide enough for a
/// line chart's `"{series}  {value}"` value rows; a bar chart's shorter
/// `"{value}"` row sits comfortably inside the same box, so both chart types
/// share the one width rather than each choosing its own (R1375).
pub(crate) const TOOLTIP_WIDTH: u32 = 132;

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

/// A filled, rounded, tagged box — the inspect tooltip's backing plate. A
/// [`box_node`] with a corner radius; kept distinct so the sharp-cornered bars
/// and the rounded callout do not have to thread a radius through every call.
pub(crate) fn rounded_box_node(rect: Rect, fill: Color, radius: u32, tag: String) -> Scene {
    Scene::Box(
        BoxNode::new(rect, BoxStyle::filled(fill).with_corner_radius(radius))
            .with_tag(tag)
            .with_layout(absolute(rect)),
    )
}

/// One value line of a [`callout`]: its text, its own colour (a line chart
/// colours each row by its series; a bar chart uses the tooltip foreground),
/// and the full introspection tag the caller assigns it.
pub(crate) struct CalloutRow {
    /// The row's text (e.g. `"ingress  2.4k"` or `"3 frames"`).
    pub text: String,
    /// The row's text colour.
    pub color: Color,
    /// The row's introspection tag (e.g. `"chart.inspect.value.0"`).
    pub tag: String,
}

/// The inspect tooltip callout: a rounded backing box, a header line, and one
/// colour-per-row value line, placed to the RIGHT of `anchor_x` and flipped to
/// the LEFT when it would overrun `plot_right`. The one definition both the
/// line chart (crosshair-anchored, one row per series) and the bar chart
/// (bar-anchored, one value row) emit (R1375) — the mechanical box geometry
/// they share, distinct from the per-chart choice of what the rows SAY.
#[allow(
    clippy::too_many_arguments,
    reason = "a callout is intrinsically an anchor + two plot bounds + a \
              header (text, tag) + rows + style + box tag; grouping them into a \
              struct would not reduce the real parameter count"
)]
pub(crate) fn callout(
    anchor_x: f32,
    plot_right: f32,
    plot_top: f32,
    header: &str,
    header_tag: String,
    rows: &[CalloutRow],
    style: &ChartStyle,
    box_tag: String,
) -> Vec<Scene> {
    let size = style.label_size_px.max(1);
    let line_h = size + 6;
    let pad = 8;
    let width = TOOLTIP_WIDTH;
    let row_count = u32::try_from(rows.len()).unwrap_or(0) + 1; // header + values
    let height = row_count * line_h + pad;
    // Place right of the anchor; flip left if it would overflow the plot.
    let mut box_x = to_u32(anchor_x) + 12;
    if box_x + width > to_u32(plot_right) {
        box_x = to_u32(anchor_x).saturating_sub(width + 12);
    }
    let box_y = to_u32(plot_top) + 8;
    let text_x = box_x + pad;

    let mut out = vec![rounded_box_node(
        Rect::new(box_x, box_y, width, height),
        style.tooltip_bg,
        6,
        box_tag,
    )];
    let mut ty = box_y + pad / 2;
    out.push(label_node(
        header,
        text_x,
        ty,
        width - pad * 2,
        TextAlign::Start,
        style.tooltip_fg,
        size,
        header_tag,
    ));
    ty += line_h;
    for row in rows {
        out.push(label_node(
            row.text.clone(),
            text_x,
            ty,
            width - pad * 2,
            TextAlign::Start,
            row.color,
            size,
            row.tag.clone(),
        ));
        ty += line_h;
    }
    out
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
