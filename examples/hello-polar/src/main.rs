//! `hello-polar` — R1568 §5.38 the crate's first **non-cartesian coordinate
//! system**, and the axis that is PERIODIC.
//!
//! The forcing consumer for [`pinion_chart::AngularScale`] and
//! [`pinion_chart::PolarChart`] (Qt's `QPolarChart`).
//!
//! ## What a circle can do that a line cannot
//!
//! On a line, `0` and `360` are two places. On a compass they are the same
//! place, and everything this round adds follows from that one fact:
//!
//! * The 372-degree gust below is a **bearing of 12**, so it draws — and is
//!   reported as *wrapped*, which is a thing only an axis that placed it can
//!   say. Qt's angular axis is an ordinary `QValueAxis`, so there it is
//!   simply out of range and nothing is drawn.
//! * The trace **closes on itself**: the segment from the last sample back to
//!   the first is the axis's doing. A Qt radar gets it by appending the first
//!   point a second time, which puts a duplicate in the data the model does
//!   not contain.
//! * The tick at 360 is the tick at 0, so only one is drawn.
//!
//! ## The three chips are the three declarations Qt hard-codes
//!
//! `QPolarChart` fixes the origin at 12 o'clock, fixes the winding clockwise,
//! and is always a full circle. Here each is a declaration:
//!
//! * **form** — the numeric compass (a wind rose over `0 .. 360`) or the
//!   named radar (`0 .. n` over five categories, the nominal angular axis).
//! * **sector** — half a turn instead of a full one. The loop opens, the seam
//!   tick comes back, and the wrapped sample becomes genuinely off-scale:
//!   wrapping a sector would fold data onto the gap it deliberately leaves.
//! * **counter-clockwise** — the mathematical convention, which
//!   `QPolarChart` cannot draw at all.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the whole schema as tagged data —
//! `chart.ring.{k}`, `chart.spoke.{k}`, `chart.rim`, `chart.series.{i}` (with
//! its command list, which is how the closed loop is read), `chart.area.{i}`,
//! `chart.point.{i}.{j}`, `chart.label.a.{k}` / `chart.label.r.{k}`. No
//! pixels are sampled (§2 #1 / §2 #7). See
//! `tools/demos/r1568_the_angular_axis_is_periodic.py`.

use pinion_a11y::{
    AccessFocus, AccessNode, AriaRole, RadioCell, ToggleSegment, WidgetA11y,
    radiogroup_radio_nodes, toggle_button_group_nodes,
};
use pinion_chart::{AngularScale, Categories, ChartStyle, DataPoint, PolarChart, Series, Winding};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::chip::{CHIP_HEIGHT, option_chip};
use pinion_widget_paint::radio_composite as rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloPolarRenderer, HelloPolarRendererError);

const WIN_W: u32 = 860;
const WIN_H: u32 = 560;
const THEME_TAG: &str = "app";

/// The form radio group (PRIMARY external). Its cells are `form#<i>`, which
/// the R51.42 `'#'`-split routes back to this one coordinator.
const FORM_TAG: &str = "form";

/// The sector and winding toggles (extras) — independent switches, each its
/// own Tab stop, unlike the radio group's single roving stop.
const SECTOR_TAG: &str = "sector";
const WINDING_TAG: &str = "winding";

/// The WAI-ARIA `group` label parent for the two option toggles.
const OPTIONS_GROUP_TAG: &str = "options";

/// The caption's tag — the SSOT for the painted text and for the demo.
const CAPTION_TAG: &str = "caption";

/// The two forms, in the order their chips appear.
const FORM_LABELS: [&str; 2] = ["Compass (0-360)", "Radar (named)"];
const FORM_COMPASS: usize = 0;
const FORM_RADAR: usize = 1;

/// Boot: the compass, a full turn, clockwise — Qt's own configuration, so the
/// first interaction with any chip is a visible change.
const BOOT_FORM: usize = FORM_COMPASS;
const BOOT_SECTOR: bool = false;
const BOOT_CCW: bool = false;

