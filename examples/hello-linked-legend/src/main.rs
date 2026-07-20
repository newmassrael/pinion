//! `hello-linked-legend` — R1392 §5.38 §5.39 a scatter's **own** interactive
//! legend cross-filters a companion LINE chart: an ARBITRARY chart-to-chart
//! cross-filter.
//!
//! The forcing consumer for [`pinion_chart::ScatterChart::interactive_legend`]:
//! the SCATTER draws its own legend, and each entry is a focusable, hit-testable
//! region (the R1380 chip mechanism, now lifted to
//! [`pinion_chart`] and shared by the line and scatter charts). Clicking an entry
//! toggles that series' visibility, and the ONE toggle state drives BOTH the
//! scatter (its points vanish) AND a companion LINE chart (its polyline vanishes)
//! — so a selection in one widget reshapes a DIFFERENT chart type. R1384 wired
//! the first cross-filter (a bar click -> a line), R1391 the numeric brush leg
//! (a range -> a scatter); this one proves the SELECTOR can be an arbitrary chart
//! type too (a scatter legend), the "arbitrary chart-to-chart" leg.
//!
//! This is a further consumer of the
//! [`pinion_core::widgets::toggle_group`] interaction substrate (with
//! `hello-segmented-multi` R733, `hello-filter-chip` R753, `hello-series-toggle`
//! R1379, `hello-legend-toggle` R1380, and `hello-cross-filter` R1384): N
//! independent [`ToggleExternal`](pinion_core::widgets::toggle::ToggleExternal)s,
//! each its own Tab stop under a WAI-ARIA `group`, sharing the keyboard model /
//! introspect reader / boot seed / AccessKit tree verbatim. The scatter's legend
//! entries ARE the toggle hit surfaces; the line is a pure target (no legend of
//! its own), so there is no duplicate-tag collision — one selector, one
//! different-type target.

use pinion_a11y::{AccessNode, ToggleSegment, WidgetA11y, toggle_button_group_nodes};
use pinion_chart::{ChartStyle, DataPoint, LineChart, ScatterChart, Series};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloLinkedLegendRenderer, HelloLinkedLegendRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 560;
const THEME_TAG: &str = "app";

/// Series (and legend-entry) count.
const N: usize = 3;

/// Per-entry dispatch + Tab-stop tags. These are BOTH the tags the SCATTER's
/// interactive legend entries carry AND the tags the `ToggleExternal`s bind to,
/// so a click on entry `i` routes straight to toggle `i`. The line is a pure
/// target (no legend), so nothing else claims these tags.
const LEGEND_TAGS: [&str; N] = ["legend_0", "legend_1", "legend_2"];

/// The WAI-ARIA §3.6 `group` label for the AccessKit toggle-button group.
const GROUP_TAG: &str = "linked_legend";

/// Series names — the single source for the series name, the legend-entry label,
/// and the AccessKit `button` name.
const LABELS: [&str; N] = ["ingress", "egress", "errors"];

/// Every series visible at boot.
const BOOT_ON: [bool; N] = [true, true, true];

/// Series colours, pinned so each legend swatch matches its marks + line exactly.
const SERIES_COLORS: [Color; N] = [
    Color::rgb(0x42, 0x85, 0xf4),
    Color::rgb(0x34, 0xa8, 0x53),
    Color::rgb(0xea, 0x43, 0x35),
];

const TITLE_FONT_PX: u32 = 18;

/// The SCATTER (top) is the selector — its interactive legend sits in its top
/// margin. The LINE (bottom) is the target.
const SCATTER_RECT: Rect = Rect::new(20, 44, WIN_W - 40, 220);
const LINE_RECT: Rect = Rect::new(20, 300, WIN_W - 40, 240);

/// The three sample series; `visible[i]` gates series `i`'s geometry in BOTH
/// charts.
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

/// The interactive-legend tags as owned strings (the chart takes `Vec<String>`).
fn legend_tags() -> Vec<String> {
    LEGEND_TAGS.iter().map(|t| (*t).to_string()).collect()
}

/// The themed style shared by both charts. `legend: true` so the SCATTER draws
/// its interactive legend; the line overrides it off (it is a pure target).
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
struct LinkedState {
    rows: [(ToggleState, bool); N],
}

impl LinkedState {
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

/// view-fn (§6.3): pure sync `LinkedState -> Scene`. Both charts are rebuilt from
/// the ONE visibility mask; the scatter carries the interactive legend (the click
/// surfaces), the line reflects the same mask as a pure target.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: LinkedState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mask = state.visibility();
    let style = chart_style(&theme);
    // The line is a pure target — no legend of its own (avoids a duplicate-tag
    // second selector), so its style drops the legend band.
    let line_style = ChartStyle {
        legend: false,
        ..style.clone()
    };

