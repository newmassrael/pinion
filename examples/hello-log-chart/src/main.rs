//! `hello-log-chart` — R1528 §5.38 the value axis can be **logarithmic**.
//!
//! The forcing consumer for [`pinion_chart::LineChart::y_log`], the crate's
//! first non-affine axis (Qt's `QLogValueAxis`). Until R1528 every axis in
//! `pinion-chart` was a [`pinion_chart::LinearScale`], which is the right
//! default and the wrong one for the data every profiler and monitoring tool
//! actually carries: latency percentiles.
//!
//! ## What the toggle shows
//!
//! Three percentile series share one plot and span four decades — `p50`
//! around 0.4 ms, `p99` around 40 ms, `p99.9` around 700 ms. On the **linear**
//! axis the y-domain is pinned by `p99.9`, so `p50` and most of `p99` are
//! pressed into a few pixels above the baseline: the axis is technically
//! correct and shows nothing. Toggling to **log** puts equal *ratios* on equal
//! pixel spans, and all three series become readable at once. The toggle is
//! the counterfactual, on screen, for the whole round.
//!
//! ## The zero sample is the point, not an accident
//!
//! Bucket 6 took no traffic, so its `p50` is `0`. A log axis has no pixel for
//! zero — and drawing it on the baseline would be indistinguishable from a
//! real sample at the domain floor, which is exactly the lie
//! [`pinion_chart::Mapped::unreadable`] exists to prevent on the *other* input
//! path (a cell that holds no number). So the sample draws no mark, and
//! [`LineChart::off_scale`](pinion_chart::LineChart::off_scale) reports it;
//! the caption under the chart is that report. Switch back to linear and the
//! caption says so: being off-scale is a property of the **axis**, not of the
//! data.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the axis as tagged data — `chart.label.y.{k}` are
//! the decade labels (`0.1 / 1 / 10 / 100 / 1k / 10k`), `chart.grid.y.{k}` the
//! labelled decade lines, and `chart.grid.minor.y.{k}` the fainter per-decade
//! subdivisions without which evenly-spaced decade lines read as a linear axis.
//! Equal decade spacing is read off the gridline rects; no pixels are sampled
//! (§2 #1 / §2 #7). See `tools/demos/r1528_log_axis.py`.

use pinion_a11y::{AccessNode, ToggleSegment, WidgetA11y, toggle_button_group_nodes};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloLogChartRenderer, HelloLogChartRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 420;
const THEME_TAG: &str = "app";

/// The scale toggle's dispatch + Tab-stop tag. One focusable tagged
/// container bound to a `ToggleExternal`, the same chip mechanism the chart's
/// own interactive legend uses (R1380) — done by hand here because what it
/// toggles is the AXIS, not a series.
const SCALE_TAG: &str = "scale_toggle";

/// The WAI-ARIA §3.6 `group` label for the AccessKit toggle button.
const GROUP_TAG: &str = "scale_group";

/// Log at boot: the axis this round exists for is what the window should
/// open on, and the toggle is how a reader reaches the comparison.
const BOOT_LOG: bool = true;

const TITLE_FONT_PX: u32 = 17;
const CAPTION_FONT_PX: u32 = 12;
const TOGGLE_FONT_PX: u32 = 13;

/// Window-absolute plot region. The chart must be handed its final geometry
/// before layout runs (see the `pinion-chart` coordinate contract), so the
/// rect is a constant; the caption sits in the gap below it.
const CHART_RECT: Rect = Rect::new(16, 60, WIN_W - 32, WIN_H - 126);

/// Series names, ordered smallest-magnitude first so the one the linear axis
/// flattens is series 0.
const LABELS: [&str; 3] = ["p50", "p99", "p99.9"];

/// Series colours, pinned so each legend swatch matches its line.
const SERIES_COLORS: [Color; 3] = [
    Color::rgb(0x42, 0x85, 0xf4),
    Color::rgb(0xf9, 0xab, 0x00),
    Color::rgb(0xea, 0x43, 0x35),
];

/// The bucket whose `p50` is zero — no traffic, so no median latency. Named
/// rather than hunted for, because the caption and the tests both point at it.
const IDLE_BUCKET: usize = 6;

