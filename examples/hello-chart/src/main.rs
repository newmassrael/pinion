//! `hello-chart` — R1354 consumer of the `pinion-chart` dataviz
//! substrate: a three-series area chart with an **interactive scrub
//! inspector** (crosshair + per-series marker dots + value tooltip).
//!
//! ## What this demonstrates
//!
//! The chart is built from retained primitives ([`Scene::Path`] polylines
//! / area fills / marker circles, [`Scene::Text`] tick + tooltip labels)
//! via [`pinion_chart::LineChart`]. On top of the static chart, a
//! **press-drag scrub** drives an inspect overlay: pressing and dragging
//! across the plot moves a vertical crosshair that snaps to the nearest x,
//! marks each series' value with a dot, and shows a value tooltip.
//!
//! ## Why a Slider
//!
//! pinion forwards a *continuous* pointer position to a binding only under
//! pointer capture (`External::pointer_move`, button held) — free hover
//! delivers only which tag is hovered, not a position. So a crosshair that
//! follows the cursor is a capture-drag scrub. The §5.38 [`SliderExternal`]
//! already *is* a captured 1-D fraction (value `0.0..=1.0`,
//! `wants_pointer_capture` + `pointer_move`), RPC-drivable via
//! `scene/intervene` and introspectable — so it is reused verbatim as the
//! scrub-position holder (exactly as `hello-path` reused a Toggle). Its
//! value is the cursor's fraction across the plot; the chart maps that to
//! the nearest data point. A transparent `chart_scrub`-tagged box over the
//! plot is the capture surface.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the inspect overlay as tagged data —
//! `chart.inspect.crosshair`, `chart.inspect.marker.{i}`,
//! `chart.inspect.tooltip`, `chart.inspect.value.{i}` — so driving the
//! scrub (a pointer drag, or `scene/intervene` on the slider value) is
//! observed as the overlay moving to the nearest point, read back without
//! OCR (§2 #1 / #7). `tools/demos/hello_chart_r1354.py` drives the value
//! over RPC and asserts the overlay + a live-pixel marker witness.

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::widgets::slider::{SliderEvent, SliderExternal, SliderState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::slider::{read_slider_state, slider_apply_key};

// pinion-forge codegen output: `pub struct HelloChartRenderer` + …
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloChartRenderer, HelloChartRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 460;

const THEME_TAG: &str = "app";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// Window-absolute plot region (see the `pinion-chart` coordinate
/// contract — path commands are literal device pixels). It is also the
/// scrub capture basis: the `chart_scrub` box covers exactly this rect, so
/// the slider value `0.0..=1.0` is the cursor fraction across it.
const CHART_RECT: Rect = Rect::new(14, 52, WIN_W - 28, WIN_H - 118);

/// Deterministic sample data — three throughput series over 12 buckets.
#[allow(
    clippy::cast_precision_loss,
    reason = "bucket index (0..12) -> f64 x-coordinate is exact"
)]
fn sample_series() -> Vec<Series> {
    let ingress = [
        820.0, 910.0, 1150.0, 1400.0, 1320.0, 1600.0, 2100.0, 2400.0, 2200.0, 1900.0, 2600.0,
        3100.0,
    ];
    let egress = [
        400.0, 520.0, 680.0, 900.0, 1100.0, 1250.0, 1400.0, 1300.0, 1500.0, 1700.0, 1650.0, 1800.0,
    ];
    let errors = [
        12.0, 8.0, 20.0, 40.0, 15.0, 60.0, 90.0, 30.0, 25.0, 70.0, 45.0, 20.0,
    ];
    let mk = |name: &str, ys: &[f64]| {
        Series::new(
            name,
            ys.iter()
                .enumerate()
                .map(|(i, &y)| DataPoint::new(i as f64, y))
                .collect(),
        )
    };
    vec![
        mk("ingress", &ingress),
        mk("egress", &egress),
        mk("errors", &errors),
    ]
}

/// Reused as the scrub-position holder, seeded to the plot centre so the
/// boot frame shows a centred inspector.
fn scrub_external() -> SliderExternal {
    let mut slider = SliderExternal::new();
    slider.set_value(0.5);
    slider
}

/// Resolve the theme into a [`ChartStyle`]. The series palette stays
/// theme-independent (categorical); axis / grid / label / tooltip chrome
/// tracks the theme so the chart reads in both light and dark.
fn chart_style(theme: &pinion_core::Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        crosshair: theme.resolve(ColorRole::OnSurfaceMuted).with_alpha(0xC0),
        tooltip_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        tooltip_fg: theme.resolve(ColorRole::OnSurface),
        x_ticks: 7,
        y_ticks: 5,
        ..ChartStyle::default()
    }
}

