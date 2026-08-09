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

use pinion_a11y::{AccessNode, AriaRole, ToggleSegment, WidgetA11y, toggle_button_group_nodes};
use pinion_chart::{ChartStyle, DataPoint, Interpolation, LineChart, Series};
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
// R1625 — taller by one chip row: the interpolation chips sit below the
// legend, and at 360 the second row ran off the bottom of the window.
const WIN_H: u32 = 420;
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

/// R1625 — the two interpolation chips. `smooth` picks a curve over straight
/// segments; `safe` picks the curve that cannot invent a value.
const SMOOTH_TAG: &str = "smooth";
const SAFE_TAG: &str = "safe";
const OPTION_TAGS: [&str; 2] = [SMOOTH_TAG, SAFE_TAG];
const OPTION_BOOT: [bool; 2] = [false, false];
const OPTION_LABELS: [&str; 2] = ["smooth", "safe"];
const OPTION_GROUP_TAG: &str = "interpolation";
const CAPTION_TAG: &str = "caption";
const CAPTION_FONT_PX: u32 = 12;

/// Every toggle this binding owns, in one list.
///
/// The **census** reads this — "is every painted chip wired to an external" is
/// a question about all of them. The **keymap** deliberately does not: an
/// arrow key roves within a group, and these are two groups. `cfg(test)`
/// for that reason: it exists to state the invariant, not to drive input.
#[cfg(test)]
fn all_toggle_tags() -> Vec<&'static str> {
    TOGGLE_TAGS
        .iter()
        .chain(OPTION_TAGS.iter())
        .copied()
        .collect()
}

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
    /// R1625 — `[smooth, safe]`.
    options: [(ToggleState, bool); 2],
    focused: Option<usize>,
}

impl LegendState {
    fn idle() -> Self {
        Self {
            rows: [(ToggleState::Idle, false); N],
            options: [(ToggleState::Idle, false); 2],
            focused: None,
        }
    }