/// Request-latency percentiles in milliseconds over 12 buckets — four decades
/// of real structure, which is the only reason a log axis is worth having.
///
/// `p50[IDLE_BUCKET]` is `0`: that bucket took no traffic. It is data, not a
/// sentinel, and a log axis cannot place it — see the module docs.
#[allow(
    clippy::cast_precision_loss,
    reason = "bucket index (0..12) -> f64 x-coordinate is exact"
)]
fn latency_series() -> Vec<Series> {
    let ys: [[f64; 12]; 3] = [
        [
            0.42, 0.51, 0.38, 0.61, 0.44, 0.72, 0.0, 0.55, 0.49, 0.83, 0.58, 0.47,
        ],
        [
            28.0, 34.0, 22.0, 51.0, 39.0, 88.0, 12.0, 44.0, 31.0, 96.0, 47.0, 36.0,
        ],
        [
            610.0, 720.0, 480.0, 910.0, 660.0, 1400.0, 210.0, 780.0, 590.0, 1600.0, 840.0, 700.0,
        ],
    ];
    (0..3)
        .map(|i| {
            let points = ys[i]
                .iter()
                .enumerate()
                .map(|(k, &y)| DataPoint::new(k as f64, y))
                .collect();
            Series::new(LABELS[i], points).with_color(SERIES_COLORS[i])
        })
        .collect()
}

/// The chart for one scale choice — the ONE place `y_log` is applied, so the
/// painted axis, the caption's off-scale report, and the tests all read the
/// same chart rather than three separately-configured ones.
fn chart_for(log: bool) -> LineChart {
    let chart = LineChart::new(latency_series());
    if log { chart.y_log() } else { chart }
}

/// The themed chart style.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        legend: true,
        label_size_px: 13,
        x_ticks: 7,
        y_ticks: 6,
        ..ChartStyle::default()
    }
}

/// The caption under the plot: what this axis could not draw, and why.
///
/// Reads [`LineChart::off_scale`](pinion_chart::LineChart::off_scale) rather
/// than restating the rule, so a caption that disagreed with the plot would be
/// a bug in the crate and not in this string.
fn caption(log: bool) -> String {
    let off = chart_for(log).off_scale();
    if off.is_empty() {
        return "linear scale — every sample is plotted, and the small \
                percentiles are pressed onto the baseline"
            .to_string();
    }
    let names: Vec<&str> = off
        .iter()
        .map(|o| LABELS.get(o.series).copied().unwrap_or("?"))
        .collect();
    format!(
        "log scale — {} sample not plotted ({} at bucket {}): a log axis has \
         no zero, so it is reported, not drawn on the baseline",
        off.len(),
        names.join(", "),
        IDLE_BUCKET,
    )
}

/// The scale toggle: a focusable tagged container the router dispatches
/// clicks to, painted as a chip with an on/off swatch.
fn scale_toggle(on: bool, theme: &Theme) -> Scene {
    let swatch = Scene::Box(
        pinion_core::scene::BoxNode::new(
            Rect::default(),
            BoxStyle::filled(if on {
                theme.resolve(ColorRole::Accent)
            } else {
                theme.resolve(ColorRole::Outline)
            })
            .with_corner_radius(3),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(TOGGLE_FONT_PX, TOGGLE_FONT_PX))),
    );
    let label = Scene::Text(TextNode::styled(
        "logarithmic y-axis",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TOGGLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![swatch, label])
            .with_tag(SCALE_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(6)
                    .with_focusable(true)
                    .with_absolute_position(WIN_W - 190, 22)
                    .with_size(Size::px(176, TOGGLE_FONT_PX + 8)),
            ),
    )
}

/// view-fn (§6.3): pure sync `ScaleState -> Scene`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ScaleState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Request latency percentiles (ms)",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 22)),
    );

    let chart = chart_for(state.log).build(CHART_RECT, &chart_style(&theme));

    let caption = Scene::Text(
        TextNode::styled(
            caption(state.log),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CAPTION_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("caption".to_string())
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(18, WIN_H - 56)
                .with_size(Size::px(WIN_W - 36, 40)),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![chart, title, scale_toggle(state.log, &theme), caption])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

/// Which scale the value axis is on, plus the toggle chip's visual state.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct ScaleState {
    toggle: ToggleState,
    log: bool,
}

struct LogChartView;

impl WidgetCore for LogChartView {
    type State = ScaleState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(toggle_group::boot_toggle(BOOT_LOG))
    }

    fn tag() -> &'static str {
        SCALE_TAG
    }

    fn read_state(scene: &Scene) -> ScaleState {
        let (toggle, log) = toggle_group::read_toggle(scene, SCALE_TAG);
        ScaleState { toggle, log }
    }

    fn view(state: ScaleState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-log-chart (R1528 §5.38 logarithmic value axis)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        toggle_group::apply_key(scene, focused, key, &[SCALE_TAG])
    }

    fn fmt_state_log(state: &ScaleState) -> String {
        format!(
            "{}{}",
            state.toggle.as_name(),
            if state.log { " log" } else { " linear" }
        )
    }
}

