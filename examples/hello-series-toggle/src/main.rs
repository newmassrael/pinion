//! `hello-series-toggle` — R1379 §5.38 §5.40 series-**visibility** toggles over
//! a multi-series line chart.
//!
//! The forcing consumer for the [`pinion_chart::Series::visible`] model: a bar
//! of independently-toggleable chips, one per series, that show / hide that
//! series' geometry. Toggling a chip off drops only its own polyline — the
//! palette indices, the other series, and the auto-domain are unchanged, so a
//! series that is toggled back on lands on exactly the same grid (no rescale).
//!
//! This is a further consumer of the
//! [`pinion_core::widgets::toggle_group`] interaction substrate (with
//! `hello-segmented-multi` R733, `hello-filter-chip` R753, and
//! `hello-frame-profiler`): N independent
//! [`ToggleExternal`](pinion_core::widgets::toggle::ToggleExternal)s, each its
//! own Tab stop under a WAI-ARIA `group`, lowering to `button[aria-pressed]`,
//! sharing the keyboard model / introspect reader / boot seed / AccessKit tree
//! verbatim. The chips double as the chart legend — each carries a swatch in its
//! series colour + the series name — so the chart itself draws **no** internal
//! legend (`ChartStyle { legend: false }`); a click-the-chart's-own-legend
//! toggle is a separate follow-up (the chart-owned legend has no hit geometry
//! yet).

use pinion_a11y::{AccessNode, ToggleSegment, WidgetA11y, toggle_button_group_nodes};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::external::External;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::chip::{self, CHIP_HEIGHT};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSeriesToggleRenderer, HelloSeriesToggleRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";

/// Series (and chip) count.
const N: usize = 3;

/// Per-chip dispatch + Tab-stop tags. `&'static str` (not `format!`) because
/// each is `.with_focusable(true)` — the scene-derived §5.39 Tab stops — and
/// the input router hit-tests a click on chip `i` straight to its toggle.
const TOGGLE_TAGS: [&str; N] = ["series_0", "series_1", "series_2"];

/// The WAI-ARIA §3.6 `group` container tag carried by the chip row.
const GROUP_TAG: &str = "series_legend";

/// Series names — the single source of truth for the chart series name, the
/// chip label, and the AccessKit `button` name.
const LABELS: [&str; N] = ["ingress", "egress", "errors"];

/// Every series visible at boot (a multi-select control has no "exactly one"
/// invariant; a full boot frame shows all three lines).
const BOOT_ON: [bool; N] = [true, true, true];

/// Series colours, pinned explicitly (theme-independent mid-tones that read on
/// both schemes) so each chip's swatch matches its line exactly — the chips ARE
/// the legend.
const SERIES_COLORS: [Color; N] = [
    Color::rgb(0x42, 0x85, 0xf4),
    Color::rgb(0x34, 0xa8, 0x53),
    Color::rgb(0xea, 0x43, 0x35),
];

/// Window-absolute plot region (the `pinion-chart` build-needs-its-rect
/// contract). The chip row sits below it.
const CHART_RECT: Rect = Rect::new(20, 44, WIN_W - 40, 240);
const CHIP_W: u32 = 136;
const CHIP_GAP: u32 = 10;
const SWATCH: u32 = 12;
const TITLE_FONT_PX: u32 = 18;
const LABEL_FONT_PX: u32 = 15;

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

/// The themed chart style. `legend: false` — the toggle chips ARE the legend.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        legend: false,
        x_ticks: 7,
        y_ticks: 5,
        ..ChartStyle::default()
    }
}

/// Cached projection: one `(ToggleState, on)` pair per series chip plus the
/// §5.40 AT-side focus index.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct LegendState {
    rows: [(ToggleState, bool); N],
    focused: Option<usize>,
}

impl LegendState {
    fn idle() -> Self {
        Self {
            rows: [(ToggleState::Idle, false); N],
            focused: None,
        }
    }

    /// The per-series visibility mask (each chip's `on`).
    fn visibility(&self) -> [bool; N] {
        let mut v = [false; N];
        for (i, r) in self.rows.iter().enumerate() {
            v[i] = r.1;
        }
        v
    }
}

/// view-fn (§6.3): pure sync `LegendState -> Scene`. The chart is rebuilt with
/// the current visibility mask (a hidden series draws no polyline); the chip row
/// below it is the interactive legend.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: LegendState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Throughput — toggle a series to hide its line",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(20, 14)),
    );

    let chart =
        LineChart::new(series_with(state.visibility())).build(CHART_RECT, &chart_style(&theme));

    let chips: Vec<Scene> = (0..N)
        .map(|i| chip(i, state.rows[i].0, state.rows[i].1, &theme))
        .collect();
    let legend = Scene::Container(
        ContainerNode::new(chips).with_tag(GROUP_TAG).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(CHIP_GAP)
                .with_absolute_position(20, CHART_RECT.y + CHART_RECT.h + 20),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![chart, title, legend])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

