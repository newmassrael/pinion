//! `hello-time-chart` — R1529 §5.38 the x-axis can be **UTC time**.
//!
//! The forcing consumer for [`pinion_chart::LineChart::x_time`], the crate's
//! third axis kind (Qt's `QDateTimeAxis`, d3's `scaleUtc`). Until R1529 a
//! timestamp reaching a `pinion-chart` axis was a plain number, and the axis
//! every monitoring chart has could not be drawn.
//!
//! ## What the toggle shows, and why it is two defects
//!
//! One dataset — four hours of request latency across a real incident window,
//! `2026-03-02 22:00` to `2026-03-03 02:00` UTC — plotted twice.
//!
//! On the **numeric** x-axis both halves of the axis are wrong at once:
//!
//! * the *ticks* come from the `1 / 2 / 5 x 10^n` nice-number step, which
//!   assumes the quantity subdivides decimally. Above a second time is
//!   mixed-radix, so the gridlines land on multiples of 2,000,000 ms — times
//!   no clock shows.
//! * the *labels* compact by magnitude, and one decimal at the giga scale is
//!   27-hour resolution, so all nine gridlines print `1772.5G`. Nine lines,
//!   one string.
//!
//! Toggling to **UTC time** puts the ticks on the half hour and gives each
//! label the finest calendar field that distinguishes it. The window straddles
//! midnight on purpose: the date appears exactly **once**, on the tick that
//! crosses into `Mar 03`, and every other label is a clock time. That is the
//! multi-resolution property — a reader gets the date where the axis changes
//! day, without every label repeating it.
//!
//! ## The scrub readout is not a tick label
//!
//! The crosshair header shows the full stamp (`2026-03-03 00:40:00`), not the
//! axis's `00:30`. An axis label is *relative* — legible because its
//! neighbours are on screen beside it — and a scrub has no neighbours, so the
//! same string would leave a reader unable to say which day was scrubbed.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the axis as tagged data — `chart.label.x.{k}` are
//! the tick labels and `chart.grid.x.{k}` their gridlines. The whole round is
//! read off those strings; no pixels are sampled (§2 #1 / §2 #7). See
//! `tools/demos/r1529_time_axis.py`.

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
vello_renderer_impl!(HelloTimeChartRenderer, HelloTimeChartRendererError);

const WIN_W: u32 = 680;
const WIN_H: u32 = 430;
const THEME_TAG: &str = "app";

/// The axis toggle's dispatch + Tab-stop tag.
const AXIS_TAG: &str = "axis_toggle";

/// The WAI-ARIA §3.6 `group` label for the AccessKit toggle button.
const GROUP_TAG: &str = "axis_group";

/// Time at boot: the axis this round exists for is what the window should
/// open on, and the toggle is how a reader reaches the comparison.
const BOOT_TIME: bool = true;

const TITLE_FONT_PX: u32 = 17;
const CAPTION_FONT_PX: u32 = 12;
const TOGGLE_FONT_PX: u32 = 13;

/// Window-absolute plot region. The chart must be handed its final geometry
/// before layout runs (see the `pinion-chart` coordinate contract), so the
/// rect is a constant; the caption sits in the gap below it.
const CHART_RECT: Rect = Rect::new(16, 60, WIN_W - 32, WIN_H - 130);

/// Epoch milliseconds at `2026-03-02 22:00:00 UTC` — the incident window's
/// start. A literal rather than a computed "now", so the axis a demo reads is
/// the axis the tests assert (a host-clock-dependent chart would be the
/// R1500 failure: a test that reads its environment).
const T0_MS: f64 = 1_772_488_800_000.0;

/// Sample interval — ten minutes.
const STEP_MS: f64 = 600_000.0;

/// Samples: four hours at ten-minute resolution, inclusive of both ends.
const SAMPLES: usize = 25;

/// Series names.
const LABELS: [&str; 2] = ["p50", "p99"];

/// Series colours, pinned so each legend swatch matches its line.
const SERIES_COLORS: [Color; 2] = [Color::rgb(0x42, 0x85, 0xf4), Color::rgb(0xea, 0x43, 0x35)];

/// Request latency (ms) over the incident window, sampled every ten minutes.
///
/// The x-channel is an epoch millisecond — the unit
/// [`LineChart::x_time`](pinion_chart::LineChart::x_time) reads, matching Qt's
/// `QDateTimeAxis` and d3's `scaleUtc`. The shape is incidental to the round;
/// what matters is that x is a real instant.
#[allow(
    clippy::cast_precision_loss,
    reason = "sample index (0..25) -> f64 offset is exact"
)]
fn latency_series() -> Vec<Series> {
    let p50: [f64; SAMPLES] = [
        41.0, 38.0, 44.0, 40.0, 43.0, 39.0, 42.0, 58.0, 91.0, 140.0, 176.0, 168.0, 151.0, 133.0,
        108.0, 84.0, 61.0, 48.0, 44.0, 41.0, 39.0, 42.0, 40.0, 38.0, 41.0,
    ];
    let p99: [f64; SAMPLES] = [
        280.0, 265.0, 291.0, 274.0, 288.0, 270.0, 296.0, 410.0, 690.0, 1150.0, 1420.0, 1360.0,
        1180.0, 1020.0, 820.0, 610.0, 430.0, 340.0, 305.0, 288.0, 276.0, 294.0, 281.0, 268.0,
        285.0,
    ];
    [p50, p99]
        .iter()
        .enumerate()
        .map(|(i, ys)| {
            let points = ys
                .iter()
                .enumerate()
                .map(|(k, &y)| DataPoint::new(T0_MS + k as f64 * STEP_MS, y))
                .collect();
            Series::new(LABELS[i], points).with_color(SERIES_COLORS[i])
        })
        .collect()
}

