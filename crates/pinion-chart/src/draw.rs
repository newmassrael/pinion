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

use crate::interpolate::Interpolation;
use pinion_core::Scene;
use pinion_core::scene::{BoxNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    Border, BorderPlacement, BoxStyle, Color, Gradient, LayoutStyle, PathStyle, Size, SizeValue,
    Stroke, TextAlign, TextOverflow, TextStyle,
};

use crate::color_scale::ValueEncoding;
use crate::scale::{CategoryScale, ValueScale, index_value};
use crate::style::{ChartStyle, Margin};
use crate::ticks::{TickFormat, format_at_decimals, value_decimals};

/// Fixed width (px) of the inspect [`callout`] tooltip box. Wide enough for a
/// line / scatter chart's `"{series}  {value}"` value rows; a bar chart's
/// shorter `"{value}"` row and the donut's `"{value} ({pct}%)"` row sit
/// comfortably inside the same box, so every chart with an inspect tooltip shares
/// the one width rather than each choosing its own (R1375).
pub(crate) const TOOLTIP_WIDTH: u32 = 132;

/// The alpha a mark OUTSIDE the active cross-filter selection is dimmed to — low
/// enough to read as "muted / filtered out" beside the full-strength selected
/// marks, high enough to stay visible (and re-selectable) as context. Shared by
/// the categorical [`BarChart::select`](crate::BarChart::select) (R1384) and the
/// numeric [`ScatterChart::select_x_range`](crate::ScatterChart::select_x_range)
/// (R1391) so the two cross-filter forms dim to the identical strength — lifted
/// here from `bar.rs` at the 2nd consumer, the [`TOOLTIP_WIDTH`] precedent.
pub(crate) const MUTED_ALPHA: u8 = 0x4D;

/// The plotting area inside `rect` after the [`Margin`] insets — `(left, right,
/// top, bottom)` in device pixels (each edge `+1`-clamped so a zero-inset side
/// still leaves a paintable span). Pure margin→pixel geometry, independent of
/// whether the x-axis is numeric (line / scatter, via [`crate::plot`]) or
/// categorical (bar), so all three cartesian builders share this one definition
/// (R1374).
pub(crate) fn plot_rect(rect: Rect, margin: Margin) -> (f32, f32, f32, f32) {
    // R1534 — derived from the public [`crate::plot_area`] rather than
    // recomputing the insets, so a consumer aligning something to the axis
    // (a brush strip, a wheel-zoom target) and the axis itself cannot land a
    // pixel apart.
    let area = crate::plot_area(rect, margin);
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.x + area.w, area.y + area.h);
    (to_f32(x0), to_f32(x1), to_f32(y0), to_f32(y1))
}

/// R1625 — a stroked path that joins its points under `kind`.
///
/// [`Interpolation::Linear`] gives the identical node
/// [`stroke_path`] does, so a chart that never asks for a curve pays nothing
/// and its scene is byte-unchanged. Anything else emits real cubic commands —
/// R1623's vocabulary — rather than a densely sampled polyline, so the scene
/// still says "a curve through these samples" to anyone reading it.
///
/// The bounding box comes from the CURVE rather than from the points: a
/// smooth interpolation may leave the box its samples span, and a node rect
/// that did not know that would clip the very excursion
/// [`crate::interpolate::overshoot`] exists to report.
pub(crate) fn curve_stroke_path(
    points: &[(f32, f32)],
    kind: Interpolation,
    stroke: Stroke,
    tag: String,
) -> Scene {
    if kind == Interpolation::Linear {
        return stroke_path(points, stroke, tag);
    }
    let absolute_commands = crate::interpolate::commands(points, kind);
    let Some(b) = pinion_core::path_data::bounds(&absolute_commands) else {
        return stroke_path(points, stroke, tag);
    };
    let bbox = bbox_of(&[(b.min_x, b.min_y), (b.max_x, b.max_y)], stroke.width);
    let (ox, oy) = (to_f32(bbox.x), to_f32(bbox.y));
    let rebased_points: Vec<(f32, f32)> = points.iter().map(|&(x, y)| (x - ox, y - oy)).collect();
    let commands = crate::interpolate::commands(&rebased_points, kind);
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::stroked(stroke))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
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

