//! `hello-legend-toggle` — R1380 §5.38 §5.39 click-the-chart's-**own**-legend
//! to show / hide a series.
//!
//! The forcing consumer for a line chart declaring
//! [`pinion_chart::LegendInteraction::Toggle`]: the
//! chart draws its own legend, and each entry is a focusable, hit-testable
//! region — a click / press anywhere on it toggles that series' visibility, and
//! a hidden series' entry renders muted (grey swatch + dimmed label). This is
//! the R1379 follow-up it named: `hello-series-toggle` had to draw a *separate*
//! chip bar because "the chart-owned legend has no hit geometry yet". R1380
//! gives the legend that geometry, so the toggle surface IS the legend.
//!
//! Nothing else changes: this is a further consumer of the
//! [`pinion_core::widgets::toggle_group`] interaction substrate (with
//! `hello-segmented-multi` R733, `hello-filter-chip` R753,
//! `hello-frame-profiler`, and `hello-series-toggle` R1379): N independent
//! [`ToggleExternal`](pinion_core::widgets::toggle::ToggleExternal)s, each its
//! own Tab stop under a WAI-ARIA `group`, lowering to `button[aria-pressed]`,
//! sharing the keyboard model / introspect reader / boot seed / AccessKit tree
//! verbatim. The only difference from R1379 is WHERE the toggle's hit surface
//! comes from: the chart's own legend, not a consumer-drawn chip.
//!
//! **R1722** — the legend entry tags are now DERIVED from the chart's tag prefix
//! rather than passed to it, so this file binds its externals to tags it cannot
//! get wrong, and its accessibility tree is the chart's own answer rather than a
//! toggle group rebuilt here.

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
vello_renderer_impl!(HelloLegendToggleRenderer, HelloLegendToggleRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";

/// Series (and legend-entry) count.
const N: usize = 3;

/// Per-entry dispatch + Tab-stop tags: the tags the chart's toggle legend
/// entries carry, which the `ToggleExternal`s bind to, so a click on entry `i`
/// routes straight to toggle `i`.
///
/// R1722 — these are **derived from the chart's tag prefix**, not chosen here.
/// Before that round a caller passed a `Vec<String>` the chart zipped against
/// its series, so a list of the wrong length silently truncated its own legend.
/// `&'static str` because each is a scene-derived §5.39 Tab stop.
const LEGEND_TAGS: [&str; N] = ["chart.legend.0", "chart.legend.1", "chart.legend.2"];

/// Series names — the single source for the chart series name, the legend-entry
/// label (the chart draws it), and the AccessKit `button` name.
const LABELS: [&str; N] = ["ingress", "egress", "errors"];

/// Every series visible at boot (a multi-select control has no "exactly one"
/// invariant; a full boot frame shows all three lines).
const BOOT_ON: [bool; N] = [true, true, true];

/// Series colours, pinned explicitly (theme-independent mid-tones that read on
/// both schemes) so each legend swatch matches its line exactly.
const SERIES_COLORS: [Color; N] = [
    Color::rgb(0x42, 0x85, 0xf4),
    Color::rgb(0x34, 0xa8, 0x53),
    Color::rgb(0xea, 0x43, 0x35),
];

/// Window-absolute plot region (the `pinion-chart` build-needs-its-rect
/// contract). The legend lives in the chart's own top margin — no bar below it.
const CHART_RECT: Rect = Rect::new(20, 44, WIN_W - 40, WIN_H - 64);
const TITLE_FONT_PX: u32 = 18;

/// The three sample series; `visible[i]` gates series `i`'s geometry.
#[allow(
    clippy::cast_precision_loss,
    reason = "bucket index (0..12) -> f64 x-coordinate is exact"
)]
fn series_with(visible: [bool; N]) -> Vec<Series> {
    let ys: [[f64; 12]; N] = [
        [
            820.0, 910.0, 1150.0, 1400.0, 1320.0, 1600.0, 2100.0, 2400.0, 2200.0, 1900.0, 2600.0,
            3100.0,
        ],
        [
            400.0, 520.0, 680.0, 900.0, 1100.0, 1250.0, 1400.0, 1300.0, 1500.0, 1700.0, 1650.0,
            1800.0,
        ],
        [
            120.0, 80.0, 200.0, 400.0, 150.0, 600.0, 900.0, 300.0, 250.0, 700.0, 450.0, 200.0,
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
    LineChart::new(series_with(visible)).with_legend(LegendInteraction::Toggle)
}

/// The themed chart style. `legend: true` and interactive — the legend is the
/// toggle surface.
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
/// the current visibility mask (a hidden series draws no polyline) AND an
/// interactive legend whose entries carry `LEGEND_TAGS` — the click surfaces.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: LegendState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Throughput — click a legend entry to hide its line",
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

struct LegendToggleView;

impl WidgetCore for LegendToggleView {
    type State = LegendState;
    // Every state change flows through `apply_key` (keyboard) or the input
    // router's per-entry pointer dispatch — never the enum `keybinding` channel.
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
        "pinion hello-legend-toggle (R1380 §5.38 interactive legend)"
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

impl WidgetA11y for LegendToggleView {
    /// **The chart's own answer** (R1722): one `group` parent plus one
    /// `button[aria-pressed]` per legend entry, derived from the same
    /// `LegendInteraction::Toggle` that made the entries focusable.
    ///
    /// This used to be hand-built here — a `ToggleSegment` per series, a group
    /// tag chosen by this file, and a roster that would have kept announcing
    /// three buttons in a pane too narrow to draw three. Asking the chart is
    /// what makes the announcement and the paint one derivation; the theme is
    /// irrelevant to the roster, so the default style seats the row.
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

impl WidgetView for LegendToggleView {
    type Renderer = HelloLegendToggleRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<LegendToggleView>();
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

    /// Render the view inside an `Owner` scope (`use_theme` needs one).
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

    fn count_prefix(scene: &Scene, prefix: &str) -> usize {
        let mut n = usize::from(scene.tag().is_some_and(|t| t.starts_with(prefix)));
        if let Scene::Container(c) = scene {
            for ch in &c.children {
                n += count_prefix(ch, prefix);
            }
        }
        n
    }

    #[test]
    fn all_series_visible_draws_three_polylines() {
        let scene = render([true, true, true]);
        for i in 0..N {
            assert_eq!(
                count_prefix(&scene, &format!("chart.series.{i}")),
                1,
                "series {i} draws its polyline when its legend entry is on"
            );
        }
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
    fn toggling_an_entry_off_hides_only_that_series() {
        let scene = render([true, false, true]);
        assert_eq!(
            count_prefix(&scene, "chart.series.1"),
            0,
            "hidden series 1 has no polyline"
        );
        assert_eq!(
            count_prefix(&scene, "chart.series.0"),
            1,
            "series 0 still drawn"
        );
        assert_eq!(
            count_prefix(&scene, "chart.series.2"),
            1,
            "series 2 still drawn"
        );
        // The entry stays in the legend regardless (it is the toggle back on).
        assert!(
            find(&scene, LEGEND_TAGS[1]).is_some(),
            "the hidden series keeps its legend entry"
        );
    }

    #[test]
    fn every_series_hidden_draws_no_polylines_but_keeps_entries() {
        let scene = render([false, false, false]);
        assert_eq!(
            count_prefix(&scene, "chart.series."),
            0,
            "no polylines when all hidden"
        );
        for tag in LEGEND_TAGS {
            assert!(
                find(&scene, tag).is_some(),
                "entry {tag} present (the way back)"
            );
        }
    }

    #[test]
    fn boot_reads_every_series_visible() {
        assert_eq!(BOOT_ON, [true, true, true], "boot shows all series");
    }

    #[test]
    fn a11y_exposes_one_pressed_button_per_series() {
        let nodes = LegendToggleView::access_node(&state_with([true, false, true]), None);
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(buttons, N, "one aria-pressed button per legend entry");
    }
}