/// The chart for one axis choice — the ONE place `x_time` is applied, so the
/// painted axis, the caption, and the tests all read the same chart rather
/// than three separately-configured ones.
fn chart_for(time: bool) -> LineChart {
    let chart = LineChart::new(latency_series()).inspect(Some(0.5));
    if time { chart.x_time() } else { chart }
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

/// Every x-tick label the chart PAINTED, in axis order.
///
/// Read off the scene rather than recomputed, so a caption that disagreed
/// with the plot would be a bug in the crate and not in this string — the
/// same discipline `hello-log-chart`'s caption follows by reading
/// `off_scale()`.
fn x_tick_labels(scene: &Scene) -> Vec<String> {
    let mut out = Vec::new();
    for k in 0..32 {
        match find(scene, &format!("chart.label.x.{k}")) {
            Some(Scene::Text(t)) => out.push(t.content.clone()),
            _ => break,
        }
    }
    out
}

/// The caption under the plot: how many gridlines the axis drew, and how many
/// distinct strings it managed to label them with.
///
/// That ratio IS the defect. It is derived from the painted labels, so the
/// caption cannot claim a legibility the axis does not have.
fn caption(scene: &Scene, time: bool) -> String {
    let labels = x_tick_labels(scene);
    let distinct: std::collections::BTreeSet<&String> = labels.iter().collect();
    if time {
        format!(
            "UTC time x-axis — {} gridlines, {} distinct labels; the date is \
             named once, where the axis crosses into a new day",
            labels.len(),
            distinct.len(),
        )
    } else {
        format!(
            "numeric x-axis — {} gridlines, {} distinct label ({}): a decimal \
             step off the clock, and an epoch millisecond compacted by magnitude",
            labels.len(),
            distinct.len(),
            labels.first().map_or("-", String::as_str),
        )
    }
}

/// The axis toggle: a focusable tagged container the router dispatches clicks
/// to, painted as a chip with an on/off swatch.
fn axis_toggle(on: bool, theme: &Theme) -> Scene {
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
        "UTC time x-axis",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TOGGLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![swatch, label])
            .with_tag(AXIS_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(6)
                    .with_focusable(true)
                    .with_absolute_position(WIN_W - 176, 22)
                    .with_size(Size::px(162, TOGGLE_FONT_PX + 8)),
            ),
    )
}

/// Find the first node carrying `tag`.
fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
    if scene.tag() == Some(tag) {
        return Some(scene);
    }
    if let Scene::Container(c) = scene {
        return c.children.iter().find_map(|ch| find(ch, tag));
    }
    None
}

/// view-fn (§6.3): pure sync `AxisState -> Scene`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: AxisState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Request latency during an incident (ms)",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 22)),
    );

    // Built first: the caption reports what this very scene painted.
    let chart = chart_for(state.time).build(CHART_RECT, &chart_style(&theme));
    let caption_text = caption(&chart, state.time);

    let caption = Scene::Text(
        TextNode::styled(
            caption_text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(CAPTION_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("caption".to_string())
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(18, WIN_H - 58)
                .with_size(Size::px(WIN_W - 36, 44)),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![chart, title, axis_toggle(state.time, &theme), caption])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

/// Which kind the x-axis is on, plus the toggle chip's visual state.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct AxisState {
    toggle: ToggleState,
    time: bool,
}

struct TimeChartView;

impl WidgetCore for TimeChartView {
    type State = AxisState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(toggle_group::boot_toggle(BOOT_TIME))
    }

    fn tag() -> &'static str {
        AXIS_TAG
    }

    fn read_state(scene: &Scene) -> AxisState {
        let (toggle, time) = toggle_group::read_toggle(scene, AXIS_TAG);
        AxisState { toggle, time }
    }

    fn view(state: AxisState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-time-chart (R1529 §5.38 UTC time axis)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        toggle_group::apply_key(scene, focused, key, &[AXIS_TAG])
    }

    fn fmt_state_log(state: &AxisState) -> String {
        format!(
            "{}{}",
            state.toggle.as_name(),
            if state.time { " time" } else { " numeric" }
        )
    }
}