impl WidgetA11y for LogChartView {
    fn access_node(state: &ScaleState, focused: Option<&str>) -> Vec<AccessNode> {
        let segments = [ToggleSegment {
            tag: SCALE_TAG,
            label: "logarithmic y-axis",
            state: state.toggle,
            on: state.log,
        }];
        toggle_button_group_nodes(GROUP_TAG, "Value axis scale", &segments, focused)
    }
}

impl WidgetView for LogChartView {
    type Renderer = HelloLogChartRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<LogChartView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// A settled (non-hovered, non-pressed) chip on the given scale — the
    /// state the view is exercised from.
    const fn idle(log: bool) -> ScaleState {
        ScaleState {
            toggle: ToggleState::Idle,
            log,
        }
    }

    fn render(log: bool) -> Scene {
        Owner::new().run(|| view(idle(log), &Frame::new()))
    }

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        if let Scene::Container(c) = scene {
            return c.children.iter().find_map(|ch| find(ch, tag));
        }
        None
    }

    fn count_prefix(scene: &Scene, prefix: &str) -> usize {
        let mut n = usize::from(scene.tag().is_some_and(|t| t.starts_with(prefix)));
        if let Scene::Container(c) = scene {
            for ch in &c.children {
                n += count_prefix(ch, prefix);
            }
        }
        n
    }

    fn text_of(scene: &Scene, tag: &str) -> String {
        match find(scene, tag) {
            Some(Scene::Text(t)) => t.content.clone(),
            _ => panic!("no text node tagged {tag}"),
        }
    }

    /// The vertical pixel extent of one series' polyline — how much of the
    /// plot that series is allowed to use.
    fn series_extent(scene: &Scene, i: usize) -> u32 {
        let Some(Scene::Path(p)) = find(scene, &format!("chart.series.{i}")) else {
            panic!("series {i} polyline present")
        };
        p.rect.h
    }

    /// ★ The whole round, read off the painted scene: on a linear axis the
    /// small percentile is a sliver; on a log axis it uses the plot.
    #[test]
    fn r1528_the_log_axis_reveals_what_the_linear_one_flattens() {
        let linear = series_extent(&render(false), 0);
        let log = series_extent(&render(true), 0);
        assert!(
            linear < 10,
            "linear: p50 spans {linear}px — pressed onto the baseline"
        );
        assert!(
            log > linear * 4,
            "log: p50 spans {log}px, more than 4x the linear {linear}px"
        );
    }

    #[test]
    fn r1528_the_y_axis_is_labelled_in_decades() {
        let scene = render(true);
        let labels: Vec<String> = (0..6)
            .map(|k| text_of(&scene, &format!("chart.label.y.{k}")))
            .collect();
        // Six decades, not five: the 1600 ms spike opens the `1k` decade and
        // the axis snaps OUTWARD to a whole one, exactly as the linear axis
        // snaps outward to a whole nice step.
        assert_eq!(labels, ["0.1", "1", "10", "100", "1k", "10k"]);
    }

    #[test]
    fn r1528_minor_gridlines_are_present_only_under_the_log_axis() {
        assert!(count_prefix(&render(true), "chart.grid.minor.y.") > 0);
        assert_eq!(count_prefix(&render(false), "chart.grid.minor."), 0);
    }

    /// ★ The zero sample is reported by the caption and drawn by neither
    /// axis-appropriate lie: no baseline vertex on log, an ordinary vertex on
    /// linear. Being off-scale is a property of the axis.
    #[test]
    fn r1528_the_caption_reports_what_the_axis_cannot_draw() {
        let log = text_of(&render(true), "caption");
        assert!(log.contains("1 sample not plotted"), "got {log}");
        assert!(log.contains("p50"), "names the series: {log}");
        let bucket = IDLE_BUCKET.to_string();
        assert!(log.contains(&bucket), "names the bucket");

        let linear = text_of(&render(false), "caption");
        assert!(linear.starts_with("linear scale"), "got {linear}");
        assert!(
            !linear.contains("not plotted"),
            "zero is an ordinary value on a linear axis: {linear}"
        );
    }

    #[test]
    fn r1528_the_scale_toggle_is_a_focusable_tagged_hit_region() {
        let scene = render(true);
        let Some(Scene::Container(chip)) = find(&scene, SCALE_TAG) else {
            panic!("the scale toggle is a focusable container")
        };
        assert!(chip.layout.focusable, "it is a Tab / click target");
    }

    #[test]
    fn r1528_a11y_exposes_the_axis_scale_as_one_toggle_button() {
        let nodes = LogChartView::access_node(&idle(true), None);
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(buttons, 1, "one aria-pressed button for the axis scale");
    }
}
