//! `hello-cross-filter` — R1384 click a bar in one widget to **filter** another.
//!
//! The forcing consumer for [`pinion_chart::BarChart::select`] +
//! [`pinion_chart::BarChart::selectable`]. A bar chart of per-category totals is
//! the SELECTOR; a line chart of per-category timelines is the DEPENDENT view.
//! Clicking a bar's column toggles that category into the active filter set, and
//! the timeline re-derives to show only the selected categories — a selection in
//! one widget reshapes another. This is the dashboard interaction the chart
//! family was building toward: with line / bar / donut / scatter / treemap all
//! shipped (R1354-R1382), cross-filtering is what turns a set of charts into a
//! dashboard.
//!
//! The selection state is a further consumer of the
//! [`pinion_core::widgets::toggle_group`] substrate (with `hello-segmented-multi`
//! R733, `hello-filter-chip` R753, `hello-frame-profiler`, `hello-series-toggle`
//! R1379, and `hello-legend-toggle` R1380 — the 6th): N independent
//! [`ToggleExternal`](pinion_core::widgets::toggle::ToggleExternal)s, each its
//! own Tab stop under a WAI-ARIA `group`, lowering to `button[aria-pressed]`,
//! sharing the keyboard model / introspect reader / boot seed / AccessKit tree
//! verbatim. The one new thing over R1380 is WHERE the toggle's hit surface
//! comes from — the BAR chart's bars ([`selectable`]) — and, crucially, that the
//! toggle drives a DIFFERENT widget than the one it lives on (the cross-filter).
//! An empty selection is "no filter": every bar full, every timeline shown (the
//! crossfilter convention that no selection = all data).
//!
//! [`selectable`]: pinion_chart::BarChart::selectable

use pinion_a11y::{AccessNode, ToggleSegment, WidgetA11y, toggle_button_group_nodes};
use pinion_chart::{Bar, BarChart, ChartStyle, DataPoint, LineChart, Series};
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
vello_renderer_impl!(HelloCrossFilterRenderer, HelloCrossFilterRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 480;
const THEME_TAG: &str = "app";

/// Category count (bars, timelines, and toggles all share these indices).
const N: usize = 4;

/// Per-category dispatch + Tab-stop tags. These are BOTH the tags the bar
/// chart's clickable regions carry (via [`BarChart::selectable`]) AND the tags
/// the `ToggleExternal`s bind to — the caller owns the namespace, the chart just
/// applies it, so a click on bar `i` routes straight to toggle `i`. `&'static
/// str` because each is a scene-derived §5.39 Tab stop.
const CAT_TAGS: [&str; N] = ["cat_0", "cat_1", "cat_2", "cat_3"];

/// The WAI-ARIA §3.6 `group` label for the AccessKit toggle-button group.
const GROUP_TAG: &str = "categories";

/// Category names — the single source for the bar label, the line-series name
/// (and its legend entry), and the AccessKit `button` name.
const LABELS: [&str; N] = ["alpha", "beta", "gamma", "delta"];

/// Boot with NO category selected — an empty filter, so the bars are all full
/// and every timeline is shown (crossfilter's "no selection = all data").
const BOOT_ON: [bool; N] = [false; N];

/// Category colours, pinned explicitly (theme-independent mid-tones that read on
/// both schemes) so a bar and its timeline share ONE colour — selecting the
/// green bar highlights the green line.
const CAT_COLORS: [Color; N] = [
    Color::rgb(0x42, 0x85, 0xf4),
    Color::rgb(0x34, 0xa8, 0x53),
    Color::rgb(0xea, 0x43, 0x35),
    Color::rgb(0xfb, 0xbc, 0x05),
];

/// Per-category totals (the bar heights) — the aggregate the timelines sum to.
const TOTALS: [f64; N] = [3400.0, 1800.0, 2600.0, 900.0];

/// Window-absolute plot regions (the `pinion-chart` build-needs-its-rect
/// contract): the SELECTOR bar chart on top, the DEPENDENT line chart below.
const BAR_RECT: Rect = Rect::new(20, 40, WIN_W - 40, 168);
const LINE_RECT: Rect = Rect::new(20, 250, WIN_W - 40, WIN_H - 268);
const TITLE_FONT_PX: u32 = 17;
const SUBTITLE_FONT_PX: u32 = 13;

/// One deterministic 12-point timeline per category (ZERO-FLAKE: fixed data, no
/// randomness). Each category has a distinct shape so a reader can tell which
/// survives a filter.
#[allow(
    clippy::cast_precision_loss,
    reason = "bucket index (0..12) -> f64 x-coordinate is exact"
)]
fn series_points(cat: usize) -> Vec<DataPoint> {
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
        [
            300.0, 260.0, 240.0, 220.0, 210.0, 180.0, 160.0, 150.0, 130.0, 120.0, 100.0, 90.0,
        ],
    ];
    ys[cat]
        .iter()
        .enumerate()
        .map(|(k, &y)| DataPoint::new(k as f64, y))
        .collect()
}