/// A **closed** path from plot-space points, taking both `PathStyle` arms —
/// the primitive for a mark whose outline is not a rectangle (R1553's box
/// plot box, which grows a waist when notched).
///
/// Distinct from [`stroke_path`] (open polyline, stroke only) and
/// [`area_path`] (closed against a baseline, fill only): this one is the
/// general case where the caller states the whole outline and both arms.
/// The bbox is padded by the stroke width so a stroked edge is not clipped
/// by its own node rect.
pub(crate) fn polygon_node(points: &[(f32, f32)], style: PathStyle, tag: String) -> Scene {
    let bbox = bbox_of(points, style.stroke.as_ref().map_or(0, |s| s.width));
    let commands = polyline_commands(&rebased(points, bbox), true);
    Scene::Path(
        PathNode::new(bbox, commands, style)
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

/// A filled area path: the polyline dropped to `baseline_y` and closed.
pub(crate) fn area_path(
    points: &[(f32, f32)],
    baseline_y: f32,
    kind: Interpolation,
    fill: Color,
    tag: String,
) -> Scene {
    let (bbox, commands) = area_geometry(points, baseline_y, kind);
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::filled(fill))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

/// R1622 §5.28 — the area **between two curves**: a stacked band, filled.
///
/// [`area_path`] closes its shape onto a scalar baseline, which is the right
/// shape for one area over zero and cannot express a band sitting on the
/// cumulative total below it — the reason an application wanting a stack had
/// to pre-sum its own data. `upper` is walked forwards and `lower` backwards,
/// so the ring closes without a self-intersection wherever the two curves
/// cross (a series dipping negative), which a naive forward-forward walk would
/// draw as a bow tie.
///
/// Both slices are plot-space points and must be the same length; a mismatch
/// draws nothing rather than a partial band, because half a band reads as data.
pub(crate) fn area_between(
    upper: &[(f32, f32)],
    lower: &[(f32, f32)],
    kind: Interpolation,
    fill: Color,
    tag: String,
) -> Option<Scene> {
    if upper.len() != lower.len() || upper.len() < 2 {
        return None;
    }
    // R1628 — both edges take the interpolation. The lower one is walked
    // backwards through `append_reversed`, which retraces the FORWARD curve
    // exactly rather than re-interpolating a descending x (which is not a
    // graph, and would silently flatten the band's underside).
    let mut ring = crate::interpolate::commands(upper, kind);
    ring.push(PathCommand::LineTo(PathPoint::new(
        lower[lower.len() - 1].0,
        lower[lower.len() - 1].1,
    )));
    crate::interpolate::append_reversed(lower, kind, &mut ring);
    ring.push(PathCommand::Close);
    let bbox = curve_bbox(&ring, None);
    let commands = translated(&ring, bbox);
    Some(Scene::Path(
        PathNode::new(bbox, commands, PathStyle::filled(fill))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    ))
}

/// R1440 — the same area path filled with a gradient ALONG X, given `ramp` as
/// `(x_px, colour)` samples in plot space.
///
/// The area chart's answer to "colour this mark by a measure". Unlike a point or
/// a tile, an area spans a range of x, so a single fill colour would have to pick
/// one value out of many; a horizontal gradient encodes the measure
/// *continuously* along the mark, which is what the data actually is.
///
/// The stops are placed by converting each sample's x into a fraction of THIS
/// path's bounding box, which is why the conversion lives here rather than at the
/// call site: a [`Gradient`]'s UV is box-relative (§5.50), and a caller computing
/// fractions against the plot rect instead would shift every stop whenever a
/// series does not span the full plot width. The bbox is derived by the same
/// [`area_geometry`] the commands come from, so the ramp and the shape cannot
/// disagree.
///
/// Superior to a pixel-painted heat-band in the way that matters for §2 #7: the
/// stops ride in the scene as data, so an introspecting client reads the measure
/// out of `scene/snapshot` at every sample x.
pub(crate) fn area_path_along_x(
    points: &[(f32, f32)],
    baseline_y: f32,
    kind: Interpolation,
    ramp: &[(f32, Color)],
    tag: String,
) -> Scene {
    let (bbox, commands) = area_geometry(points, baseline_y, kind);
    let span = to_f32(bbox.w).max(1.0);
    let origin = to_f32(bbox.x);
    let mut gradient = Gradient::horizontal();
    for &(x_px, color) in ramp {
        gradient = gradient.with_stop(((x_px - origin) / span).clamp(0.0, 1.0), color);
    }
    // The flat fallback is the first sample's colour, so a backend that ignores
    // gradients (the TUI adapter) still shows a colour from the encoding.
    let fill = ramp.first().map_or(Color::TRANSPARENT, |&(_, c)| c);
    Scene::Path(
        PathNode::new(
            bbox,
            commands,
            PathStyle::filled(fill).with_gradient(gradient),
        )
        .with_tag(tag)
        .with_layout(absolute(bbox)),
    )
}

/// The bounding box and rebased commands of an area path — the ONE definition
/// both [`area_path`] and [`area_path_along_x`] read, so a gradient placed
/// against the bbox lands on the shape that bbox describes.
fn area_geometry(
    points: &[(f32, f32)],
    baseline_y: f32,
    kind: Interpolation,
) -> (Rect, Vec<PathCommand>) {
    // R1628 — the top edge takes the interpolation, the two closing edges are
    // straight because a baseline IS straight. Before this the stroke curved
    // and its own fill did not, so one node's two halves disagreed about the
    // shape they were drawing.
    let mut absolute = crate::interpolate::commands(points, kind);
    if let (Some(&(last_x, _)), Some(&(first_x, _))) = (points.last(), points.first()) {
        absolute.push(PathCommand::LineTo(PathPoint::new(last_x, baseline_y)));
        absolute.push(PathCommand::LineTo(PathPoint::new(first_x, baseline_y)));
        absolute.push(PathCommand::Close);
    }
    // The bbox is resolved from the CURVE, not from the samples: a smooth
    // interpolation may leave the box its points span, and a node rect that
    // did not know that would clip the excursion `overshoot` reports.
    let bbox = curve_bbox(&absolute, Some(baseline_y));
    let commands = translated(&absolute, bbox);
    (bbox, commands)
}

/// R1628 — the pixel box a command stream occupies, optionally unioned with a
/// baseline row (which can lift the box's origin above every sample).
fn curve_bbox(absolute: &[PathCommand], baseline_y: Option<f32>) -> Rect {
    let Some(b) = pinion_core::path_data::bounds(absolute) else {
        return Rect::default();
    };
    let mut corners = vec![(b.min_x, b.min_y), (b.max_x, b.max_y)];
    if let Some(y) = baseline_y {
        corners.push((b.min_x, y));
    }
    bbox_of(&corners, 0)
}

/// R1358 — rebase absolute commands onto `bbox`'s origin, so a path is placed
/// by its rect. Reuses R1623's transform rather than re-emitting the stream.
fn translated(absolute: &[PathCommand], bbox: Rect) -> Vec<PathCommand> {
    pinion_core::path_data::scale_translate(absolute, 1.0, -to_f32(bbox.x), -to_f32(bbox.y))
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
                .with_align(align)
                // (R1396) A label box has a fixed width; a longer string used to
                // paint past it (`TextOverflow::Visible` default), so a wide
                // tick / legend label overran the chart into a neighbouring dock
                // pane. Clip scissors the glyphs to the box, so the box's own
                // clamp is the only containment the caller must get right.
                .with_overflow(TextOverflow::Clip),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, y)
                .with_size(Size::px(width.max(1), label_box_h(size))),
        ),
    )
}

