//! `hello-boxplot` — R1553 §5.38 a datum can be a **distribution**.
//!
//! The forcing consumer for [`pinion_chart::Distribution`] and
//! [`pinion_chart::BoxPlotChart`] (Qt's `QBoxPlotSeries`). Every datum this
//! crate could plot until R1553 resolved to one position; a distribution
//! occupies a span of the value axis and carries interior landmarks, so one
//! datum draws a box, a median, two whiskers, two caps and a mark per
//! outlier.
//!
//! ## The dataset is SAMPLES, and that is the point
//!
//! Five endpoints hand over their raw per-request latencies. Nothing here
//! computes a quartile — [`Distribution::from_samples`] does, under a method
//! the reader picks at runtime. Qt cannot be used this way: `QBoxSet` takes
//! five doubles and `QtCharts` computes none of them (its own box-plot
//! example ships a `findMedian()` helper *in the example*), so a Qt consumer
//! summarises upstream and the definition it used is lost.
//!
//! ## What the three method chips show
//!
//! `/search` was hit six times. At `n = 6` the three standard quartile
//! definitions genuinely disagree — Tukey's hinges put its upper hinge at
//! `26`, Hyndman & Fan type 7 at `24.25`, type 6 at `29.75` — and the
//! disagreement is not cosmetic: the `41 ms` sample sits **inside** Tukey's
//! `1.5 * IQR` fence and **outside** type 7's, so switching the chip turns a
//! whisker end into an outlier. Same data, same fence rule, different
//! answer. That is why the method is part of
//! [`pinion_chart::DistributionSource`] rather than a caller's comment.
//!
//! ## The notch is available because `n` survived
//!
//! The notch chip draws the McGill-Tukey-Larsen waist
//! (`median +- 1.58 * IQR / sqrt(n)`): where two notches do not overlap, the
//! medians differ significantly at roughly 95%. `QBoxSet` carries no sample
//! count, so Qt could not offer this even as a paint option.
//!
//! ## The zero samples are data, not sentinels
//!
//! `/health` answered two requests from cache, and a millisecond-resolution
//! timer recorded them as `0.0`. They are far below the lower fence, so they
//! are outliers — and on a **log** axis they have no pixel at all. They draw
//! no mark and are reported by
//! [`BoxPlotChart::off_scale`](pinion_chart::BoxPlotChart::off_scale); the
//! caption is that report. Switch back to linear and the caption says so:
//! being off-scale is a property of the axis, not of the data.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the whole schema as tagged data —
//! `chart.box.{i}`, `chart.median.{i}`, `chart.whisker.{i}.lo` / `.hi`,
//! `chart.cap.{i}.lo` / `.hi`, `chart.outlier.{i}.{j}`. No pixels are
//! sampled (§2 #1 / §2 #7). See `tools/demos/r1553_distribution_datum.py`.

use pinion_a11y::{
    AccessFocus, AccessNode, AriaRole, RadioCell, ToggleSegment, WidgetA11y,
    radiogroup_radio_nodes, toggle_button_group_nodes,
};
use pinion_chart::{BoxPlotChart, ChartStyle, Distribution, LandmarkKind, QuantileMethod};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::interaction::InteractionState;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::chip::{CHIP_HEIGHT, chip_layout, chip_style, selection_border};
use pinion_widget_paint::radio_composite as rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloBoxPlotRenderer, HelloBoxPlotRendererError);

const WIN_W: u32 = 820;
const WIN_H: u32 = 500;
const THEME_TAG: &str = "app";

/// The quantile-method radio group (PRIMARY external). Its cells are
/// `method#<i>`, which the R51.42 `'#'`-split routes back to this one
/// coordinator.
const METHOD_TAG: &str = "method";

/// The notch and log-axis toggles (extras) — independent switches, each its
/// own Tab stop, unlike the radio group's single roving stop.
const NOTCH_TAG: &str = "notch";
const LOG_TAG: &str = "logscale";

/// The WAI-ARIA `group` label parent for the two option toggles.
const OPTIONS_GROUP_TAG: &str = "options";

