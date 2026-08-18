//! `hello-rescale-toggle` — R1381 §5.38 hide the dominant series to **rescale**
//! the axes to what is left.
//!
//! The forcing consumer for [`pinion_chart::LineChart::rescale_to_visible`]:
//! three series of wildly different magnitude (`total` ~4k, `cache` ~900,
//! `errors` ~90) share one plot. With every series visible the y-axis is pinned
//! to `total`, so `errors` is an unreadable sliver at the baseline. Because the
//! chart is built `rescale_to_visible(true)`, hiding `total` (via the R1380
//! interactive legend) snaps the y-domain to the remaining visible series, and
//! `errors` grows to fill the plot — the interactive-dashboard behaviour the
//! stable-grid default ([`hello-legend-toggle`]) deliberately does not do.
//!
//! Everything else is R1380 verbatim: the chart-owned interactive legend is the
//! toggle surface (a 6th consumer of the [`pinion_core::widgets::toggle_group`]
//! substrate), driving [`pinion_chart::Series::visible`]. The ONE new ingredient
//! is `.rescale_to_visible(true)`.

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_chart::{
    ChartLegend, ChartStyle, DataPoint, LegendInteraction, LegendPostures, LineChart, Series,
};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRescaleToggleRenderer, HelloRescaleToggleRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";

/// Series (and legend-entry) count.
const N: usize = 3;

/// Per-entry dispatch + Tab-stop tags — both the chart's toggle legend entry
/// tags AND the tags the `ToggleExternal`s bind to (R1380).
///
/// R1722 — derived from the chart's tag prefix, not chosen here.
const LEGEND_TAGS: [&str; N] = ["chart.legend.0", "chart.legend.1", "chart.legend.2"];

/// Series names — ordered biggest-magnitude first so entry 0 is the dominant
/// series a viewer hides to reveal the rest.
const LABELS: [&str; N] = ["total", "cache", "errors"];

/// Every series visible at boot.
const BOOT_ON: [bool; N] = [true, true, true];

/// Series colours, pinned so each legend swatch matches its line.
const SERIES_COLORS: [Color; N] = [
    Color::rgb(0x42, 0x85, 0xf4),
    Color::rgb(0x34, 0xa8, 0x53),
    Color::rgb(0xea, 0x43, 0x35),
];

/// Window-absolute plot region. The legend lives in the chart's top margin.
const CHART_RECT: Rect = Rect::new(20, 44, WIN_W - 40, WIN_H - 64);
const TITLE_FONT_PX: u32 = 18;

/// The three sample series, decreasing in magnitude (total >> cache >> errors)
/// so the rescale is dramatic; `visible[i]` gates series `i`'s geometry.
#[allow(
    clippy::cast_precision_loss,
    reason = "bucket index (0..12) -> f64 x-coordinate is exact"
)]
fn series_with(visible: [bool; N]) -> Vec<Series> {
    let ys: [[f64; 12]; N] = [
        [
            2800.0, 3000.0, 3200.0, 3100.0, 3400.0, 3600.0, 3300.0, 3800.0, 4000.0, 3900.0, 4200.0,
            4400.0,
        ],
        [
            400.0, 450.0, 520.0, 600.0, 580.0, 700.0, 760.0, 720.0, 800.0, 850.0, 820.0, 900.0,
        ],
        [
            20.0, 35.0, 28.0, 50.0, 45.0, 70.0, 65.0, 40.0, 55.0, 85.0, 60.0, 90.0,
        ],
    ];
    (0..N)
        .map(|i| {
            let points = ys[i]
                .iter()
                .enumerate()
                .map(|(k, &y)| DataPoint::new(k as f64, y))
                .collect();
            Series::new(LABELS[i], points)
                .with_color(SERIES_COLORS[i])
                .with_visible(visible[i])
        })
        .collect()
}

/// The chart this view paints, for the visibility mask `visible`.
///
/// One definition, because the paint and the accessibility tree must be built
/// from the same chart: `access_node` asks it for `legend_access_nodes`, which
/// seats the row exactly as `build` did (R1722).
fn chart(visible: [bool; N]) -> LineChart {
    LineChart::new(series_with(visible))
        .with_legend(LegendInteraction::Toggle)
        .rescale_to_visible(true)
}

/// The themed chart style. Interactive legend on; the rescale opt-in is set on
/// the builder, not here.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        legend: true,
        label_size_px: 14,
        x_ticks: 7,
        y_ticks: 5,
        ..ChartStyle::default()
    }
}

/// Cached projection: one `(ToggleState, on)` pair per series legend entry.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct LegendState {
    rows: [(ToggleState, bool); N],
}

impl LegendState {
    fn idle() -> Self {
        Self {
            rows: [(ToggleState::Idle, false); N],
        }
    }