/// One legend chip: a series-colour swatch + the series name. Toggled ON reads
/// as a filled tonal pill; toggled OFF drops to a muted, `Outline`-bordered pill
/// (the "hidden" affordance). Tagged `series_{index}` so a click routes straight
/// to that chip's `ToggleExternal`.
fn chip(index: usize, state: ToggleState, on: bool, theme: &Theme) -> Scene {
    let ink = if on {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    // The swatch is the series colour when visible, a muted tone when hidden —
    // so an off chip reads as "this line is gone" at a glance.
    let swatch_color = if on {
        SERIES_COLORS[index]
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    let swatch = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(swatch_color).with_corner_radius(3),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH, SWATCH))),
    );
    let label = Scene::Text(TextNode::styled(
        LABELS[index],
        Rect::default(),
        TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(ink),
    ));
    let fill_base = if on {
        theme.resolve(ColorRole::SurfaceContainerHigh)
    } else {
        Color::rgba(0, 0, 0, 0)
    };
    // R1446 — the M3 selected-chip-drops-its-outline rule, lifted at its 3rd
    // consumer. The base fill above stays local: a series toggle's tonal fill
    // deliberately differs from a filter chip's Accent.
    let border = chip::selection_border(theme, on);
    let style = chip::chip_style(fill_base, border, state, theme);
    Scene::Container(
        ContainerNode::new(vec![swatch, label])
            .with_tag(TOGGLE_TAGS[index])
            .with_style(style)
            .with_layout(
                chip::chip_layout(Size::px(CHIP_W, CHIP_HEIGHT), None).with_focusable(true),
            ),
    )
}

struct SeriesToggleView;

impl WidgetCore for SeriesToggleView {
    type State = LegendState;
    // Every state change flows through `apply_key` (keyboard) or the input
    // router's per-chip pointer dispatch — never the enum `keybinding` channel.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(toggle_group::boot_toggle(BOOT_ON[0]))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        toggle_group::extra_toggles(&TOGGLE_TAGS, &BOOT_ON)
    }

    fn tag() -> &'static str {
        TOGGLE_TAGS[0]
    }

    fn read_state(scene: &Scene) -> LegendState {
        let mut out = LegendState::idle();
        for (i, slot) in out.rows.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, TOGGLE_TAGS[i]);
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
        "pinion hello-series-toggle (R1379 §5.38 series visibility)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        toggle_group::apply_key(scene, focused, key, &TOGGLE_TAGS)
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

impl WidgetA11y for SeriesToggleView {
    /// One [`AriaRole::Group`](pinion_a11y::AriaRole::Group) parent (`"Series"`)
    /// plus one `button[aria-pressed]` per series chip, built by the shared
    /// [`toggle_button_group_nodes`] substrate.
    fn access_node(state: &LegendState, focused: Option<&str>) -> Vec<AccessNode> {
        let segments: Vec<ToggleSegment<'_>> = (0..N)
            .map(|i| ToggleSegment {
                tag: TOGGLE_TAGS[i],
                label: LABELS[i],
                state: state.rows[i].0,
                on: state.rows[i].1,
            })
            .collect();
        toggle_button_group_nodes(GROUP_TAG, "Series", &segments, focused)
    }
}

impl WidgetView for SeriesToggleView {
    type Renderer = HelloSeriesToggleRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<SeriesToggleView>();
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
                "series {i} draws its polyline when its chip is on"
            );
        }
    }

    #[test]
    fn toggling_a_chip_off_hides_only_that_series() {
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
        // The chip stays in the legend regardless (it is the toggle back on).
        assert!(
            count_prefix(&scene, TOGGLE_TAGS[1]) >= 1,
            "the hidden series keeps its chip"
        );
    }

    #[test]
    fn every_series_hidden_draws_no_polylines() {
        let scene = render([false, false, false]);
        assert_eq!(
            count_prefix(&scene, "chart.series."),
            0,
            "no polylines when all hidden"
        );
        // All three chips still present (the only way to bring a series back).
        for tag in TOGGLE_TAGS {
            assert!(count_prefix(&scene, tag) >= 1, "chip {tag} present");
        }
    }

    #[test]
    fn boot_reads_every_series_visible() {
        assert_eq!(BOOT_ON, [true, true, true], "boot shows all series");
    }

    #[test]
    fn a11y_exposes_one_pressed_button_per_series() {
        let nodes = SeriesToggleView::access_node(&state_with([true, false, true]), None);
        // group + N buttons.
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(buttons, N, "one aria-pressed button per series");
    }
}