/// The caption's tag — the SSOT for the painted text and for the demo.
const CAPTION_TAG: &str = "caption";

/// The three quantile definitions, in the order their chips appear.
const METHODS: [QuantileMethod; 3] = [
    QuantileMethod::Tukey,
    QuantileMethod::Linear,
    QuantileMethod::Exclusive,
];

/// Chip labels — the standard names, so a reader can look the definition up.
const METHOD_LABELS: [&str; 3] = ["Tukey hinges", "Linear (HF-7)", "Exclusive (HF-6)"];

/// Boot: Tukey's hinges (what the box plot was defined on), no notch, linear
/// axis — so the first interaction with any chip is a visible change.
const BOOT_METHOD: usize = 0;
const BOOT_NOTCH: bool = false;
const BOOT_LOG: bool = false;

const TITLE_FONT_PX: u32 = 17;
const CAPTION_FONT_PX: u32 = 12;
const CHIP_FONT_PX: u32 = 13;
const CHIP_W: u32 = 132;

/// Window-absolute plot region. The chart must be handed its final geometry
/// before layout runs (see the `pinion-chart` coordinate contract).
const CHART_RECT: Rect = Rect::new(16, 96, WIN_W - 32, WIN_H - 172);

/// The endpoint whose six samples make the three methods disagree — named
/// rather than hunted for, because the caption and the tests point at it.
const SMALL_N: usize = 2;

/// The endpoint whose cached responses were timed as `0.0` — the samples a
/// logarithmic axis cannot place.
const ZEROED: usize = 0;

/// Raw per-request latencies in milliseconds, one slice per endpoint.
///
/// Nothing here is a quartile. That is the round: the summary is derived
/// from these, by a named definition, at paint time.
const ENDPOINTS: [(&str, &[f64]); 5] = [
    (
        // Two cache hits a millisecond-resolution timer recorded as zero.
        "/health",
        &[
            0.0, 0.0, 0.82, 0.86, 0.88, 0.91, 0.93, 0.95, 0.97, 0.99, 1.01, 1.03, 1.05, 1.08, 1.11,
            1.14, 1.18, 1.22, 1.28, 1.35, 1.41, 1.55,
        ],
    ),
    (
        "/login",
        &[
            8.2, 9.1, 9.8, 10.4, 11.2, 12.0, 12.6, 13.3, 14.1, 15.0, 15.8, 16.9, 18.2, 19.5, 21.0,
            23.4, 26.1, 29.8, 34.5, 48.0,
        ],
    ),
    // Six requests. Small enough that the quartile definition decides whether
    // the 41 ms sample is a whisker end or an outlier.
    ("/search", &[12.0, 14.0, 15.0, 19.0, 26.0, 41.0]),
    (
        "/report",
        &[
            44.0, 47.0, 49.0, 52.0, 55.0, 58.0, 60.0, 62.0, 65.0, 68.0, 70.0, 73.0, 75.0, 78.0,
            80.0, 84.0, 88.0, 92.0, 610.0, 940.0, 1500.0,
        ],
    ),
    (
        "/export",
        &[
            118.0, 132.0, 145.0, 158.0, 166.0, 174.0, 185.0, 196.0, 208.0, 221.0, 240.0, 262.0,
            285.0, 310.0, 344.0, 392.0,
        ],
    ),
];

/// The five distributions summarised under `method`.
///
/// The ONE derivation the chart, the caption and the tests all read, so a
/// caption that disagreed with the plot would be a bug in the crate rather
/// than in a second copy of the arithmetic here.
fn summarise(method: QuantileMethod) -> Vec<Distribution> {
    ENDPOINTS
        .iter()
        .map(|(name, samples)| {
            Distribution::from_samples(*name, samples, method)
                .expect("every endpoint has finite samples")
        })
        .collect()
}

/// The chart for one set of options — the ONE place the method, the notch and
/// the axis kind are applied.
fn chart_for(state: &Options) -> BoxPlotChart {
    let chart = BoxPlotChart::new(summarise(state.method())).notched(state.notch);
    if state.log { chart.y_log() } else { chart }
}

