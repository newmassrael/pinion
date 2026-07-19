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
use crate::ticks::format_axis_tick;

/// Fixed width (px) of the inspect [`callout`] tooltip box. Wide enough for a
/// line / scatter chart's `"{series}  {value}"` value rows; a bar chart's
/// shorter `"{value}"` row and the donut's `"{value} ({pct}%)"` row sit
/// comfortably inside the same box, so every chart with an inspect tooltip shares
/// the one width rather than each choosing its own (R1375).
pub(crate) const TOOLTIP_WIDTH: u32 = 132;

/// The plotting area inside `rect` after the [`Margin`] insets — `(left, right,
/// top, bottom)` in device pixels (each edge `+1`-clamped so a zero-inset side
/// still leaves a paintable span). Pure margin→pixel geometry, independent of
/// whether the x-axis is numeric (line / scatter, via [`crate::plot`]) or
/// categorical (bar), so all three cartesian builders share this one definition
/// (R1374).
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
/// the LEFT when it would overrun `plot_right`. The one definition every chart's
/// inspector emits (R1375) — the line and scatter charts (crosshair-anchored,
/// one row per series), the bar chart (bar-anchored, one value row), and the
/// donut (slice-anchored) — the mechanical box geometry they share, distinct
/// from the per-chart choice of what the rows SAY.
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

/// Append cubic-Bézier `CurveTo`s approximating the arc from `a0` to `a1`
/// (radians, either direction) at radius `r`, measured CLOCKWISE FROM THE TOP
/// (`0` points up, via `(sin, -cos)`), split into <=90-degree segments so each
/// cubic stays within a true-arc error bound (`k = 4/3 * tan(step/4) * r`). The
/// caller has already emitted the arc's start point. Lifted from `donut.rs`
/// (R1377) so the crate has ONE arc: the donut draws its sectors with it, and
/// [`circle_commands`] closes a full `0..2pi` sweep into the filled dot the line
/// marker and the scatter point both draw.
pub(crate) fn arc_beziers(cx: f32, cy: f32, r: f32, a0: f32, a1: f32, cmds: &mut Vec<PathCommand>) {
    use core::f32::consts::FRAC_PI_2;
    let sweep = a1 - a0;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a segment count is small and non-negative"
    )]
    let steps = ((sweep.abs() / FRAC_PI_2).ceil() as u32).max(1);
    let step = sweep / to_f32(steps);
    let point = |a: f32| PathPoint::new(cx + r * a.sin(), cy - r * a.cos());
    for i in 0..steps {
        let a_start = a0 + step * to_f32(i);
        let a_end = a_start + step;
        // The control-point offset along the unit tangent (cos, sin).
        let k = (4.0 / 3.0) * (step / 4.0).tan() * r;
        cmds.push(PathCommand::CurveTo {
            c1: PathPoint::new(
                cx + r * a_start.sin() + k * a_start.cos(),
                cy - r * a_start.cos() + k * a_start.sin(),
            ),
            c2: PathPoint::new(
                cx + r * a_end.sin() - k * a_end.cos(),
                cy - r * a_end.cos() - k * a_end.sin(),
            ),
            end: point(a_end),
        });
    }
}

/// The path commands for a full circle of radius `r` centred at `(cx, cy)`: a
/// `0..2pi` [`arc_beziers`] sweep closed back to the start — `MoveTo` + four
/// <=90-degree cubic `CurveTo`s + `Close`. The ONE circle the crate draws (R1377):
/// the line chart's inspect marker and the scatter chart's point marks both use
/// it, so a change to the circle approximation moves in one place. (Before
/// R1377 this was `line.rs`'s own four-Bézier `circle_commands`; expressing it
/// through the donut's arc makes the two circle sites one.)
pub(crate) fn circle_commands(cx: f32, cy: f32, r: f32) -> Vec<PathCommand> {
    use core::f32::consts::TAU;
    // Start at the top (angle 0 in the arc's clockwise-from-top frame).
    let mut cmds = vec![PathCommand::MoveTo(PathPoint::new(cx, cy - r))];
    arc_beziers(cx, cy, r, 0.0, TAU, &mut cmds);
    cmds.push(PathCommand::Close);
    cmds
}

/// A filled circle marker of radius `r` at `(cx, cy)` — a line chart's inspect
/// dot or a scatter chart's data point. Lifted from `line.rs` (R1377); the
/// circle geometry is [`circle_commands`], rect-relative to the node's own bbox
/// so the path centres on its placed rect (R1358).
pub(crate) fn marker_node(cx: f32, cy: f32, r: u32, fill: Color, tag: String) -> Scene {
    let bbox = Rect::new(
        to_u32(cx - to_f32(r)),
        to_u32(cy - to_f32(r)),
        r * 2 + 1,
        r * 2 + 1,
    );
    let commands = circle_commands(cx - to_f32(bbox.x), cy - to_f32(bbox.y), to_f32(r));
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::filled(fill))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