const TITLE_FONT_PX: u32 = 17;
const CAPTION_FONT_PX: u32 = 12;
const CHIP_W: u32 = 148;

/// Window-absolute plot region. The chart must be handed its final geometry
/// before layout runs (see the `pinion-chart` coordinate contract).
const CHART_RECT: Rect = Rect::new(16, 96, WIN_W - 32, WIN_H - 190);

/// The gust whose bearing lies OUTSIDE the compass period — a sensor that
/// counted past north rather than resetting. On a full turn it is a bearing
/// of 12 and draws; on a sector it is off-scale.
const WRAPPED_BEARING: f64 = 372.0;

/// Mean gust speed by bearing (m/s), from a mast that reports every 90
/// degrees — plus the one reading that ran past north.
const GUSTS: [(f64, f64); 5] = [
    (0.0, 4.2),
    (90.0, 9.1),
    (180.0, 2.4),
    (270.0, 6.8),
    (WRAPPED_BEARING, 5.5),
];

/// The radar's spokes.
const FACETS: [&str; 5] = ["speed", "range", "armour", "cost", "crew"];

/// Two designs scored on those five facets.
const SCORES: [(&str, [f64; 5]); 2] = [
    ("prototype", [8.0, 4.0, 6.0, 3.0, 7.0]),
    ("incumbent", [5.0, 7.0, 8.0, 6.0, 4.0]),
];

/// The compass series — one mast, five readings.
fn gust_series() -> Vec<Series> {
    vec![Series::new(
        "gusts",
        GUSTS.iter().map(|&(b, v)| DataPoint::new(b, v)).collect(),
    )]
}

/// The radar series — two designs over the five facets, `x` being the facet
/// index (the category-axis convention).
fn score_series() -> Vec<Series> {
    SCORES
        .iter()
        .map(|(name, row)| {
            Series::new(
                *name,
                row.iter()
                    .enumerate()
                    .map(|(i, v)| {
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "a five-element facet index is exact in f64"
                        )]
                        let slot = i as f64;
                        DataPoint::new(slot, *v)
                    })
                    .collect(),
            )
        })
        .collect()
}

/// The chart for one set of options — the ONE place the form, the sweep and
/// the winding are applied.
///
/// The two declarations ride `PolarChart`'s own builders rather than a
/// replacement axis, so the radar's `0 .. 5` period and its five labels
/// cannot be re-seated apart from each other.
fn chart_for(state: &Options) -> PolarChart {
    let chart = if state.form == FORM_RADAR {
        PolarChart::radar(score_series(), Categories::new(FACETS))
    } else {
        PolarChart::new(gust_series(), AngularScale::new((0.0, 360.0)))
    };
    let chart = if state.sector {
        chart.with_sweep(core::f32::consts::PI)
    } else {
        chart
    };
    if state.ccw {
        chart.with_winding(Winding::CounterClockwise)
    } else {
        chart
    }
}

/// The themed chart style.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        label_size_px: 12,
        y_ticks: 5,
        x_ticks: 8,
        ..ChartStyle::default()
    }
}

/// The caption: what this axis did with the reading that ran past north, and
/// whether the trace closes.
///
/// Every clause is read back off the built chart rather than restated, so
/// none can drift from the picture.
fn caption(state: &Options) -> String {
    let chart = chart_for(state);
    let closes = chart.angular().closes();
    let head = if closes {
        "Full turn \u{2014} the axis is PERIODIC, so the trace closes on itself \
         with no duplicated sample."
    } else {
        "Half turn \u{2014} a sector is not periodic, so the trace stays open \
         and both ends of the period are their own place."
    };
    let wrapped = chart.wrapped().len();
    let off = chart.off_scale().len();
    let tail = if state.form == FORM_RADAR {
        format!(
            "Five named spokes over a 0-{} period; every facet is a slot.",
            FACETS.len()
        )
    } else if wrapped > 0 {
        format!(
            "The {WRAPPED_BEARING:.0}\u{b0} reading is a bearing of \
             {:.0}\u{b0}: {wrapped} wrapped, {off} off-scale.",
            WRAPPED_BEARING - 360.0
        )
    } else {
        format!(
            "The {WRAPPED_BEARING:.0}\u{b0} reading is outside a half-turn \
             sector: {wrapped} wrapped, {off} off-scale."
        )
    };
    let winding = chart.angular().winding().name();
    format!("{head} {tail} Values increase {winding}.")
}