/// The themed chart style.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        label_size_px: 12,
        y_ticks: 6,
        ..ChartStyle::default()
    }
}

/// The caption: what this method decided, and what this axis could not draw.
///
/// Both clauses are read back off the built chart rather than restated, so
/// neither can drift from the picture.
fn caption(state: &Options) -> String {
    let chart = chart_for(state);
    let small = &chart.distributions()[SMALL_N];
    let head = format!(
        "{} \u{2014} {} upper quartile {:.2}, {} outlier(s)",
        small.label(),
        state.method().name(),
        small.q3(),
        small.outliers().len(),
    );

    let off = chart.off_scale();
    if off.is_empty() {
        return format!(
            "{head}. Linear axis \u{2014} every landmark is plotted, and the \
             sub-millisecond endpoints are pressed onto the baseline."
        );
    }
    let outliers = off
        .iter()
        .filter(|o| matches!(o.landmark, LandmarkKind::Outlier(_)))
        .count();
    format!(
        "{head}. Log axis \u{2014} {} landmark(s) not plotted, {outliers} of them \
         outliers of {}: a log axis has no zero, so they are reported, not \
         drawn on the domain floor.",
        off.len(),
        ENDPOINTS[ZEROED].0,
    )
}

/// One chip. Generic over the interaction state because the method row and
/// the option row are owned by two different selection models
/// (`RadioState` / `ToggleState`) but wear one skin.
///
/// `focusable` is where they part: a radio group is ONE Tab stop with a
/// roving active descendant, so its chips are hit targets but not stops —
/// the strip container carries the stop. Independent toggles are each a stop,
/// because Tab is how you reach the second without touching the first.
fn chip<S: InteractionState + Copy>(
    tag: String,
    label: &str,
    selected: bool,
    focusable: bool,
    width: u32,
    state: S,
    theme: &Theme,
) -> Scene {
    let base = if selected {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::rgba(0, 0, 0, 0)
    };
    let ink = if selected {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let text = Scene::Text(TextNode::styled(
        label.to_owned(),
        Rect::default(),
        TextStyle::new().with_size_px(CHIP_FONT_PX).with_fg(ink),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(tag)
            .with_style(chip_style(
                base,
                selection_border(theme, selected),
                state,
                theme,
            ))
            .with_layout(chip_layout(Size::px(width, CHIP_HEIGHT), None).with_focusable(focusable)),
    )
}

/// The method radio strip: three chips inside one focusable container, which
/// is the group's single Tab stop.
fn method_row(state: &Options, theme: &Theme) -> Scene {
    let chips: Vec<Scene> = (0..METHODS.len())
        .map(|i| {
            chip(
                format!("{METHOD_TAG}#{i}"),
                METHOD_LABELS[i],
                state.method_rows[i].1,
                false,
                CHIP_W,
                state.method_rows[i].0,
                theme,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(chips)
            .with_tag(METHOD_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8)
                    .with_focusable(true)
                    .with_absolute_position(16, 48)
                    .with_size(Size::px(CHIP_W * 3 + 16, CHIP_HEIGHT)),
            ),
    )
}

/// The two option toggles, each its own Tab stop.
fn option_row(state: &Options, theme: &Theme) -> Scene {
    let notch = chip(
        NOTCH_TAG.to_string(),
        "notch",
        state.notch,
        true,
        96,
        state.notch_row.0,
        theme,
    );
    let log = chip(
        LOG_TAG.to_string(),
        "log axis",
        state.log,
        true,
        104,
        state.log_row.0,
        theme,
    );
    Scene::Container(
        ContainerNode::new(vec![notch, log])
            .with_tag(OPTIONS_GROUP_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8)
                    .with_absolute_position(WIN_W - 224, 48)
                    .with_size(Size::px(208, CHIP_HEIGHT)),
            ),
    )
}

/// view-fn (§6.3): pure sync `Options -> Scene`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the `&Frame` shape is the WidgetCore::view signature this wraps"
)]
fn view(state: Options, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Request latency by endpoint (ms) \u{2014} raw samples, summarised here",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 18)),
    );

    let chart = chart_for(&state).build(CHART_RECT, &chart_style(&theme));

    let caption = Scene::Text(
        TextNode::styled(
            caption(&state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CAPTION_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(CAPTION_TAG.to_string())
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(18, WIN_H - 64)
                .with_size(Size::px(WIN_W - 36, 56)),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![
            chart,
            title,
            method_row(&state, &theme),
            option_row(&state, &theme),
            caption,
        ])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_size(Size::px(WIN_W, WIN_H)),
        ),
    )
}