    let title = Scene::Text(
        TextNode::styled(
            "Click a scatter legend entry — it hides the series in BOTH charts",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(20, 14)),
    );

    // SELECTOR: the scatter with its own interactive legend (tags = LEGEND_TAGS).
    // A distinct tag prefix keeps its `scatter.*` nodes apart from the line's, so
    // the two charts share no ambient tag (bg / axes / grid).
    let scatter = ScatterChart::new(series_with(mask))
        .interactive_legend(legend_tags())
        .with_tag_prefix("scatter")
        .build(SCATTER_RECT, &style);

    // TARGET: the line chart of the SAME series, filtered by the same mask.
    let line = LineChart::new(series_with(mask))
        .with_tag_prefix("line")
        .build(LINE_RECT, &line_style);

    Scene::Container(
        ContainerNode::new(vec![scatter, line, title])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct LinkedLegendView;

impl WidgetCore for LinkedLegendView {
    type State = LinkedState;
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

    fn read_state(scene: &Scene) -> LinkedState {
        let mut out = LinkedState::idle();
        for (i, slot) in out.rows.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, LEGEND_TAGS[i]);
        }
        out
    }

    fn view(state: LinkedState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-linked-legend (R1392 arbitrary chart-to-chart cross-filter)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        toggle_group::apply_key(scene, focused, key, &LEGEND_TAGS)
    }

    fn fmt_state_log(state: &LinkedState) -> String {
        state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (s, on))| format!("{i}={}{}", s.as_name(), if *on { "+" } else { "-" }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl WidgetA11y for LinkedLegendView {
    /// One [`AriaRole::Group`](pinion_a11y::AriaRole::Group) parent (`"Series"`)
    /// plus one `button[aria-pressed]` per legend entry, built by the shared
    /// [`toggle_button_group_nodes`] substrate. The button tags are `LEGEND_TAGS`
    /// — the same tags the scatter's focusable legend entries carry, so AccessKit
    /// maps each button onto its painted entry.
    fn access_node(state: &LinkedState, focused: Option<&str>) -> Vec<AccessNode> {
        let segments: Vec<ToggleSegment<'_>> = (0..N)
            .map(|i| ToggleSegment {
                tag: LEGEND_TAGS[i],
                label: LABELS[i],
                state: state.rows[i].0,
                on: state.rows[i].1,
            })
            .collect();
        toggle_button_group_nodes(GROUP_TAG, "Series", &segments, focused)
    }
}

impl WidgetView for LinkedLegendView {
    type Renderer = HelloLinkedLegendRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<LinkedLegendView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn state_with(on: [bool; N]) -> LinkedState {
        let mut s = LinkedState::idle();
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
    fn all_visible_draws_both_charts_fully() {
        let scene = render([true, true, true]);
        // Three scatter series (points) + three line series (polylines).
        for i in 0..N {
            assert!(
                count_prefix(&scene, &format!("scatter.point.{i}.")) > 0,
                "scatter series {i} draws points"
            );
            assert_eq!(
                count_prefix(&scene, &format!("line.series.{i}")),
                1,
                "line series {i} draws its polyline"
            );
        }
    }

    #[test]
    fn each_scatter_legend_entry_is_a_focusable_tagged_hit_region() {
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
    fn hiding_a_series_filters_both_the_scatter_and_the_line() {
        // The cross-filter essence: ONE toggle off removes the series from BOTH
        // charts — a selection in the scatter's legend reshapes the LINE too.
        let scene = render([true, false, true]);
        assert_eq!(
            count_prefix(&scene, "scatter.point.1."),
            0,
            "hidden series 1 has no scatter points"
        );
        assert_eq!(
            count_prefix(&scene, "line.series.1"),
            0,
            "hidden series 1 has no line polyline (cross-filtered)"
        );
        // The other two survive in both charts.
        assert!(
            count_prefix(&scene, "scatter.point.0.") > 0,
            "series 0 points stay"
        );
        assert_eq!(
            count_prefix(&scene, "line.series.0"),
            1,
            "series 0 line stays"
        );
        // The legend entry stays (the toggle back on).
        assert!(
            find(&scene, LEGEND_TAGS[1]).is_some(),
            "the hidden series keeps its legend entry"
        );
    }

    #[test]
    fn the_line_target_has_no_second_interactive_legend() {
        // Only the scatter's legend is a selector; the line is a pure target, so
        // no LEGEND_TAG appears twice (no duplicate-tag second selector).
        let scene = render([true, true, true]);
        for tag in LEGEND_TAGS {
            assert_eq!(
                count_prefix(&scene, tag),
                1,
                "exactly one focusable entry carries {tag}"
            );
        }
    }

    #[test]
    fn a11y_exposes_one_pressed_button_per_series() {
        let nodes = LinkedLegendView::access_node(&state_with([true, false, true]), None);
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(buttons, N, "one aria-pressed button per legend entry");
    }
}