/// The plot area edges in device pixels — `(left, right, top, bottom)`, the
/// same shape [`plot_rect`] returns. The cartesian axis-furniture helpers take
/// it as one argument so a chart hands its resolved plot frame across in a
/// single value.
pub(crate) type PlotFrame = (f32, f32, f32, f32);

/// The gridlines of a cartesian plot: a horizontal line at each `y_positions`
/// pixel (tagged `.grid.y.{k}`) and a vertical line at each `x_positions` pixel
/// (`.grid.x.{k}`). The caller pre-maps ticks to pixels, so this stays a pure
/// "draw lines at these positions" primitive with no scale knowledge — a
/// categorical bar chart passes an empty `x_positions` (it has no numeric
/// x-gridlines), a line / scatter chart passes both. One definition the three
/// cartesian charts share (R1377).
pub(crate) fn gridlines(
    frame: PlotFrame,
    x_positions: &[f32],
    y_positions: &[f32],
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let (left, right, top, bottom) = frame;
    let stroke = Stroke::new(style.grid, 1);
    let mut out = Vec::new();
    for (k, &y) in y_positions.iter().enumerate() {
        out.push(stroke_path(
            &[(left, y), (right, y)],
            stroke,
            format!("{prefix}.grid.y.{k}"),
        ));
    }
    for (k, &x) in x_positions.iter().enumerate() {
        out.push(stroke_path(
            &[(x, top), (x, bottom)],
            stroke,
            format!("{prefix}.grid.x.{k}"),
        ));
    }
    out
}

/// The left (y) and bottom (x) axis lines of a cartesian plot — the L-shaped
/// frame tagged `.axis.y` / `.axis.x`. Shared by all three cartesian charts
/// (R1377).
pub(crate) fn axes(frame: PlotFrame, style: &ChartStyle, prefix: &str) -> Vec<Scene> {
    let (left, right, top, bottom) = frame;
    let stroke = Stroke::new(style.axis, 1);
    vec![
        stroke_path(
            &[(left, top), (left, bottom)],
            stroke,
            format!("{prefix}.axis.y"),
        ),
        stroke_path(
            &[(left, bottom), (right, bottom)],
            stroke,
            format!("{prefix}.axis.x"),
        ),
    ]
}

/// Right-aligned y-axis tick labels in the left gutter — one `.label.y.{k}` per
/// tick, formatted at the axis's `step` precision, its box top offset so the
/// text centres on the tick's `y_positions` pixel. The parallel `ticks` /
/// `y_positions` slices are the value (for the text) and the pixel (for the
/// placement) of the same tick `k`. Shared by all three cartesian charts
/// (R1377); the x-axis labels stay per-chart (numeric ticks for line / scatter,
/// category names centred in a slot for bar).
pub(crate) fn y_tick_labels(
    rect_x: u32,
    ticks: &[f64],
    y_positions: &[f32],
    step: f64,
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let size = style.label_size_px.max(1);
    let gutter = style.margin.left.saturating_sub(6).max(1);
    let mut out = Vec::new();
    for (k, (&t, &py)) in ticks.iter().zip(y_positions).enumerate() {
        let ly = to_u32(py).saturating_sub(size / 2 + 1);
        out.push(label_node(
            format_axis_tick(t, step),
            rect_x + 2,
            ly,
            gutter,
            TextAlign::End,
            style.label,
            size,
            format!("{prefix}.label.y.{k}"),
        ));
    }
    out
}

/// Fixed width (px) of one legend entry slot — a swatch + its label. Shared so
/// every chart WITH a legend (line, donut, scatter — the bar chart has none)
/// lays it out on the same grid (R1377).
pub(crate) const LEGEND_SLOT: u32 = 104;

/// One legend row: a colour swatch + a text label per `(color, text)` entry,
/// laid out left-to-right on the [`LEGEND_SLOT`] grid from `start_x` at `row_y`,
/// tagged `.legend.{i}.swatch` / `.legend.{i}.label`. The line and donut charts
/// emitted this inner loop byte-identically; R1377's scatter chart was the third
/// legend consumer that lifted it here. The per-chart choice is only WHERE the
/// row sits (`start_x` / `row_y` — a top band for line / scatter, a centred
/// bottom band for the donut) and what fills the entries (series names vs slice
/// labels), which the caller resolves and passes in.
pub(crate) fn legend_row(
    entries: &[(Color, String)],
    start_x: u32,
    row_y: u32,
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let size = style.label_size_px.max(1);
    let swatch = size;
    let mut out = Vec::new();
    for (i, (color, text)) in entries.iter().enumerate() {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the legend index is small; the slot offset stays within u32"
        )]
        let entry_x = start_x + (i as u32) * LEGEND_SLOT;
        out.push(box_node(
            Rect::new(entry_x, row_y, swatch, swatch),
            *color,
            format!("{prefix}.legend.{i}.swatch"),
        ));
        out.push(label_node(
            text.clone(),
            entry_x + swatch + 4,
            row_y.saturating_sub(1),
            LEGEND_SLOT.saturating_sub(swatch + 4),
            TextAlign::Start,
            style.label,
            size,
            format!("{prefix}.legend.{i}.label"),
        ));
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