impl WidgetA11y for TimeChartView {
    fn access_node(state: &AxisState, focused: Option<&str>) -> Vec<AccessNode> {
        let segments = [ToggleSegment {
            tag: AXIS_TAG,
            label: "UTC time x-axis",
            state: state.toggle,
            on: state.time,
        }];
        toggle_button_group_nodes(GROUP_TAG, "Horizontal axis kind", &segments, focused)
    }
}

impl WidgetView for TimeChartView {
    type Renderer = HelloTimeChartRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TimeChartView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_chart::{format_time_stamp, format_time_tick};
    use pinion_core::Owner;

    /// A settled (non-hovered, non-pressed) chip on the given axis kind.
    const fn idle(time: bool) -> AxisState {
        AxisState {
            toggle: ToggleState::Idle,
            time,
        }
    }

    fn render(time: bool) -> Scene {
        Owner::new().run(|| view(idle(time), &Frame::new()))
    }

    fn text_of(scene: &Scene, tag: &str) -> String {
        match find(scene, tag) {
            Some(Scene::Text(t)) => t.content.clone(),
            _ => panic!("no text node tagged {tag}"),
        }
    }

    /// ★ The whole round, read off the painted labels. The numeric axis draws
    /// nine gridlines and manages ONE distinct string for them; the time axis
    /// gives every gridline its own.
    #[test]
    fn r1529_the_numeric_axis_labels_nine_gridlines_with_one_string() {
        let numeric = x_tick_labels(&render(false));
        let distinct: std::collections::BTreeSet<&String> = numeric.iter().collect();
        assert_eq!(numeric.len(), 9, "nine gridlines: {numeric:?}");
        assert_eq!(distinct.len(), 1, "one distinct label: {numeric:?}");
        assert_eq!(numeric[0], "1772.5G");

        let timed = x_tick_labels(&render(true));
        let distinct: std::collections::BTreeSet<&String> = timed.iter().collect();
        assert_eq!(timed.len(), 9, "the same nine positions: {timed:?}");
        assert_eq!(distinct.len(), 9, "each one labelled: {timed:?}");
    }

    /// ★ Multi-resolution: the ticks are half-hourly clock times, and the ONE
    /// that crosses into a new day names the day instead. That single label is
    /// the property a fixed format string cannot produce — `HH:MM` everywhere
    /// would lose the date, and `YYYY-MM-DD HH:MM` everywhere would repeat it
    /// on all nine.
    #[test]
    fn r1529_the_date_is_named_once_at_the_midnight_crossing() {
        let labels = x_tick_labels(&render(true));
        assert_eq!(
            labels,
            [
                "22:00", "22:30", "23:00", "23:30", "Mar 03", "00:30", "01:00", "01:30", "02:00"
            ]
        );
        let dated = labels.iter().filter(|l| l.starts_with("Mar")).count();
        assert_eq!(dated, 1, "exactly one label carries the date");
    }

    /// ★ A readout is not a tick label: the scrub header carries the full
    /// stamp, because it has no neighbouring labels to read the day from.
    #[test]
    fn r1529_the_scrub_header_is_a_full_stamp_not_an_axis_label() {
        // The scrub sits mid-plot, on the 23:50 sample.
        let focus = T0_MS + 11.0 * STEP_MS;
        let header = text_of(&render(true), "chart.inspect.header");
        assert_eq!(header, format!("x = {}", format_time_stamp(focus)));
        assert!(
            header.contains("2026-03-02"),
            "the scrub says which day: {header}"
        );
        // The axis label for that same instant is relative, and shorter.
        assert_eq!(format_time_tick(focus), "23:50");
        assert!(
            !x_tick_labels(&render(true))
                .iter()
                .any(|l| header.ends_with(l.as_str())),
            "no tick label is the full stamp"
        );

        // Off a time axis the two forms coincide, which is why the
        // distinction did not exist before this round.
        let numeric = text_of(&render(false), "chart.inspect.header");
        assert_eq!(numeric, "x = 1772.5G");
    }

    /// The numeric axis is unchanged — this is an opt-in, not a
    /// reinterpretation of every chart's x-channel.
    #[test]
    fn r1529_the_caption_reports_what_each_axis_achieved() {
        let numeric = text_of(&render(false), "caption");
        assert!(numeric.starts_with("numeric x-axis"), "got {numeric}");
        assert!(
            numeric.contains("9 gridlines, 1 distinct label"),
            "{numeric}"
        );

        let timed = text_of(&render(true), "caption");
        assert!(timed.starts_with("UTC time x-axis"), "got {timed}");
        assert!(timed.contains("9 gridlines, 9 distinct labels"), "{timed}");
    }

    #[test]
    fn r1529_the_axis_toggle_is_a_focusable_tagged_hit_region() {
        let scene = render(true);
        let Some(Scene::Container(chip)) = find(&scene, AXIS_TAG) else {
            panic!("the axis toggle is a focusable container")
        };
        assert!(chip.layout.focusable, "it is a Tab / click target");
    }

    #[test]
    fn r1529_a11y_exposes_the_axis_kind_as_one_toggle_button() {
        let nodes = TimeChartView::access_node(&idle(true), None);
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(buttons, 1, "one aria-pressed button for the axis kind");
    }
}