    /// R1625 — the interpolation the two option chips encode.
    ///
    /// Two bits rather than a three-way picker because they are two separate
    /// questions: *is the line curved*, and *may that curve draw a value the
    /// data never had*. The second is only meaningful when the first is on,
    /// and the caption says so rather than the chip disappearing.
    fn interpolation(&self) -> Interpolation {
        match (self.options[0].1, self.options[1].1) {
            (false, _) => Interpolation::Linear,
            (true, true) => Interpolation::Monotone,
            (true, false) => Interpolation::CatmullRom,
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

    let built =
        LineChart::new(series_with(state.visibility())).interpolation(state.interpolation());
    let chart = built.build(CHART_RECT, &chart_style(&theme));

    let caption = Scene::Text(
        TextNode::styled(
            interpolation_report(&state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CAPTION_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(CAPTION_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(20, 40)),
    );

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

    let options: Vec<Scene> = (0..2)
        .map(|i| option_chip(i, state.options[i].0, state.options[i].1, &theme))
        .collect();
    let option_row = Scene::Container(
        ContainerNode::new(options)
            .with_tag(OPTION_GROUP_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(CHIP_GAP)
                    .with_absolute_position(20, CHART_RECT.y + CHART_RECT.h + 60),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![chart, title, caption, legend, option_row])
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
    labelled_chip(
        TOGGLE_TAGS[index].to_string(),
        LABELS[index],
        state,
        on,
        SERIES_COLORS[index],
        theme,
    )
}

/// The one chip painter, shared by the legend row and R1625's interpolation
/// row so the two cannot drift in look or behaviour. `swatch_on` is the
/// colour the swatch takes when the chip is on.
fn labelled_chip(
    tag: String,
    text: &str,
    state: ToggleState,
    on: bool,
    swatch_on: Color,
    theme: &Theme,
) -> Scene {
    let ink = if on {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    // The swatch is the series colour when visible, a muted tone when hidden —
    // so an off chip reads as "this line is gone" at a glance.
    let swatch_color = if on {
        swatch_on
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
        text,
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
            .with_tag(tag)
            .with_style(style)
            .with_layout(
                chip::chip_layout(Size::px(CHIP_W, CHIP_HEIGHT), None).with_focusable(true),
            ),
    )
}

/// R1625 — what the current interpolation did to the data, READ OFF the
/// chart rather than restated.
///
/// One derivation feeding both the painted caption and the live region, so a
/// screen reader and a sighted reader cannot be told different things.
fn interpolation_report(state: &LegendState) -> String {
    let built =
        LineChart::new(series_with(state.visibility())).interpolation(state.interpolation());
    let invented = built.overshoot();
    let kind = state.interpolation().name();
    if invented.is_empty() {
        format!("{kind} — no value drawn that the data does not contain")
    } else {
        let worst = invented
            .iter()
            .map(|(_, o)| o.beyond)
            .fold(0.0f32, f32::max);
        format!(
            "{kind} — {} segment(s) leave their samples, worst by {worst:.2}",
            invented.len(),
        )
    }
}

/// R1625 — an interpolation chip. Same painter as the legend chip; only the
/// label differs, so the two rows cannot drift apart in look or behaviour.
fn option_chip(index: usize, state: ToggleState, on: bool, theme: &Theme) -> Scene {
    labelled_chip(
        OPTION_TAGS[index].to_string(),
        OPTION_LABELS[index],
        state,
        on,
        theme.resolve(ColorRole::Accent),
        theme,
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
        let mut out = toggle_group::extra_toggles(&TOGGLE_TAGS, &BOOT_ON);
        // `toggles`, not `extra_toggles`: no tag here is the primary
        // external, and the form that drops the first one silently left the
        // `smooth` chip wired to nothing.
        out.extend(toggle_group::toggles(&OPTION_TAGS, &OPTION_BOOT));
        out
    }

    fn tag() -> &'static str {
        TOGGLE_TAGS[0]
    }

    fn read_state(scene: &Scene) -> LegendState {
        let mut out = LegendState::idle();
        for (i, slot) in out.rows.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, TOGGLE_TAGS[i]);
        }
        for (i, slot) in out.options.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, OPTION_TAGS[i]);
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
        // R1625 — TWO groups, asked in turn, rather than one flat list.
        // Merging them made ArrowRight wrap out of the legend and into the
        // interpolation chips, which the R1379 demo caught: an arrow key
        // roves within a group, and these are two groups because they answer
        // two questions.
        toggle_group::apply_key(scene, focused, key, &TOGGLE_TAGS)
            || toggle_group::apply_key(scene, focused, key, &OPTION_TAGS)
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
    /// One [`AriaRole::Group`] parent (`"Series"`)
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
        let mut nodes = toggle_button_group_nodes(GROUP_TAG, "Series", &segments, focused);

        // R1625 — the interpolation chips announce themselves too. A
        // focusable container with no AccessNode is a keyboard stop a screen
        // reader cannot name (R1581's class), and adding a control row is
        // exactly when that happens.
        let options: Vec<ToggleSegment<'_>> = (0..2)
            .map(|i| ToggleSegment {
                tag: OPTION_TAGS[i],
                label: OPTION_LABELS[i],
                state: state.options[i].0,
                on: state.options[i].1,
            })
            .collect();
        nodes.extend(toggle_button_group_nodes(
            OPTION_GROUP_TAG,
            "Interpolation",
            &options,
            focused,
        ));

        // The report is a live region, so a reader hears that the curve left
        // the data rather than having to look.
        nodes.push(
            AccessNode::new(CAPTION_TAG, AriaRole::Status).with_name(interpolation_report(state)),
        );
        nodes
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
        // R1625 — one button per series AND one per interpolation chip. The
        // count is derived from the two lists rather than written down, which
        // is what made adding a control row a one-line edit here instead of a
        // silent under-count.
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(
            buttons,
            N + OPTION_TAGS.len(),
            "one aria-pressed button per toggle",
        );
        for tag in all_toggle_tags() {
            assert!(
                nodes.iter().any(|n| n.tag == tag),
                "{tag} announces itself — a focusable chip with no node is a \
                 keyboard stop a screen reader cannot name",
            );
        }
    }

    /// R1625 — **every chip this binding paints is wired to an external.**
    ///
    /// FOUND BY A COUNTERFACTUAL. The round shipped a `smooth` chip that was
    /// painted, focusable and announced to assistive technology, and backed
    /// by nothing: `toggle_group::extra_toggles` drops its first tag because
    /// in a binding's FIRST group that tag is already the primary external,
    /// and calling it on a second group silently produces a dead control.
    /// The demo caught it and no unit test could, because nothing here had
    /// ever compared the tags it paints with the tags it registers.
    ///
    /// This is that comparison, and it is the invariant rather than the
    /// instance: a chip whose tag has no external is a control wired to
    /// nothing, whatever produced it.
    #[test]
    fn r1625_every_chip_tag_has_an_external_behind_it() {
        let mut registered: Vec<String> = vec![<SeriesToggleView as WidgetCore>::tag().to_string()];
        registered.extend(
            SeriesToggleView::create_extra_externals()
                .into_iter()
                .map(|e| e.tag.into_owned()),
        );
        for tag in all_toggle_tags() {
            assert!(
                registered.iter().any(|r| r == tag),
                "{tag} is painted and focusable but has no external: {registered:?}",
            );
        }
        assert_eq!(
            registered.len(),
            all_toggle_tags().len(),
            "and nothing is registered that is never painted",
        );
    }

    /// R1625 — the two chips encode three interpolations, and the caption
    /// reports what the chosen one did to the data.
    #[test]
    fn r1625_the_chips_pick_the_curve_and_the_caption_reports_it() {
        let with = |smooth: bool, safe: bool| {
            let mut st = state_with([true, true, true]);
            st.options[0] = (ToggleState::Idle, smooth);
            st.options[1] = (ToggleState::Idle, safe);
            st
        };
        assert_eq!(with(false, false).interpolation(), Interpolation::Linear);
        assert_eq!(with(false, true).interpolation(), Interpolation::Linear);
        assert_eq!(with(true, false).interpolation(), Interpolation::CatmullRom);
        assert_eq!(with(true, true).interpolation(), Interpolation::Monotone);

        // Straight and monotone invent nothing, and the caption says so.
        for st in [with(false, false), with(true, true)] {
            let said = interpolation_report(&st);
            assert!(said.contains("no value drawn"), "{said}");
        }
        // The painted caption and the live region are one derivation.
        let smooth = with(true, false);
        let scene = Owner::new().run(|| view(smooth, &Frame::new()));
        let painted = text_with_tag(&scene, CAPTION_TAG).expect("the caption is painted");
        assert_eq!(painted, interpolation_report(&smooth));
        let nodes = SeriesToggleView::access_node(&smooth, None);
        let live = nodes
            .iter()
            .find(|n| n.tag == CAPTION_TAG)
            .expect("the caption is a live region");
        assert_eq!(live.name.as_deref(), Some(painted.as_str()));
    }

    fn text_with_tag(scene: &Scene, tag: &str) -> Option<String> {
        match scene {
            Scene::Text(t) if t.tag.as_deref() == Some(tag) => Some(t.content.clone()),
            Scene::Container(c) => c.children.iter().find_map(|ch| text_with_tag(ch, tag)),
            _ => None,
        }
    }
}