    /// The per-series visibility mask (each entry's `on`).
    fn visibility(self) -> [bool; N] {
        let mut v = [false; N];
        for (i, r) in self.rows.iter().enumerate() {
            v[i] = r.1;
        }
        v
    }
}

/// view-fn (§6.3): pure sync `LegendState -> Scene`. The chart is rebuilt with
/// the visibility mask AND `rescale_to_visible(true)` — hiding a series both
/// drops its line and lets the axes snap to the survivors.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: LegendState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Throughput — hide the big series to rescale the small ones",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(20, 14)),
    );

    let chart = chart(state.visibility()).build(CHART_RECT, &chart_style(&theme));

    Scene::Container(
        ContainerNode::new(vec![chart, title])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

struct RescaleToggleView;

impl WidgetCore for RescaleToggleView {
    type State = LegendState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(toggle_group::boot_toggle(BOOT_ON[0]))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        toggle_group::extra_toggles(&LEGEND_TAGS, &BOOT_ON)
    }

    fn tag() -> &'static str {
        LEGEND_TAGS[0]
    }

    fn read_state(scene: &Scene) -> LegendState {
        let mut out = LegendState::idle();
        for (i, slot) in out.rows.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, LEGEND_TAGS[i]);
        }
        out
    }

    fn view(state: LegendState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-rescale-toggle (R1381 §5.38 rescale to visible)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        toggle_group::apply_key(scene, focused, key, &LEGEND_TAGS)
    }

    fn fmt_state_log(state: &LegendState) -> String {
        state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (s, on))| format!("{i}={}{}", s.as_name(), if *on { "+" } else { "-" }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl WidgetA11y for RescaleToggleView {
    /// **The chart's own answer** (R1722), derived from the same
    /// `LegendInteraction::Toggle` that made the entries focusable — so the
    /// roster a screen reader walks is the roster the row drew.
    fn access_node(state: &LegendState, focused: Option<&str>) -> Vec<AccessNode> {
        let postures = (0..N).fold(LegendPostures::at_rest(), |acc, i| {
            acc.under(i, &state.rows[i].0)
        });
        chart(state.visibility()).legend_access_nodes(
            CHART_RECT,
            &ChartStyle::default(),
            &postures,
            focused,
        )
    }
}

impl WidgetView for RescaleToggleView {
    type Renderer = HelloRescaleToggleRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<RescaleToggleView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn state_with(on: [bool; N]) -> LegendState {
        let mut s = LegendState::idle();
        for (i, slot) in s.rows.iter_mut().enumerate() {
            slot.1 = on[i];
        }
        s
    }

    fn render(on: [bool; N]) -> Scene {
        Owner::new().run(|| view(state_with(on), &Frame::new()))
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

    /// The vertical pixel extent of the `errors` series (series 2) polyline.
    fn errors_extent(scene: &Scene) -> u32 {
        let Some(Scene::Path(p)) = find(scene, "chart.series.2") else {
            panic!("errors series polyline present")
        };
        p.rect.h
    }

    #[test]
    fn hiding_the_dominant_series_rescales_and_grows_the_small_one() {
        // All visible: errors is a sliver pinned under `total`. Hide `total`
        // (and `cache`): the y-axis rescales to `errors`, so its polyline spans
        // much more of the plot.
        let all = errors_extent(&render([true, true, true]));
        let total_hidden = errors_extent(&render([false, true, true]));
        let big_two_hidden = errors_extent(&render([false, false, true]));
        assert!(
            total_hidden > all,
            "hiding `total` rescales up the errors extent (all={all}, total_hidden={total_hidden})"
        );
        assert!(
            big_two_hidden > total_hidden * 2,
            "hiding `total` + `cache` rescales errors to fill the plot \
             (total_hidden={total_hidden}, big_two_hidden={big_two_hidden})"
        );
    }

    #[test]
    fn a_hidden_series_still_draws_no_polyline() {
        let scene = render([false, true, true]);
        assert!(
            find(&scene, "chart.series.0").is_none(),
            "hidden `total` draws no line"
        );
        assert!(
            find(&scene, "chart.series.1").is_some(),
            "`cache` still drawn"
        );
        assert!(
            find(&scene, "chart.series.2").is_some(),
            "`errors` still drawn"
        );
    }

    #[test]
    fn each_legend_entry_is_a_focusable_tagged_hit_region() {
        let scene = render([true, true, true]);
        for tag in LEGEND_TAGS {
            let Some(Scene::Container(entry)) = find(&scene, tag) else {
                panic!("legend entry {tag} is a focusable container")
            };
            assert!(
                entry.layout.focusable,
                "entry {tag} is a Tab / click target"
            );
        }
    }

    #[test]
    fn a11y_exposes_one_pressed_button_per_series() {
        let nodes = RescaleToggleView::access_node(&state_with([true, false, true]), None);
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(buttons, N, "one aria-pressed button per legend entry");
    }
}