/// The x-axis label for category slot `index`, centred under the band the
/// [`CategoryScale`] draws it in and tagged `{prefix}.xlabel.{index}` (R1567).
///
/// The per-CATEGORY x label, as distinct from [`label_node`]'s general form
/// and from the per-TICK `{prefix}.label.x.{k}` a numeric axis emits. It
/// arrived here as its third byte-identical copy — the bar chart (R1374), the
/// box plot (R1553) and the candlestick chart all place a label under a slot
/// the same way, and each held its own `band(i).unwrap_or(..)` fallback for
/// the case the axis carries no such slot. Three mechanical copies is the
/// lift threshold this project works to; the fallback is the reason it
/// matters, since a chart that got it wrong would paint every out-of-window
/// label stacked at the plot's left edge.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameters are the label's frame plus the axis it is derived from; a struct would rename them, not remove them"
)]
pub(crate) fn category_label_node(
    x: &CategoryScale,
    index: usize,
    left: f32,
    bottom: f32,
    format: &TickFormat,
    color: Color,
    size: u32,
    prefix: &str,
) -> Scene {
    let (slot_lo, slot_hi) = x.band(index).unwrap_or((left, left));
    label_node(
        format.label(index_value(index)),
        to_u32(slot_lo),
        to_u32(bottom) + X_LABEL_GAP,
        to_u32(slot_hi - slot_lo).max(1),
        TextAlign::Center,
        color,
        size,
        format!("{prefix}.xlabel.{index}"),
    )
}

/// The gap (px) between the plot's baseline and the top of an x-axis label
/// box — one definition, so the three category charts cannot drift apart by a
/// pixel.
const X_LABEL_GAP: u32 = 4;

/// Fixed width (px) of a numeric x-tick label's box. A tick label is centred
/// on its tick and clamped inside the chart, so the slot is what decides how
/// much of a long label survives [`label_node`]'s clip.
const X_TICK_SLOT: u32 = 60;

/// The x-axis tick labels of a **numeric** axis, tagged
/// `{prefix}.label.x.{k}` — one per tick the scale can place, centred on it
/// and clamped inside `rect` (R1567).
///
/// The per-TICK x label, as distinct from [`category_label_node`]'s per-SLOT
/// one. R1377 lifted the y-axis twin and deliberately left this here, with
/// the reason written down in `line.rs`: *"deferred from the R1377 lift until
/// a third numeric-x consumer arrives"*. The candlestick chart's elapsed
/// reading is that third consumer, so the loop lands where its own comment
/// said it would.
pub(crate) fn x_tick_labels(
    x: &ValueScale,
    ticks: &[f64],
    bottom: f32,
    rect: Rect,
    format: &TickFormat,
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let size = style.label_size_px.max(1);
    let mut out = Vec::new();
    for (k, &t) in ticks.iter().enumerate() {
        // (R1396) A tick the scale cannot place has no label — a log axis's
        // domain can exclude one — and the box is clamped inside `rect` so the
        // last label does not overhang into a docked neighbour.
        let Some(px) = x.map(t) else { continue };
        out.push(label_node(
            format.label(t),
            centered_label_x(px, X_TICK_SLOT, rect),
            to_u32(bottom) + X_LABEL_GAP,
            X_TICK_SLOT,
            TextAlign::Center,
            style.label,
            size,
            format!("{prefix}.label.x.{k}"),
        ));
    }
    out
}

/// Height (px) of the box a [`label_node`] of text `size` occupies.
///
/// A caller that seats a label by its CENTRE (the vertical colour bar's ticks)
/// has to know the box height, not just the glyph size, or the box overhangs by
/// the padding — which is invisible at the top of a chart and paints outside it
/// at the bottom. Exposed as one definition rather than re-adding the padding at
/// the call site (R1439).
pub(crate) const fn label_box_h(size: u32) -> u32 {
    size + 4
}