/// Which quantile definition is selected, and whether each option is on.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Options {
    /// Per-method `(interaction state, selected)`.
    method_rows: [(RadioState, bool); 3],
    /// The selected method index (the radio group's 1-of-N invariant).
    method_selected: usize,
    /// The AT-side roving descendant of the method group.
    method_focused: Option<usize>,
    notch_row: (ToggleState, bool),
    log_row: (ToggleState, bool),
    notch: bool,
    log: bool,
}

impl Options {
    fn idle() -> Self {
        Self {
            method_rows: [(RadioState::Idle, false); 3],
            method_selected: BOOT_METHOD,
            method_focused: None,
            notch_row: (ToggleState::Idle, BOOT_NOTCH),
            log_row: (ToggleState::Idle, BOOT_LOG),
            notch: BOOT_NOTCH,
            log: BOOT_LOG,
        }
    }

    /// The selected quantile definition. An out-of-range index (which the
    /// group cannot produce) falls back to the boot method rather than
    /// panicking in a view-fn.
    fn method(&self) -> QuantileMethod {
        METHODS
            .get(self.method_selected)
            .copied()
            .unwrap_or(METHODS[BOOT_METHOD])
    }
}

struct BoxPlotView;

impl WidgetCore for BoxPlotView {
    type State = Options;
    // Every change arrives through `apply_key` or the input router's per-chip
    // pointer dispatch — never the enum keybinding channel.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        use pinion_core::widgets::radio::RadioEvent;
        let mut group = RadioGroupExternal::new(METHODS.len());
        group.send(BOOT_METHOD, RadioEvent::PointerEnter);
        group.send(BOOT_METHOD, RadioEvent::PointerDown);
        group.send(BOOT_METHOD, RadioEvent::PointerUp);
        group.send(BOOT_METHOD, RadioEvent::PointerLeave);
        Box::new(group)
    }

    /// The two option toggles. Built by hand rather than through
    /// `toggle_group::extra_toggles`, which skips index 0 on the assumption
    /// that the first toggle is the primary external — here the primary is
    /// the method radio group.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                NOTCH_TAG,
                Box::new(toggle_group::boot_toggle(BOOT_NOTCH)) as Box<dyn External>,
            ),
            ExtraExternal::new(
                LOG_TAG,
                Box::new(toggle_group::boot_toggle(BOOT_LOG)) as Box<dyn External>,
            ),
        ]
    }

    fn tag() -> &'static str {
        METHOD_TAG
    }

    fn read_state(scene: &Scene) -> Options {
        let mut out = Options::idle();
        if let Some(node) = scene.find_external_with_tag(METHOD_TAG)
            && let Some(intro) = node.handle.introspect()
        {
            rc::read_rows(intro, &mut out.method_rows);
            out.method_focused = rc::focused_index(intro);
            out.method_selected = rc::selected_index(intro).unwrap_or(BOOT_METHOD);
        }
        out.notch_row = toggle_group::read_toggle(scene, NOTCH_TAG);
        out.log_row = toggle_group::read_toggle(scene, LOG_TAG);
        out.notch = out.notch_row.1;
        out.log = out.log_row.1;
        out
    }

    fn view(state: Options, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-boxplot (R1553 §5.38 a datum can be a distribution)"
    }

    /// Two keymaps, one per control. `toggle_group::apply_key` returns
    /// `false` unless one of the option tags owns focus, and the method
    /// branch returns `false` unless [`METHOD_TAG`] does, so exactly one can
    /// consume a key.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if toggle_group::apply_key(scene, focused, key, &[NOTCH_TAG, LOG_TAG]) {
            return true;
        }
        if focused != Some(METHOD_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(METHOD_TAG) else {
            return false;
        };
        let Some(idx) = resolve_method_target(node.handle.introspect(), key) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        rc::drive_activate(intro, idx);
        true
    }

    fn fmt_state_log(state: &Options) -> String {
        format!(
            "{}{}{}",
            state.method().name(),
            if state.notch { " notch" } else { "" },
            if state.log { " log" } else { " linear" }
        )
    }
}