/// The selector bars — per-category totals, coloured to match the timelines.
fn bars() -> Vec<Bar> {
    (0..N)
        .map(|i| Bar::new(LABELS[i], TOTALS[i]).with_color(CAT_COLORS[i]))
        .collect()
}

/// The dependent timelines; `visible[i]` gates series `i`'s polyline.
fn series_with(visible: [bool; N]) -> Vec<Series> {
    (0..N)
        .map(|i| {
            Series::new(LABELS[i], series_points(i))
                .with_color(CAT_COLORS[i])
                .with_visible(visible[i])
        })
        .collect()
}

/// The clickable-bar / toggle tags as owned strings (the chart takes
/// `Vec<String>`).
fn cat_tags() -> Vec<String> {
    CAT_TAGS.iter().map(|t| (*t).to_string()).collect()
}

/// The cross-filter rule (the one place the two widgets are linked): with a
/// NON-empty selection the timeline shows only the selected categories; with an
/// EMPTY selection it shows all of them (no filter = all data).
fn line_visibility(mask: [bool; N]) -> [bool; N] {
    let any = mask.iter().any(|&b| b);
    let mut v = [false; N];
    for (i, slot) in v.iter_mut().enumerate() {
        *slot = !any || mask[i];
    }
    v
}

/// The themed chart style, shared by both charts (the bar chart ignores
/// `legend`; the line chart draws one so a filtered series is named).
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        legend: true,
        label_size_px: 13,
        x_ticks: 7,
        y_ticks: 5,
        ..ChartStyle::default()
    }
}

/// Cached projection: one `(ToggleState, on)` pair per category.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct FilterState {
    rows: [(ToggleState, bool); N],
}

impl FilterState {
    fn idle() -> Self {
        Self {
            rows: [(ToggleState::Idle, false); N],
        }
    }

    /// The active-category mask (each row's `on`).
    fn mask(self) -> [bool; N] {
        let mut m = [false; N];
        for (i, r) in self.rows.iter().enumerate() {
            m[i] = r.1;
        }
        m
    }
}

/// A themed text label placed at an absolute window position.
fn text_at(content: &str, x: u32, y: u32, size: u32, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(
            content,
            Rect::default(),
            TextStyle::new().with_size_px(size).with_fg(fg),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(x, y)),
    )
}