/// ★★★★★ R1956 — **the top of a label box that straddles `line`**, which is
/// what `line - size / 2 - 1` was reaching for and missing.
///
/// The box is [`label_box_h`] tall, not `size` tall, so the hand-spelled offset
/// is short by however much this crate's label box exceeds its face — one pixel
/// at every odd size. Measured on the assembled analysis tool: five y-tick
/// labels of a latency chart each sat a pixel below the grid line they name,
/// and the axis and a bar were reported beside them because all three are drawn
/// from the same tick.
///
/// Two sites spelled the offset — this crate's y-tick labels and its timeline
/// lane names — which is why this is a derivation rather than a fix at each.
/// The centring itself is `containment::band_on`, so this crate and the gate
/// that reads the paint are asking **one rule**; only the height is ours.
pub(crate) fn label_y_on(line: u32, size: u32) -> u32 {
    pinion_core::containment::band_on(line, 0, 0, label_box_h(size)).y
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

/// A hollow, tagged ring framing `rect` — the inspect highlight every
/// rect-based chart draws over its focused mark. A transparent fill with a
/// 2px, radius-2 border in `color`, placed `Outside` the rect so the ring
/// frames the mark without tinting or covering it. Lifted from `bar.rs`
/// (R1382): the bar chart rings its focused bar with it and the treemap rings
/// its focused tile, so a rect-highlight ring is now one leaf here beside the
/// crate's other shared draw leaves ([`box_node`] / [`rounded_box_node`] /
/// [`marker_node`]) rather than a bar-private helper the treemap re-derives.
pub(crate) fn outline_box(rect: Rect, color: Color, tag: String) -> Scene {
    Scene::Box(
        BoxNode::new(
            rect,
            BoxStyle::filled(Color::TRANSPARENT)
                .with_border(Border::new(color, 2).with_placement(BorderPlacement::Outside))
                .with_corner_radius(2),
        )
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

/// Multiply two `0..=255` alphas (`a * b / 255`) — dims an already
/// translucent colour by a second factor, so a muted area reads lighter
/// than a muted stroke rather than the same weight.
///
/// Lived in `line.rs` until R1528 gave [`MUTED_ALPHA`] a second dimming
/// caller (the minor gridlines). It sits beside the constant it is almost
/// always applied to; a `crate::line::mul_alpha` import in this module
/// would read as the shared drawing core borrowing the line chart's helper.
#[allow(
    clippy::cast_possible_truncation,
    reason = "(a * b) / 255 <= 255, so it fits u8"
)]
pub(crate) fn mul_alpha(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b)) / 255) as u8
}