/// The roving-cursor keymap of the method group.
fn resolve_method_target(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    let n = METHODS.len();
    match key {
        "ArrowRight" | "ArrowDown" => Some(rc::step(intro, 1, n)),
        "ArrowLeft" | "ArrowUp" => Some(rc::step(intro, -1, n)),
        "Home" => Some(0),
        "End" => Some(n - 1),
        _ => None,
    }
}

impl WidgetA11y for BoxPlotView {
    /// The method group as a WAI-ARIA `radiogroup`, the options as a `group`
    /// of `button[aria-pressed]`, and the caption as a live region — so the
    /// off-scale report and the quartile the method chose are HEARD, not only
    /// seen. Qt's charts implement no accessibility interface at all.
    fn access_node(state: &Options, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(METHOD_TAG);
        let active = rc::active_index(&state.method_rows, state.method_focused);
        let tags: Vec<String> = (0..METHODS.len())
            .map(|i| format!("{METHOD_TAG}#{i}"))
            .collect();
        let cells: Vec<RadioCell<'_>> = (0..METHODS.len())
            .map(|i| RadioCell {
                tag: &tags[i],
                label: Some(METHOD_LABELS[i]),
                state: state.method_rows[i].0,
                selected: state.method_rows[i].1,
                focused: group_focused && i == active,
            })
            .collect();
        let mut nodes = radiogroup_radio_nodes(METHOD_TAG, "Quantile definition", &cells);

        let segments = [
            ToggleSegment {
                tag: NOTCH_TAG,
                label: "notch",
                state: state.notch_row.0,
                on: state.notch,
            },
            ToggleSegment {
                tag: LOG_TAG,
                label: "log axis",
                state: state.log_row.0,
                on: state.log,
            },
        ];
        nodes.extend(toggle_button_group_nodes(
            OPTIONS_GROUP_TAG,
            "Plot options",
            &segments,
            focused,
        ));

        nodes.push(AccessNode::new(CAPTION_TAG, AriaRole::Status).with_name(caption(state)));
        nodes
    }

    /// R1518 §5.40 — the method group is one Tab stop with a roving cursor,
    /// so name the radio that cursor addresses as the
    /// `aria-activedescendant`.
    fn access_focus_target(state: &Options, focused: Option<&str>) -> Option<AccessFocus> {
        rc::composite_focus_target(
            METHOD_TAG,
            focused,
            rc::active_index(&state.method_rows, state.method_focused),
        )
    }
}

impl WidgetView for BoxPlotView {
    type Renderer = HelloBoxPlotRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<BoxPlotView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn options(method: usize, notch: bool, log: bool) -> Options {
        let mut out = Options::idle();
        out.method_selected = method;
        out.method_rows[method].1 = true;
        out.notch = notch;
        out.notch_row = (ToggleState::Idle, notch);
        out.log = log;
        out.log_row = (ToggleState::Idle, log);
        out
    }