/// The form radio strip: two chips inside one focusable container, which is
/// the group's single Tab stop.
fn form_row(state: &Options, theme: &Theme) -> Scene {
    let chips: Vec<Scene> = (0..FORM_LABELS.len())
        .map(|i| {
            option_chip(
                format!("{FORM_TAG}#{i}"),
                FORM_LABELS[i],
                state.form_rows[i].1,
                false,
                CHIP_W,
                state.form_rows[i].0,
                theme,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(chips)
            .with_tag(FORM_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8)
                    .with_focusable(true)
                    .with_absolute_position(16, 48)
                    .with_size(Size::px(CHIP_W * 2 + 8, CHIP_HEIGHT)),
            ),
    )
}

/// The two option toggles, each its own Tab stop.
fn option_row(state: &Options, theme: &Theme) -> Scene {
    let sector = option_chip(
        SECTOR_TAG.to_string(),
        "half turn",
        state.sector,
        true,
        108,
        state.sector_row.0,
        theme,
    );
    let winding = option_chip(
        WINDING_TAG.to_string(),
        "counter-cw",
        state.ccw,
        true,
        112,
        state.winding_row.0,
        theme,
    );
    Scene::Container(
        ContainerNode::new(vec![sector, winding])
            .with_tag(OPTIONS_GROUP_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8)
                    .with_absolute_position(WIN_W - 240, 48)
                    .with_size(Size::px(228, CHIP_HEIGHT)),
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
            "Mean gust by bearing \u{2014} an axis where 0 and 360 are one place",
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
                .with_absolute_position(18, WIN_H - 76)
                .with_size(Size::px(WIN_W - 36, 68)),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![
            chart,
            title,
            form_row(&state, &theme),
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

/// Which form is selected, and whether each axis declaration is on.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Options {
    /// Per-form `(interaction state, selected)`.
    form_rows: [(RadioState, bool); 2],
    /// The selected form index (the radio group's 1-of-N invariant).
    form: usize,
    /// The AT-side roving descendant of the form group.
    form_focused: Option<usize>,
    sector_row: (ToggleState, bool),
    winding_row: (ToggleState, bool),
    sector: bool,
    ccw: bool,
}

impl Options {
    fn idle() -> Self {
        Self {
            form_rows: [(RadioState::Idle, false); 2],
            form: BOOT_FORM,
            form_focused: None,
            sector_row: (ToggleState::Idle, BOOT_SECTOR),
            winding_row: (ToggleState::Idle, BOOT_CCW),
            sector: BOOT_SECTOR,
            ccw: BOOT_CCW,
        }
    }
}

struct PolarView;

impl WidgetCore for PolarView {
    type State = Options;
    // Every change arrives through `apply_key` or the input router's per-chip
    // pointer dispatch — never the enum keybinding channel.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        use pinion_core::widgets::radio::RadioEvent;
        let mut group = RadioGroupExternal::new(FORM_LABELS.len());
        group.send(BOOT_FORM, RadioEvent::PointerEnter);
        group.send(BOOT_FORM, RadioEvent::PointerDown);
        group.send(BOOT_FORM, RadioEvent::PointerUp);
        group.send(BOOT_FORM, RadioEvent::PointerLeave);
        Box::new(group)
    }

    /// The two option toggles. Built by hand rather than through
    /// `toggle_group::extra_toggles`, which skips index 0 on the assumption
    /// that the first toggle is the primary external — here the primary is
    /// the form radio group.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                SECTOR_TAG,
                Box::new(toggle_group::boot_toggle(BOOT_SECTOR)) as Box<dyn External>,
            ),
            ExtraExternal::new(
                WINDING_TAG,
                Box::new(toggle_group::boot_toggle(BOOT_CCW)) as Box<dyn External>,
            ),
        ]
    }

    fn tag() -> &'static str {
        FORM_TAG
    }

    fn read_state(scene: &Scene) -> Options {
        let mut out = Options::idle();
        if let Some(node) = scene.find_external_with_tag(FORM_TAG)
            && let Some(intro) = node.handle.introspect()
        {
            rc::read_rows(intro, &mut out.form_rows);
            out.form_focused = rc::focused_index(intro);
            out.form = rc::selected_index(intro).unwrap_or(BOOT_FORM);
        }
        out.sector_row = toggle_group::read_toggle(scene, SECTOR_TAG);
        out.winding_row = toggle_group::read_toggle(scene, WINDING_TAG);
        out.sector = out.sector_row.1;
        out.ccw = out.winding_row.1;
        out
    }

    fn view(state: Options, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-polar (R1568 §5.38 the angular axis is periodic)"
    }

    /// Two keymaps, one per control. `toggle_group::apply_key` returns
    /// `false` unless one of the option tags owns focus, and the form branch
    /// returns `false` unless [`FORM_TAG`] does, so exactly one can consume a
    /// key.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if toggle_group::apply_key(scene, focused, key, &[SECTOR_TAG, WINDING_TAG]) {
            return true;
        }
        if focused != Some(FORM_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(FORM_TAG) else {
            return false;
        };
        let Some(idx) = resolve_form_target(node.handle.introspect(), key) else {
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
            FORM_LABELS[state.form.min(FORM_LABELS.len() - 1)],
            if state.sector { " sector" } else { " full" },
            if state.ccw { " ccw" } else { " cw" }
        )
    }
}