/// view-fn (§6.3): pure sync `FilterState -> Scene`. The bar chart is
/// `selectable` (its columns carry `CAT_TAGS`) and `select`ed with the active
/// mask (selected bars full, others muted); the line chart is rebuilt with
/// [`line_visibility`] applied, so the SAME mask that emphasises the bars
/// filters the timeline.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: FilterState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mask = state.mask();
    let style = chart_style(&theme);
    let ink = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let title = text_at(
        "Category totals — click a bar to filter the timeline",
        20,
        14,
        TITLE_FONT_PX,
        ink,
    );
    let subtitle = text_at(
        "Timeline (only the selected categories; all when none selected)",
        20,
        224,
        SUBTITLE_FONT_PX,
        muted,
    );

    let bar_chart = BarChart::new(bars())
        .selectable(cat_tags())
        .select(mask.to_vec())
        .build(BAR_RECT, &style);

    let line_chart = LineChart::new(series_with(line_visibility(mask))).build(LINE_RECT, &style);

    Scene::Container(
        ContainerNode::new(vec![bar_chart, line_chart, title, subtitle])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

struct CrossFilterView;

impl WidgetCore for CrossFilterView {
    type State = FilterState;
    // Every state change flows through `apply_key` (keyboard) or the input
    // router's per-bar pointer dispatch — never the enum `keybinding` channel.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(toggle_group::boot_toggle(BOOT_ON[0]))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        toggle_group::extra_toggles(&CAT_TAGS, &BOOT_ON)
    }

    fn tag() -> &'static str {
        CAT_TAGS[0]
    }

    fn read_state(scene: &Scene) -> FilterState {
        let mut out = FilterState::idle();
        for (i, slot) in out.rows.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, CAT_TAGS[i]);
        }
        out
    }

    fn view(state: FilterState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-cross-filter (R1384 click a bar to filter the timeline)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        toggle_group::apply_key(scene, focused, key, &CAT_TAGS)
    }

    fn fmt_state_log(state: &FilterState) -> String {
        state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (s, on))| format!("{i}={}{}", s.as_name(), if *on { "+" } else { "-" }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl WidgetA11y for CrossFilterView {
    /// One [`AriaRole::Group`](pinion_a11y::AriaRole::Group) parent
    /// (`"Categories"`) plus one `button[aria-pressed]` per category, built by
    /// the shared [`toggle_button_group_nodes`] substrate. The button tags are
    /// `CAT_TAGS` — the same tags the bar chart's clickable regions carry, so
    /// AccessKit maps each button onto its painted bar column.
    fn access_node(state: &FilterState, focused: Option<&str>) -> Vec<AccessNode> {
        let segments: Vec<ToggleSegment<'_>> = (0..N)
            .map(|i| ToggleSegment {
                tag: CAT_TAGS[i],
                label: LABELS[i],
                state: state.rows[i].0,
                on: state.rows[i].1,
            })
            .collect();
        toggle_button_group_nodes(GROUP_TAG, "Categories", &segments, focused)
    }
}

impl WidgetView for CrossFilterView {
    type Renderer = HelloCrossFilterRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<CrossFilterView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn state_with(on: [bool; N]) -> FilterState {
        let mut s = FilterState::idle();
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

    fn bar_fill(scene: &Scene, i: usize) -> Color {
        let Some(Scene::Box(b)) = find(scene, &format!("chart.bar.{i}")) else {
            panic!("bar {i} is a box")
        };
        b.style.fill
    }

    #[test]
    fn no_selection_shows_every_timeline_and_no_muted_bar() {
        let scene = render(BOOT_ON);
        // Every timeline drawn (no filter).
        for i in 0..N {
            assert_eq!(
                count_prefix(&scene, &format!("chart.series.{i}")),
                1,
                "series {i} drawn when nothing is selected"
            );
        }
        // Every bar clickable + full-strength (a full bar equals itself at full
        // alpha — i.e. not muted).
        for i in 0..N {
            assert_eq!(
                bar_fill(&scene, i),
                CAT_COLORS[i],
                "bar {i} is full colour with no selection"
            );
            assert!(find(&scene, CAT_TAGS[i]).is_some(), "bar {i} is clickable");
        }
    }

    #[test]
    fn selecting_one_category_filters_the_timeline_to_it() {
        // Select ONLY beta (index 1): the timeline shows just beta's line.
        let scene = render([false, true, false, false]);
        assert_eq!(
            count_prefix(&scene, "chart.series.1"),
            1,
            "the selected category's timeline stays"
        );
        for i in [0usize, 2, 3] {
            assert_eq!(
                count_prefix(&scene, &format!("chart.series.{i}")),
                0,
                "an unselected category's timeline is filtered out ({i})"
            );
        }
    }

    #[test]
    fn selecting_a_bar_mutes_the_other_bars() {
        let full = render(BOOT_ON);
        let sel = render([false, true, false, false]);
        // beta (1) keeps full colour; the others mute.
        assert_eq!(
            bar_fill(&sel, 1),
            bar_fill(&full, 1),
            "the selected bar is full"
        );
        for i in [0usize, 2, 3] {
            assert_ne!(
                bar_fill(&sel, i),
                bar_fill(&full, i),
                "an unselected bar is muted ({i})"
            );
        }
    }

    #[test]
    fn a_multi_category_selection_keeps_all_selected_timelines() {
        // A filter is a SET: selecting alpha + gamma keeps both, drops the rest.
        let scene = render([true, false, true, false]);
        assert_eq!(count_prefix(&scene, "chart.series.0"), 1, "alpha kept");
        assert_eq!(count_prefix(&scene, "chart.series.2"), 1, "gamma kept");
        assert_eq!(
            count_prefix(&scene, "chart.series.1"),
            0,
            "beta filtered out"
        );
        assert_eq!(
            count_prefix(&scene, "chart.series.3"),
            0,
            "delta filtered out"
        );
    }

    #[test]
    fn every_bar_is_a_focusable_tagged_hit_region() {
        let scene = render(BOOT_ON);
        for tag in CAT_TAGS {
            let Some(Scene::Container(hit)) = find(&scene, tag) else {
                panic!("bar hit region {tag} is a focusable container")
            };
            assert!(
                hit.layout.focusable,
                "bar column {tag} is a Tab / click target"
            );
        }
    }

    #[test]
    fn each_toggle_tag_appears_exactly_once() {
        // Each `CAT_TAGS` tag is emitted ONCE — by the bar chart's clickable
        // region, not duplicated in the dependent timeline. The click surface
        // belongs to the selector; the timeline is the filtered view (the two
        // distinct widgets that make this a CROSS-filter, not a self-toggle).
        let scene = render(BOOT_ON);
        for tag in CAT_TAGS {
            assert_eq!(
                count_prefix(&scene, tag),
                1,
                "{tag} is one clickable bar column, not duplicated"
            );
        }
    }

    #[test]
    fn boot_selects_nothing() {
        assert_eq!(
            BOOT_ON, [false; N],
            "boot is an empty filter (all data shown)"
        );
    }

    #[test]
    fn a11y_exposes_one_pressed_button_per_category() {
        let nodes = CrossFilterView::access_node(&state_with([false, true, false, true]), None);
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(buttons, N, "one aria-pressed button per category");
    }
}