    fn render(method: usize, notch: bool, log: bool) -> Scene {
        Owner::new().run(|| view(options(method, notch, log), &Frame::new()))
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

    /// ★ The round, read off the derived data: at `n = 6` the three standard
    /// quartile definitions disagree, and the disagreement decides whether
    /// the 41 ms sample is a whisker end or an outlier. Qt computes none of
    /// the three, so a `QBoxSet` cannot record which one it was built with.
    #[test]
    fn r1553_the_method_decides_whether_the_sample_is_an_outlier() {
        let q3_and_outliers = |m| {
            let d = &summarise(m)[SMALL_N];
            (d.q3(), d.outliers().len())
        };
        let (tukey_q3, tukey_out) = q3_and_outliers(QuantileMethod::Tukey);
        let (linear_q3, linear_out) = q3_and_outliers(QuantileMethod::Linear);
        let (excl_q3, excl_out) = q3_and_outliers(QuantileMethod::Exclusive);

        assert!((tukey_q3 - 26.0).abs() < 1e-9, "tukey hinge {tukey_q3}");
        assert!((linear_q3 - 24.25).abs() < 1e-9, "type 7 {linear_q3}");
        assert!((excl_q3 - 29.75).abs() < 1e-9, "type 6 {excl_q3}");

        assert_eq!(tukey_out, 0, "41 ms is inside Tukey's fence");
        assert_eq!(linear_out, 1, "...and outside type 7's");
        assert_eq!(excl_out, 0, "...and inside type 6's wider one");
    }

    /// ★ That difference reaches the PAINTED scene: switching the method
    /// makes an outlier node appear where there was none.
    #[test]
    fn r1553_the_method_change_reaches_the_painted_marks() {
        let tukey = render(0, false, false);
        let linear = render(1, false, false);
        let tag = format!("chart.outlier.{SMALL_N}.0");
        assert!(find(&tukey, &tag).is_none(), "no outlier under Tukey");
        assert!(find(&linear, &tag).is_some(), "one appears under type 7");
    }

    /// ★ One datum emits a whole schema. Five endpoints, five of each mark.
    #[test]
    fn r1553_every_endpoint_draws_the_whole_schema() {
        let scene = render(0, false, false);
        assert_eq!(count_prefix(&scene, "chart.box."), ENDPOINTS.len());
        assert_eq!(count_prefix(&scene, "chart.median."), ENDPOINTS.len());
        assert_eq!(count_prefix(&scene, "chart.whisker."), ENDPOINTS.len() * 2);
        assert_eq!(count_prefix(&scene, "chart.cap."), ENDPOINTS.len() * 2);
        // /health's two zero cache hits, /login's 48 ms tail, and /report's
        // three far samples — six marks Qt's five-slot `QBoxSet` has no room
        // for at all.
        assert_eq!(count_prefix(&scene, "chart.outlier."), 6);
    }

    /// ★ The notch is a waist on the box path, and it is only drawn when
    /// asked. The counterfactual is the same chart with the chip off.
    #[test]
    fn r1553_the_notch_widens_the_box_path() {
        let plain = render(0, false, false);
        let notched = render(0, true, false);
        let Some(Scene::Path(p)) = find(&plain, "chart.box.0") else {
            panic!("the box is a path")
        };
        let Some(Scene::Path(n)) = find(&notched, "chart.box.0") else {
            panic!("the box is a path")
        };
        assert_eq!(p.commands.len(), 5, "four corners and a close");
        assert_eq!(n.commands.len(), 11, "ten outline points and a close");
    }

    /// ★ The zero cache hits have no pixel on a log axis: their marks vanish
    /// and the caption reports them. On a linear axis they are ordinary, so
    /// being off-scale is a property of the AXIS.
    #[test]
    fn r1553_the_log_axis_reports_what_it_cannot_place() {
        let linear = render(0, false, false);
        assert!(find(&linear, &format!("chart.outlier.{ZEROED}.0")).is_some());
        let caption = text_of(&linear, CAPTION_TAG);
        assert!(caption.contains("Linear axis"), "{caption}");
        assert!(!caption.contains("not plotted"), "{caption}");

        let log = render(0, false, true);
        assert!(
            find(&log, &format!("chart.outlier.{ZEROED}.0")).is_none(),
            "a zero has no pixel on a log axis"
        );
        let caption = text_of(&log, CAPTION_TAG);
        assert!(caption.contains("2 landmark(s) not plotted"), "{caption}");
        assert!(caption.contains("2 of them outliers"), "{caption}");

        // And the report names them precisely.
        let off = chart_for(&options(0, false, true)).off_scale();
        assert!(
            off.iter()
                .all(|o| o.distribution == ZEROED && matches!(o.landmark, LandmarkKind::Outlier(_))),
            "{off:?}"
        );
        // Nothing here is a summary landmark — the box itself still draws.
        assert!(
            !off.iter()
                .any(|o| matches!(o.landmark, LandmarkKind::Summary(_))),
            "every one of the five landmarks is placeable: {off:?}"
        );
        assert!(find(&log, &format!("chart.box.{ZEROED}")).is_some());
    }

    /// ★ The log axis reveals what the linear one flattens. Measured as the
    /// pixel gap between two endpoints' medians: `/health` at ~1 ms and
    /// `/login` at ~15 ms are fifteen-fold apart, which a linear axis
    /// spanning a 1500 ms outlier renders as a few pixels of nothing.
    ///
    /// The box HEIGHT is deliberately not the measurement — `/health`'s
    /// interquartile range is narrow in ratio terms too (0.91..1.18), so a
    /// log axis barely widens it. What the axis fixes is the separation
    /// between distributions, not the spread within one.
    #[test]
    fn r1553_the_log_axis_separates_what_the_linear_one_flattens() {
        let median_gap = |log: bool| {
            let scene = render(0, false, log);
            let y_of = |i: usize| {
                let Some(Scene::Path(p)) = find(&scene, &format!("chart.median.{i}")) else {
                    panic!("median {i} is present")
                };
                i64::from(p.rect.y)
            };
            (y_of(ZEROED) - y_of(1)).abs()
        };
        let linear = median_gap(false);
        let log = median_gap(true);
        assert!(
            linear < 10,
            "linear: /health and /login sit {linear}px apart"
        );
        assert!(
            log > linear * 8,
            "log: they sit {log}px apart, more than 8x the linear {linear}px"
        );
    }

    /// ★ The method group is one Tab stop with three hit targets; the two
    /// options are their own stops.
    #[test]
    fn r1553_the_controls_carry_the_right_tab_stops() {
        let scene = render(0, false, false);
        let Some(Scene::Container(group)) = find(&scene, METHOD_TAG) else {
            panic!("the method group is a container")
        };
        assert!(group.layout.focusable, "the group owns the stop");
        for i in 0..METHODS.len() {
            let Some(Scene::Container(cell)) = find(&scene, &format!("{METHOD_TAG}#{i}")) else {
                panic!("cell {i} is a container")
            };
            assert!(!cell.layout.focusable, "a radio cell is not its own stop");
        }
        for tag in [NOTCH_TAG, LOG_TAG] {
            let Some(Scene::Container(c)) = find(&scene, tag) else {
                panic!("{tag} is a container")
            };
            assert!(c.layout.focusable, "{tag} is its own stop");
        }
    }

    /// ★ The summary and its provenance reach assistive technology: the
    /// caption is a live region, the methods a radiogroup, the options
    /// pressed buttons.
    #[test]
    fn r1553_a11y_exposes_the_method_the_options_and_the_report() {
        let nodes = BoxPlotView::access_node(&options(1, false, true), None);
        let radios = nodes
            .iter()
            .filter(|n| matches!(n.role, AriaRole::RadioButton))
            .count();
        assert_eq!(radios, METHODS.len(), "one radio per quantile definition");
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, AriaRole::Button))
            .count();
        assert_eq!(buttons, 2, "one aria-pressed button per option");
        let status = nodes
            .iter()
            .find(|n| matches!(n.role, AriaRole::Status))
            .expect("the caption is a live region");
        let name = status.name.clone().unwrap_or_default();
        assert!(name.contains("linear"), "names the method: {name}");
        assert!(name.contains("not plotted"), "names the report: {name}");
    }
}