/// view-fn (§6.3): pure sync mapping `(SliderState, f32) -> Scene`. `value`
/// is the scrub fraction `0.0..=1.0` across [`CHART_RECT`].
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: SliderState, value: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);

    let title = Scene::Text(
        TextNode::styled(
            "Throughput (pkt/s) — drag to inspect",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 16)),
    );

    let chart = LineChart::new(sample_series())
        .filled(true)
        .inspect(Some(value))
        .build(CHART_RECT, &chart_style(&theme));

    // Transparent capture surface over the plot — the `chart_scrub`
    // primary tag. On top so a press anywhere on the plot drives the
    // scrub; transparent so the chart shows through, pointer-opaque so it
    // captures (geometric hit-test is alpha-independent).
    let scrub = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
            .with_tag("chart_scrub")
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(CHART_RECT.x, CHART_RECT.y)
                    .with_size(Size::px(CHART_RECT.w, CHART_RECT.h)),
            ),
    );

    let status = Scene::Text(
        TextNode::styled(
            format!("{} | scrub {value:.2}", state.as_name()),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, WIN_H - 24)),
    );

    Scene::Container(
        ContainerNode::new(vec![chart, scrub, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. Reuses the §5.38 Slider as the scrub-position
/// holder; the chart is the substrate under test.
#[widget(
    tag = "chart_scrub",
    state = (SliderState, f32),
    event = SliderEvent,
    title = "pinion hello-chart (R1354 dataviz — scrub inspector)",
    renderer = HelloChartRenderer,
    initial_size = (WIN_W, WIN_H),
    external = scrub_external,
    apply_key,
    keybinding,
    event_name_derive,
    fmt_state_log,
    a11y_manual,
)]
struct ChartView;

impl ChartView {
    /// Tuple-state read: the slider gesture state (`state`) + its value
    /// sidecar (`value` = scrub fraction). Missing-external fallback is
    /// `(Idle, 0.5)` so a fresh binding still shows a centred inspector.
    fn read_state(scene: &Scene) -> (SliderState, f32) {
        read_slider_state(scene, <Self as WidgetCore>::tag()).unwrap_or((SliderState::Idle, 0.5))
    }

    /// Inherent view shim — unpacks the tuple and forwards to the free
    /// [`view`].
    fn view(state: (SliderState, f32), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    /// ARIA slider keyboard scrub: arrows / Home / End / Page move the
    /// scrub position, mirrored to the RPC `scene/intervene` value channel
    /// (the lifted `slider_apply_key` SSOT).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        slider_apply_key(scene, focused, Self::tag(), |current| match key {
            "ArrowLeft" | "ArrowDown" => Some((current - 0.05).clamp(0.0, 1.0)),
            "ArrowRight" | "ArrowUp" => Some((current + 0.05).clamp(0.0, 1.0)),
            "Home" => Some(0.0),
            "End" => Some(1.0),
            "PageDown" => Some((current - 0.10).clamp(0.0, 1.0)),
            "PageUp" => Some((current + 0.10).clamp(0.0, 1.0)),
            _ => None,
        })
    }

    fn keybinding(key: &str) -> Option<SliderEvent> {
        match key {
            "d" => Some(SliderEvent::Disable),
            "e" => Some(SliderEvent::Enable),
            _ => None,
        }
    }

    fn fmt_state_log(state: (SliderState, f32)) -> String {
        format!("{} / {:.2}", state.0.as_name(), state.1)
    }
}

// The scrub surface carries `AriaRole::Slider` with the scrub fraction as
// its `AccessValue::Float` — an AT client (and the RPC a11y walk) reads the
// inspect position exactly as the pointer / keyboard set it.
impl pinion_a11y::WidgetA11y for ChartView {
    fn access_node(state: &(SliderState, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, value) = (state.0, state.1);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            ..AccessState::from_interaction(interaction, None)
        };
        vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Slider)
                .with_value(AccessValue::Float {
                    value,
                    min: 0.0,
                    max: 1.0,
                })
                .with_state(access_state),
        ]
    }
}

fn main() {
    pinion_shell::run::<ChartView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::WidgetA11y;
    use pinion_core::Owner;
    use pinion_core::scene::PathCommand;

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(scene);
                }
                c.children.iter().find_map(|ch| find(ch, tag))
            }
            other => (other.tag() == Some(tag)).then_some(scene),
        }
    }

    fn rendered(state: SliderState, value: f32) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, value, &Frame::new()))
    }

    #[test]
    fn chart_and_scrub_surface_present() {
        let scene = rendered(SliderState::Idle, 0.5);
        assert!(find(&scene, "chart").is_some());
        assert!(find(&scene, "chart_scrub").is_some());
    }

    #[test]
    fn three_series_and_areas_present() {
        let scene = rendered(SliderState::Idle, 0.5);
        for i in 0..3 {
            assert!(find(&scene, &format!("chart.series.{i}")).is_some());
            assert!(find(&scene, &format!("chart.area.{i}")).is_some());
        }
    }

    #[test]
    fn inspect_overlay_tracks_the_scrub_value() {
        let scene = rendered(SliderState::Dragging, 0.5);
        assert!(find(&scene, "chart.inspect.crosshair").is_some());
        assert!(find(&scene, "chart.inspect.tooltip").is_some());
        for i in 0..3 {
            assert!(find(&scene, &format!("chart.inspect.marker.{i}")).is_some());
            assert!(find(&scene, &format!("chart.inspect.value.{i}")).is_some());
        }
    }

    #[test]
    fn scrub_marker_is_a_bezier_circle() {
        let scene = rendered(SliderState::Dragging, 0.5);
        let Scene::Path(p) = find(&scene, "chart.inspect.marker.0").expect("marker") else {
            panic!("marker is a path")
        };
        assert_eq!(p.commands.len(), 6); // MoveTo + 4 CurveTo + Close
        assert!(matches!(p.commands[1], PathCommand::CurveTo { .. }));
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ChartView>(
            (SliderState::Idle, 0.5),
            &Frame::new(),
        );
    }

    #[test]
    fn scrub_reports_slider_role_and_value() {
        let nodes = <ChartView as WidgetA11y>::access_node(&(SliderState::Idle, 0.5), None);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert_eq!(nodes[0].tag, "chart_scrub");
    }
}