/// The roving-cursor keymap of the form group.
fn resolve_form_target(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    let n = FORM_LABELS.len();
    match key {
        "ArrowRight" | "ArrowDown" => Some(rc::step(intro, 1, n)),
        "ArrowLeft" | "ArrowUp" => Some(rc::step(intro, -1, n)),
        "Home" => Some(0),
        "End" => Some(n - 1),
        _ => None,
    }
}

impl WidgetA11y for PolarView {
    /// The form group as a WAI-ARIA `radiogroup`, the options as a `group` of
    /// `button[aria-pressed]`, and the caption as a live region — so what the
    /// axis did with the out-of-period reading is HEARD, not only seen. Qt's
    /// charts implement no accessibility interface at all.
    fn access_node(state: &Options, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(FORM_TAG);
        let active = rc::active_index(&state.form_rows, state.form_focused);
        let tags: Vec<String> = (0..FORM_LABELS.len())
            .map(|i| format!("{FORM_TAG}#{i}"))
            .collect();
        let cells: Vec<RadioCell<'_>> = (0..FORM_LABELS.len())
            .map(|i| RadioCell {
                tag: &tags[i],
                label: Some(FORM_LABELS[i]),
                state: state.form_rows[i].0,
                selected: state.form_rows[i].1,
                focused: group_focused && i == active,
            })
            .collect();
        let mut nodes = radiogroup_radio_nodes(FORM_TAG, "Polar form", &cells);

        let segments = [
            ToggleSegment {
                tag: SECTOR_TAG,
                label: "half turn",
                state: state.sector_row.0,
                on: state.sector,
            },
            ToggleSegment {
                tag: WINDING_TAG,
                label: "counter-cw",
                state: state.winding_row.0,
                on: state.ccw,
            },
        ];
        nodes.extend(toggle_button_group_nodes(
            OPTIONS_GROUP_TAG,
            "Axis declarations",
            &segments,
            focused,
        ));

        nodes.push(AccessNode::new(CAPTION_TAG, AriaRole::Status).with_name(caption(state)));
        nodes
    }