/// The **minor** gridlines of a logarithmic axis (R1528), tagged
/// `.grid.minor.y.{k}` / `.grid.minor.x.{k}` — the per-decade subdivisions,
/// drawn at half the major gridlines' alpha.
///
/// Fainter is load-bearing, not decoration. A log axis's decade lines are
/// evenly spaced, so at equal weight the picture is indistinguishable from a
/// linear axis whose labels happen to read `1 / 10 / 100`; the crowding
/// between decades is what shows a reader the spacing is a ratio. Half the
/// major alpha rather than a new [`ChartStyle`] field, because that keeps the
/// minors tied to whatever grid colour a theme resolved — a separate knob
/// could be set to a colour that contradicts it.
///
/// Emitted separately from [`gridlines`] rather than appended to it so the
/// two sets stay independently addressable in the scene: `.grid.y.{k}` keeps
/// counting only the labelled lines, which is what every existing gridline
/// assertion means by it.
pub(crate) fn minor_gridlines(
    frame: PlotFrame,
    x_positions: &[f32],
    y_positions: &[f32],
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let (left, right, top, bottom) = frame;
    let stroke = Stroke::new(
        style.grid.with_alpha(mul_alpha(style.grid.a, MUTED_ALPHA)),
        1,
    );
    let mut out = Vec::new();
    for (k, &y) in y_positions.iter().enumerate() {
        out.push(stroke_path(
            &[(left, y), (right, y)],
            stroke,
            format!("{prefix}.grid.minor.y.{k}"),
        ));
    }
    for (k, &x) in x_positions.iter().enumerate() {
        out.push(stroke_path(
            &[(x, top), (x, bottom)],
            stroke,
            format!("{prefix}.grid.minor.x.{k}"),
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
/// tick, formatted the way that axis formats ([`TickFormat`]), its box top offset so the
/// text centres on the tick's `y_positions` pixel. The parallel `ticks` /
/// `y_positions` slices are the value (for the text) and the pixel (for the
/// placement) of the same tick `k`. Shared by all three cartesian charts
/// (R1377); the x-axis labels stay per-chart (numeric ticks for line / scatter,
/// category names centred in a slot for bar).
pub(crate) fn y_tick_labels(
    rect_x: u32,
    ticks: &[f64],
    y_positions: &[f32],
    format: &TickFormat,
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let size = style.label_size_px.max(1);
    let gutter = style.margin.left.saturating_sub(6).max(1);
    let mut out = Vec::new();
    for (k, (&t, &py)) in ticks.iter().zip(y_positions).enumerate() {
        let ly = label_y_on(to_u32(py), size);
        out.push(label_node(
            format.label(t),
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

/// Left edge of a `slot`-wide label box centred on `center_px`, clamped so the
/// whole box stays inside `bounds` (R1396).
///
/// Every axis label in this crate is a fixed-width box centred on a tick, and
/// each builder used to write `to_u32(px).saturating_sub(slot / 2)` — which
/// bounds the LEFT edge (a tick near x=0 stops at 0) and leaves the RIGHT edge
/// free, so the last tick's box always overhung the chart by `slot / 2 -
/// margin.right`. Inside a window that overhang lands in the window's own
/// padding and is invisible; inside a **dock pane** it paints over the
/// neighbouring pane, which is what R1396's docked consumer surfaced. Clamping
/// here rather than at each call site makes "a chart paints only inside its own
/// frame" one rule with one definition: the third consumer (line + scatter
/// x-ticks, the timeline's ruler) is what lifted it.
///
/// A slot wider than `bounds` clamps to `bounds.x` — the box is then wider than
/// the chart, which only a degenerate (a few px) chart can produce, and the
/// label's own `TextOverflow::Clip` still scissors the glyphs to it.
pub(crate) fn centered_label_x(center_px: f32, slot: u32, bounds: Rect) -> u32 {
    let max_x = (bounds.x + bounds.w).saturating_sub(slot).max(bounds.x);
    to_u32(center_px)
        .saturating_sub(slot / 2)
        .clamp(bounds.x, max_x)
}

/// Preferred width (px) of one legend entry slot — a swatch + its label. Shared
/// so every chart WITH a legend (line, donut, scatter — the bar chart has none)
/// lays it out on the same grid (R1377). It is the *preferred* width since
/// R1396: [`legend_fit`] shrinks it toward [`LEGEND_MIN_SLOT`] when the chart is
/// too narrow to seat every entry at this width.
pub(crate) const LEGEND_SLOT: u32 = 104;

/// The narrowest legend slot (px) that still reads as "swatch + a word": below
/// this [`legend_fit`] stops shrinking and starts dropping entries, because a
/// 20px slot is a colour chip beside a clipped glyph — noise that costs the same
/// width as a truthful `+N`.
pub(crate) const LEGEND_MIN_SLOT: u32 = 44;

/// Width (px) reserved for the `+N` marker that stands for the entries a too-narrow
/// legend dropped. Fits `+99` at the label sizes this crate uses.
pub(crate) const LEGEND_OVERFLOW_SLOT: u32 = 32;

/// How a legend row seats `entries` entries in `avail` px (R1396).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LegendFit {
    /// Per-entry slot width — [`LEGEND_SLOT`] when everything fits at the
    /// preferred width, shrunk toward [`LEGEND_MIN_SLOT`] when it does not.
    pub slot: u32,
    /// How many entries are drawn, in order.
    pub shown: usize,
    /// How many entries were dropped (`0` = the row is complete). A non-zero
    /// count is drawn as a `+N` marker in [`LEGEND_OVERFLOW_SLOT`] px.
    pub hidden: usize,
}

/// Seat `entries` legend entries in `avail` px: shrink the slot to fit, and when
/// even [`LEGEND_MIN_SLOT`] does not fit them all, drop the tail and reserve
/// [`LEGEND_OVERFLOW_SLOT`] px for the `+N` marker (R1396).
///
/// The alternative — wrapping the legend onto further rows — is the richer
/// answer and is NOT built: the top band's height is `margin.top`, so a second
/// row would have to feed back into the plot rect, making the plot's geometry
/// depend on legend text. That is a domain change, not a clamp; this keeps the
/// row one row and never paints outside the chart.
pub(crate) fn legend_fit(avail: u32, entries: usize) -> LegendFit {
    let Ok(n) = u32::try_from(entries) else {
        return LegendFit {
            slot: LEGEND_MIN_SLOT,
            shown: 0,
            hidden: entries,
        };
    };
    if n == 0 {
        return LegendFit {
            slot: LEGEND_SLOT,
            shown: 0,
            hidden: 0,
        };
    }
    let ideal = avail / n;
    if ideal >= LEGEND_MIN_SLOT {
        return LegendFit {
            slot: ideal.min(LEGEND_SLOT),
            shown: entries,
            hidden: 0,
        };
    }
    // Not even the minimum slot fits every entry, so a `+N` marker will stand in
    // for the tail. If the marker itself does not fit, the row is omitted whole:
    // a legend area narrower than one `+N` is a sub-`LEGEND_OVERFLOW_SLOT`-px
    // chart, and a marker painted there would itself overrun the frame — the very
    // thing this fit exists to prevent. The count is dropped with it (there is
    // nowhere to show it), which only a degenerate chart width reaches.
    if avail < LEGEND_OVERFLOW_SLOT {
        return LegendFit {
            slot: LEGEND_MIN_SLOT,
            shown: 0,
            hidden: 0,
        };
    }
    // Draw as many as fit beside the marker. `shown` may be 0 — a chart wide
    // enough for the marker but not one entry says only "+N", still true and
    // still inside its own frame.
    let shown = (avail.saturating_sub(LEGEND_OVERFLOW_SLOT) / LEGEND_MIN_SLOT) as usize;
    let shown = shown.min(entries);
    LegendFit {
        slot: LEGEND_MIN_SLOT,
        shown,
        hidden: entries - shown,
    }
}

/// The width (px) a legend row actually occupies when it seats `entries`
/// entries in `avail` px — `shown * slot` plus the `+N` marker's slot when any
/// were dropped (R1396). Always `<= avail`. The donut centres its legend and
/// needs this to place the row's left edge; the top-band charts start at a fixed
/// `margin.left` and do not.
///
/// R1722 — the row's painters moved to `crate::legend` and are private there, so
/// only [`crate::Legend::width`] reaches this on a chart's behalf.
pub(crate) fn legend_row_width(avail: u32, entries: usize) -> u32 {
    let fit = legend_fit(avail, entries);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the shown count is a small display count; the width stays within u32"
    )]
    let shown_w = (fit.shown as u32) * fit.slot;
    shown_w
        + if fit.hidden > 0 {
            LEGEND_OVERFLOW_SLOT
        } else {
            0
        }
}

/// R1438 — the COLOUR BAR: the value-encoding legend a continuous
/// `ColorScale` needs, and the reason a swatch row cannot serve one. A
/// categorical [`crate::Legend`] answers "which series is this colour"; a colour
/// bar answers "how big is this colour", which is a *ramp* plus the domain it
/// spans, not a list of discrete entries.
///
/// Emits a gradient strip at `(x, row_y, w, h)` tagged `{prefix}.colorbar.strip`,
/// plus `{prefix}.colorbar.tick.{k}` labels beneath it: the domain ends always,
/// and — for a diverging encoding — the neutral, seated at the position the map
/// actually puts it.
///
/// **The stops are computed from the MAPPING, not copied off the scale.** For a
/// sequential ramp the two coincide, but a diverging ramp over an asymmetric
/// domain does not: `ColorScale::map_diverging` normalises each wing on its own
/// width (R1436), so the neutral sits at the neutral's fraction of the domain
/// rather than at the ramp's midpoint. Building the bar from `stop_offsets`
/// means the legend shows the encoding the marks were painted with — a bar that
/// merely re-spaced the scale's own stops would misreport exactly the case the
/// diverging map exists to fix.
///
/// Superior to a pixel-only colour-scale widget (the toolkit CP color scale
/// shape) in the way that matters here: the strip is a real continuous [`Gradient`],
/// so it renders as a smooth ramp, AND its stops ride in the scene as data —
/// an introspecting client reads the offsets and colours out of `scene/snapshot` and can
/// verify a mark's fill against the published ramp without sampling a single
/// pixel (§2 #7). Which way a [`color_bar`]'s VALUE axis runs (R1439).
///
/// Not a cosmetic rotation. A horizontal bar's value axis runs left→right,
/// which is also the direction [`Gradient::horizontal`] paints, so a stop's
/// domain fraction IS its gradient offset. A vertical bar's value axis runs
/// **upward** — the thermometer convention every colour-scale legend uses, high
/// at the top — while [`Gradient::vertical`] paints top→**down**. The two
/// therefore disagree, and the vertical arm has to mirror the stops. Getting
/// that wrong does not fail loudly: it silently paints an upside-down legend,
/// telling the reader that the ramp's low colour means a high value.
///
/// The axis also decides where the ticks go: beneath a horizontal bar, centred
/// on their fraction; to the right of a vertical one, centred on their row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BarAxis {
    /// Value runs left → right; ticks sit under the strip.
    Horizontal,
    /// Value runs bottom → top; ticks sit to the right of the strip.
    Vertical,
}

/// Width (px) of the centred tick-label box under a [`BarAxis::Horizontal`]
/// bar, where a label has the whole band to itself.
const COLOR_BAR_LABEL_SLOT: u32 = 60;

/// Width (px) of the tick-label box beside a [`BarAxis::Vertical`] bar. Narrower
/// than the horizontal slot because the box sits in a gutter carved out of the
/// chart's own drawing area, so every px it takes is a px the marks lose — and a
/// left-aligned domain end (`-4.2`, `1.2M`) needs far less than a centred one.
const COLOR_BAR_VALUE_SLOT: u32 = 44;

/// Gap (px) between a vertical bar's strip and its tick labels.
const COLOR_BAR_LABEL_GAP: u32 = 4;

/// Total width (px) a [`BarAxis::Vertical`] bar occupies for a `strip_w`-wide
/// strip: the strip, the gap, and the tick-label box.
///
/// A chart that carves a gutter for the bar derives it from HERE rather than
/// re-adding the same three numbers, so the reserved space and the drawn space
/// cannot drift apart — a gutter one px short would let the outermost tick label
/// overhang the chart, which is the class R1396 clamped for.
pub(crate) const fn vertical_bar_width(strip_w: u32) -> u32 {
    strip_w + COLOR_BAR_LABEL_GAP + COLOR_BAR_VALUE_SLOT
}

pub(crate) fn color_bar(
    stops: &[(f32, Color)],
    ticks: &[(f32, f64)],
    rect: Rect,
    axis: BarAxis,
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let mut gradient = match axis {
        BarAxis::Horizontal => Gradient::horizontal(),
        BarAxis::Vertical => Gradient::vertical(),
    };
    // A vertical bar's stops are mirrored (see [`BarAxis`]): the domain's high
    // end is the ramp's LAST colour and must paint at the TOP, which is
    // gradient offset 0. Reversing the iteration order as well keeps the stop
    // list ascending in offset, the form a gradient is defined on.
    match axis {
        BarAxis::Horizontal => {
            for &(offset, color) in stops {
                gradient = gradient.with_stop(offset, color);
            }
        }
        BarAxis::Vertical => {
            for &(offset, color) in stops.iter().rev() {
                gradient = gradient.with_stop(1.0 - offset, color);
            }
        }
    }
    // The flat fallback fill is the colour that paints at the strip's ORIGIN
    // under each axis, so a backend that ignores gradients (the TUI adapter)
    // degrades to a plausible solid rather than an inverted one.
    let origin_stop = match axis {
        BarAxis::Horizontal => stops.first(),
        BarAxis::Vertical => stops.last(),
    };
    let fill = origin_stop.map_or(style.label, |&(_, c)| c);
    // Absolutely placed like every other chart box ([`box_node`]) — without the
    // layout the strip lands at zero height and the bar is invisible.
    let mut out = vec![Scene::Box(
        BoxNode::new(rect, BoxStyle::filled(fill).with_gradient(gradient))
            .with_tag(format!("{prefix}.colorbar.strip"))
            .with_layout(absolute(rect)),
    )];
    let size = style.label_size_px.max(1);
    // A bar's ticks are the domain's real endpoints, not multiples of an axis
    // step, so their precision comes from the VALUES — see [`value_decimals`].
    let decimals = value_decimals(&ticks.iter().map(|&(_, v)| v).collect::<Vec<_>>());
    for (k, &(offset, value)) in ticks.iter().enumerate() {
        let tag = format!("{prefix}.colorbar.tick.{k}");
        let text = format_at_decimals(value, decimals);
        #[allow(
            clippy::cast_precision_loss,
            reason = "bar geometry is display-sized; the f32 seat is exact here"
        )]
        let node = match axis {
            BarAxis::Horizontal => label_node(
                text,
                centered_label_x(
                    rect.x as f32 + offset * rect.w as f32,
                    COLOR_BAR_LABEL_SLOT,
                    spanning_x(rect),
                ),
                rect.y + rect.h + 2,
                COLOR_BAR_LABEL_SLOT,
                TextAlign::Center,
                style.label,
                size,
                tag,
            ),
            BarAxis::Vertical => label_node(
                text,
                rect.x + rect.w + COLOR_BAR_LABEL_GAP,
                // 1 - offset: the value axis runs up, the pixel axis down.
                // Seated by the BOX height, not the glyph size — a box centred
                // by its glyph height overhangs by the padding, which shows up
                // as the bottom tick painting under the chart.
                centered_label_y(
                    rect.y as f32 + (1.0 - offset) * rect.h as f32,
                    label_box_h(size),
                    spanning_y(rect, label_box_h(size)),
                ),
                COLOR_BAR_VALUE_SLOT,
                TextAlign::Start,
                style.label,
                size,
                tag,
            ),
        };
        out.push(node);
    }
    out
}

/// R1440 — the whole horizontal colour bar for a chart that HAS a legend band:
/// the band rect plus the encoding's stops and ticks, or nothing when the chart
/// is not encoding by value.
///
/// The cartesian charts (scatter R1438, line R1440) put their bar across the
/// same top band their swatch row would have used, at the same margins — so at
/// the 3rd consumer that seating became one definition rather than each chart
/// re-deriving the identical rect. The treemap keeps its own placement: it has no
/// band, and its bar stands [`BarAxis::Vertical`] in a side gutter.
pub(crate) fn legend_band_color_bar(
    encoding: &ValueEncoding,
    domain: Option<(f64, f64)>,
    rect: Rect,
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let Some(domain) = domain else {
        return Vec::new();
    };
    let Some(ramp) = encoding.bar(domain) else {
        return Vec::new();
    };
    if ramp.stops.is_empty() {
        return Vec::new();
    }
    let bar = Rect::new(
        rect.x + style.margin.left,
        rect.y + 2,
        rect.w
            .saturating_sub(style.margin.left + style.margin.right)
            .max(1),
        style.label_size_px.max(6),
    );
    color_bar(
        &ramp.stops,
        &ramp.ticks,
        bar,
        BarAxis::Horizontal,
        style,
        prefix,
    )
}

/// Top edge of a `height`-tall label box centred on `center_px`, clamped so the
/// whole box stays inside `bounds` — the vertical twin of [`centered_label_x`],
/// added at R1439 for the vertical colour bar's row-seated ticks.
pub(crate) fn centered_label_y(center_px: f32, height: u32, bounds: Rect) -> u32 {
    let max_y = (bounds.y + bounds.h).saturating_sub(height).max(bounds.y);
    to_u32(center_px)
        .saturating_sub(height / 2)
        .clamp(bounds.y, max_y)
}

/// The clamp frame for a horizontal [`color_bar`]'s tick labels: the bar's own
/// span widened by half a label slot at each end, so the end ticks centre on the
/// bar ends instead of being pulled inward the way a plot-clamped label is.
fn spanning_x(bar: Rect) -> Rect {
    let pad = COLOR_BAR_LABEL_SLOT / 2;
    Rect::new(bar.x.saturating_sub(pad), bar.y, bar.w + pad * 2, bar.h)
}

/// The clamp frame for a vertical [`color_bar`]'s tick labels — the same idea
/// on the other axis: the strip's span widened by half a label height at each
/// end, so the end ticks centre on the strip ends rather than being pulled in.
fn spanning_y(bar: Rect, label_h: u32) -> Rect {
    let pad = label_h.div_ceil(2);
    Rect::new(bar.x, bar.y.saturating_sub(pad), bar.w, bar.h + pad * 2)
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

#[cfg(test)]
mod tests {
    use super::*;

    // ─── the two x-axis label emitters (R1567) ───────────────────────────
    //
    // Both were lifted here at their third consumer, and a counterfactual
    // pass found each one's defining property UNTESTED at every consumer it
    // came from: three charts asserted that a slot label exists and what it
    // says, and none that it sits under the slot it names. A lift inherits
    // its sources' coverage, so those holes are closed here rather than in
    // one of the three.

    /// ★ A category label is centred under the band it names — the property
    /// that makes it a *slot* label rather than a string at the plot's left
    /// edge. All three category charts read this one definition.
    #[test]
    fn r1567_a_category_label_is_centred_under_its_own_band() {
        let cats = crate::scale::Categories::new(["a", "b", "c"]);
        let x = CategoryScale::new(cats.clone(), cats.extent(), (100.0, 400.0));
        let format = TickFormat::Category(cats);
        let style = ChartStyle::default();
        let mut lefts = Vec::new();
        for i in 0..3 {
            let node = category_label_node(&x, i, 100.0, 200.0, &format, style.label, 11, "chart");
            let Scene::Text(t) = &node else {
                panic!("a label is a text node")
            };
            let (band_lo, band_hi) = x.band(i).expect("placed");
            let (left_px, top_px) = t.layout.absolute_position.expect("absolutely placed");
            assert_eq!(left_px, to_u32(band_lo), "label {i} starts at its band");
            assert_eq!(
                top_px,
                200 + X_LABEL_GAP,
                "label {i} sits under the baseline"
            );
            let want = to_u32(band_hi - band_lo).max(1);
            assert_eq!(
                t.layout.size.width,
                SizeValue::Px(want),
                "label {i} spans its band"
            );
            assert_eq!(t.tag.as_deref(), Some(&*format!("chart.xlabel.{i}")));
            lefts.push(left_px);
        }
        assert!(
            lefts.windows(2).all(|w| w[0] < w[1]),
            "the three labels ascend with their slots: {lefts:?}"
        );
    }

    /// ★ A numeric x label is emitted only for a tick the scale can PLACE.
    ///
    /// Defensive in every shipped chart — `axis_ticks` clips to the domain,
    /// so a live tick set holds no unmappable value — which is exactly why
    /// nothing exercised it, and why a counterfactual that dropped the guard
    /// went unnoticed. Asked of the helper directly, where the case can be
    /// constructed.
    #[test]
    fn r1567_a_numeric_x_label_skips_a_tick_the_scale_cannot_place() {
        let scale = ValueScale::Log(crate::scale::LogScale::new(
            (1.0, 100.0),
            (0.0, 300.0),
            10.0,
        ));
        assert!(scale.map(0.0).is_none(), "a log axis cannot place zero");
        let out = x_tick_labels(
            &scale,
            &[0.0, 1.0, 10.0, 100.0],
            200.0,
            Rect::new(0, 0, 320, 240),
            &TickFormat::Log,
            &ChartStyle::default(),
            "chart",
        );
        assert_eq!(out.len(), 3, "the unplaceable tick emits no label");
        // ...and the survivors keep the INDEX of the tick they came from, so a
        // consumer reading `chart.label.x.2` gets the third tick and not the
        // third surviving one.
        let tags: Vec<Option<String>> = out
            .iter()
            .map(|n| n.tag().map(ToString::to_string))
            .collect();
        assert_eq!(
            tags,
            vec![
                Some("chart.label.x.1".to_string()),
                Some("chart.label.x.2".to_string()),
                Some("chart.label.x.3".to_string()),
            ]
        );
    }

    // ─── centered_label_x (R1396) ────────────────────────────────────────

    #[test]
    fn centered_label_stays_inside_its_bounds_at_both_edges() {
        // The whole box (x .. x + slot) must stay within [bounds.x, bounds.x + w].
        let bounds = Rect::new(0, 0, 300, 100);
        let slot = 60;
        // A tick centred at the right edge would want x = 300 - 30 = 270, so the
        // box would end at 330 — 30px past the edge. It clamps to 300 - 60 = 240.
        assert_eq!(centered_label_x(300.0, slot, bounds), 240);
        // A tick centred at the left edge would want a negative x; it clamps to 0.
        assert_eq!(centered_label_x(0.0, slot, bounds), 0);
        // A tick comfortably in the middle is centred verbatim (150 - 30 = 120).
        assert_eq!(centered_label_x(150.0, slot, bounds), 120);
    }

    #[test]
    fn centered_label_respects_a_nonzero_bounds_origin() {
        // A docked chart's rect does not start at 0 — the clamp must use the
        // rect's own left AND right, not the window's.
        let bounds = Rect::new(100, 0, 200, 100); // x in [100, 300]
        let slot = 60;
        assert_eq!(centered_label_x(100.0, slot, bounds), 100); // left clamp = bounds.x
        assert_eq!(centered_label_x(300.0, slot, bounds), 240); // right clamp = 300 - 60
    }

    #[test]
    fn centered_label_with_a_slot_wider_than_bounds_clamps_to_the_left() {
        // Degenerate: the box cannot fit; it pins to the left edge and lets the
        // label's own Clip scissor it, rather than going negative or overflowing.
        let bounds = Rect::new(10, 0, 40, 100);
        assert_eq!(centered_label_x(30.0, 60, bounds), 10);
    }

    // ─── legend_fit / legend_row_width (R1396) ───────────────────────────

    #[test]
    fn legend_fits_every_entry_at_the_preferred_slot_when_wide() {
        // 300px, 2 entries → ideal 150, capped at the 104 preferred slot.
        let fit = legend_fit(300, 2);
        assert_eq!(fit.slot, LEGEND_SLOT);
        assert_eq!(fit.shown, 2);
        assert_eq!(fit.hidden, 0);
        assert!(legend_row_width(300, 2) <= 300);
    }

    #[test]
    fn legend_shrinks_the_slot_before_dropping_anything() {
        // 160px, 2 entries → ideal 80, above the 44 minimum: shrink, keep both.
        let fit = legend_fit(160, 2);
        assert_eq!(fit.slot, 80);
        assert_eq!(fit.shown, 2);
        assert_eq!(fit.hidden, 0);
    }

    #[test]
    fn legend_drops_to_a_plus_n_marker_when_even_the_minimum_will_not_fit() {
        // 100px, 4 entries → ideal 25 < 44 minimum. Reserve 32 for `+N`, seat
        // (100 - 32) / 44 = 1 entry, drop the other 3.
        let fit = legend_fit(100, 4);
        assert_eq!(fit.slot, LEGEND_MIN_SLOT);
        assert_eq!(fit.shown, 1);
        assert_eq!(fit.hidden, 3);
        // Never wider than the space it was given.
        assert!(legend_row_width(100, 4) <= 100);
    }

    #[test]
    fn legend_row_width_never_exceeds_avail_across_a_sweep() {
        // The containment invariant the whole clamp rests on: for any width and
        // entry count, the laid row fits. Swept so no off-by-one slips through.
        for avail in [0u32, 10, 43, 44, 60, 88, 104, 200, 401] {
            for n in 0usize..=8 {
                assert!(
                    legend_row_width(avail, n) <= avail,
                    "width {avail} n {n} → {}",
                    legend_row_width(avail, n)
                );
            }
        }
    }

    #[test]
    fn empty_legend_is_a_no_op_and_zero_entries_shows_nothing() {
        let fit = legend_fit(300, 0);
        assert_eq!(fit.shown, 0);
        assert_eq!(fit.hidden, 0);
        assert_eq!(legend_row_width(300, 0), 0);
    }
}