    /// R1518 §5.40 — the form group is one Tab stop with a roving cursor, so
    /// name the radio that cursor addresses as the `aria-activedescendant`.
    fn access_focus_target(state: &Options, focused: Option<&str>) -> Option<AccessFocus> {
        rc::composite_focus_target(
            FORM_TAG,
            focused,
            rc::active_index(&state.form_rows, state.form_focused),
        )
    }
}

impl WidgetView for PolarView {
    type Renderer = HelloPolarRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<PolarView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// Index of the out-of-period reading in the compass series.
    const WRAPPED_INDEX: usize = 4;

    fn options(form: usize, sector: bool, ccw: bool) -> Options {
        let mut out = Options::idle();
        out.form = form;
        out.form_rows[form].1 = true;
        out.sector = sector;
        out.sector_row = (ToggleState::Idle, sector);
        out.ccw = ccw;
        out.winding_row = (ToggleState::Idle, ccw);
        out
    }

    fn render(form: usize, sector: bool, ccw: bool) -> Scene {
        Owner::new().run(|| view(options(form, sector, ccw), &Frame::new()))
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

    fn command_count(scene: &Scene, tag: &str) -> usize {
        match find(scene, tag) {
            Some(Scene::Path(p)) => p.commands.len(),
            _ => panic!("no path tagged {tag}"),
        }
    }

    /// ★ The round, read off the paint: on a full turn the trace CLOSES —
    /// five samples and a `Close` — and on a sector it does not. A Qt radar
    /// gets that segment by appending the first point again.
    #[test]
    fn r1568_the_sector_chip_opens_the_loop() {
        assert_eq!(
            command_count(&render(FORM_COMPASS, false, false), "chart.series.0"),
            GUSTS.len() + 1,
            "five samples and a Close"
        );
        assert_eq!(
            command_count(&render(FORM_COMPASS, true, false), "chart.series.0"),
            GUSTS.len() - 1,
            "a sector drops the off-scale sample AND does not close"
        );
    }

    /// ★ The 372-degree reading is a bearing of 12 on a full turn — drawn and
    /// reported as wrapped — and genuinely off-scale on a sector. Qt's
    /// angular axis is an ordinary `QValueAxis`, so it is out of range in
    /// both cases and draws nothing.
    #[test]
    fn r1568_the_out_of_period_reading_is_placed_only_where_the_axis_closes() {
        let full = render(FORM_COMPASS, false, false);
        assert!(
            find(&full, &format!("chart.point.0.{WRAPPED_INDEX}")).is_some(),
            "the wrapped reading is drawn"
        );
        let text = text_of(&full, CAPTION_TAG);
        assert!(text.contains("bearing of 12"), "{text}");
        assert!(text.contains("1 wrapped, 0 off-scale"), "{text}");

        let sector = render(FORM_COMPASS, true, false);
        assert!(
            find(&sector, &format!("chart.point.0.{WRAPPED_INDEX}")).is_none(),
            "a sector cannot reach it"
        );
        let text = text_of(&sector, CAPTION_TAG);
        assert!(text.contains("0 wrapped, 1 off-scale"), "{text}");
    }

    /// ★ The radar form: five named spokes, both designs filled, and the
    /// polygon closes between the last facet and the first.
    #[test]
    fn r1568_the_radar_names_its_spokes_and_closes() {
        let scene = render(FORM_RADAR, false, false);
        assert_eq!(count_prefix(&scene, "chart.spoke."), FACETS.len());
        assert_eq!(count_prefix(&scene, "chart.area."), SCORES.len());
        assert_eq!(
            count_prefix(&scene, "chart.label.a."),
            FACETS.len(),
            "one label per facet"
        );
        for i in 0..SCORES.len() {
            assert_eq!(
                command_count(&scene, &format!("chart.series.{i}")),
                FACETS.len() + 1,
                "series {i} closes its loop"
            );
        }
    }

    /// ★ The winding is a declaration and it reaches the paint: the same
    /// bearing lands on the other side of the vertical. `QPolarChart` cannot
    /// draw the counter-clockwise convention at all.
    #[test]
    fn r1568_the_winding_chip_mirrors_the_plot() {
        let x_of = |scene: &Scene, j: usize| {
            let Some(Scene::Path(p)) = find(scene, &format!("chart.point.0.{j}")) else {
                panic!("point {j}")
            };
            f64::from(p.rect.x) + f64::from(p.rect.w) / 2.0
        };
        let cw = render(FORM_COMPASS, false, false);
        let ccw = render(FORM_COMPASS, false, true);
        // Sample 1 is the 90-degree bearing: 3 o'clock clockwise, 9 o'clock
        // the other way.
        assert!(x_of(&cw, 1) > x_of(&ccw, 1) + 20.0, "the plot mirrors");
        assert!(text_of(&cw, CAPTION_TAG).contains("increase clockwise"));
        assert!(
            text_of(&ccw, CAPTION_TAG).contains("increase counter-clockwise"),
            "{}",
            text_of(&ccw, CAPTION_TAG)
        );
    }

    /// ★ The grid is rings and spokes, and the rim follows the axis: a sector
    /// draws no full circle, because that would claim angles the axis does
    /// not carry.
    #[test]
    fn r1568_the_rim_follows_the_axis() {
        let full = render(FORM_COMPASS, false, false);
        assert!(find(&full, "chart.rim").is_some());
        assert!(count_prefix(&full, "chart.ring.") > 0);
        assert!(count_prefix(&full, "chart.spoke.") > 0);

        let sector = render(FORM_COMPASS, true, false);
        assert!(find(&sector, "chart.rim").is_none());
        assert!(count_prefix(&sector, "chart.spoke.") > 0);
    }

    /// ★ The form group is one Tab stop with two hit targets; the two
    /// declarations are their own stops.
    #[test]
    fn r1568_the_controls_carry_the_right_tab_stops() {
        let scene = render(FORM_COMPASS, false, false);
        let Some(Scene::Container(group)) = find(&scene, FORM_TAG) else {
            panic!("the form group is a container")
        };
        assert!(group.layout.focusable, "the group owns the stop");
        for i in 0..FORM_LABELS.len() {
            let Some(Scene::Container(cell)) = find(&scene, &format!("{FORM_TAG}#{i}")) else {
                panic!("cell {i} is a container")
            };
            assert!(!cell.layout.focusable, "a radio cell is not its own stop");
        }
        for tag in [SECTOR_TAG, WINDING_TAG] {
            let Some(Scene::Container(c)) = find(&scene, tag) else {
                panic!("{tag} is a container")
            };
            assert!(c.layout.focusable, "{tag} is its own stop");
        }
    }

    /// ★ What the axis did with the out-of-period reading reaches assistive
    /// technology: the caption is a live region, the forms a radiogroup, the
    /// declarations pressed buttons.
    #[test]
    fn r1568_a11y_exposes_the_form_the_declarations_and_the_report() {
        let nodes = PolarView::access_node(&options(FORM_COMPASS, true, true), None);
        let radios = nodes
            .iter()
            .filter(|n| matches!(n.role, AriaRole::RadioButton))
            .count();
        assert_eq!(radios, FORM_LABELS.len(), "one radio per form");
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, AriaRole::Button))
            .count();
        assert_eq!(buttons, 2, "one aria-pressed button per declaration");
        let status = nodes
            .iter()
            .find(|n| matches!(n.role, AriaRole::Status))
            .expect("the caption is a live region");
        let name = status.name.clone().unwrap_or_default();
        assert!(name.contains("Half turn"), "names the sweep: {name}");
        assert!(name.contains("off-scale"), "names the report: {name}");
        assert!(
            name.contains("counter-clockwise"),
            "names the winding: {name}"
        );
    }
}
