//! `hello-chart` — R1354-R1357 consumer of the `pinion-chart` dataviz
//! substrate: a three-series area chart with a **scrub inspector** and a
//! **brush-zoom strip**.
//!
//! ## What this demonstrates
//!
//! The chart is built from retained primitives ([`Scene::Path`] polylines
//! / area fills / marker circles, [`Scene::Text`] tick + tooltip labels)
//! via [`pinion_chart::LineChart`]. Two interactions sit on top:
//!
//! * **Scrub inspect** (R1355) — press-drag across the plot moves a
//!   vertical crosshair that snaps to the nearest x, marks each series'
//!   value with a dot, and shows a value tooltip.
//! * **Brush zoom** (R1357) — drag the strip under the chart to select the
//!   visible x window; the chart re-domains to it and `pinion-chart` clips
//!   each series to the window (R1356).
//!
//! ## Why a Slider + a `RangeSlider`
//!
//! pinion forwards a *continuous* pointer position to a binding only under
//! pointer capture (`External::pointer_move`, button held) — free hover
//! delivers only which tag is hovered, not a position. So both gestures are
//! capture drags, and each needs its own captured value. Rather than invent
//! externals, the binding reuses the §5.38 [`SliderExternal`] (a captured
//! 1-D fraction) as the scrub position and the [`RangeSliderExternal`] (a
//! captured 1-D *pair*) as the brush window — the latter hosted through the
//! R1249 `extra_externals` slot, so the router dispatches each drag by tag.
//! Both are RPC-drivable (`scene/intervene`) and introspectable.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the overlay + zoom as tagged data —
//! `chart.inspect.crosshair` / `.marker.{i}` / `.tooltip` / `.value.{i}`,
//! and the series polylines re-clipped to the brushed window. Driving
//! either external over RPC is observed structurally, without OCR
//! (§2 #1 / #7). See `tools/demos/hello_chart_r1354.py`.

use pinion_a11y::described::describedby_region;
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series, data_bounds};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::range_slider::RangeSliderExternal;
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
const SCRUB_TAG: &str = "chart_scrub";
const BRUSH_TAG: &str = "chart_brush";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// Window-absolute plot region (see the `pinion-chart` coordinate
/// contract — path commands are literal device pixels). Also the scrub
/// capture basis: the `chart_scrub` box covers exactly this rect, so the
/// slider value `0.0..=1.0` is the cursor fraction across it.
const CHART_RECT: Rect = Rect::new(14, 46, WIN_W - 28, WIN_H - 128);

/// The brush strip sits under the plot, aligned to the plot's x range.
const BRUSH_H: u32 = 14;
const BRUSH_GAP: u32 = 8;

/// Full x extent of [`sample_series`] — the domain the brush fractions
/// address. Derived from the data, never hand-written: a hand-kept
/// constant that fell behind the data would pin the domain short, and
/// R1356 clipping would then silently drop the newest points with no
/// error. `data_bounds` is the crate's SSOT for this.
fn x_extent() -> (f64, f64) {
    data_bounds(&sample_series()).map_or((0.0, 1.0), |b| b.x)
}

/// Narrowest window the brush can select, as a fraction of the extent —
/// keeps the domain non-degenerate.
const MIN_BRUSH_SPAN: f32 = 0.04;

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

/// R1249 — the brush window as a sibling `External`, dispatched by tag.
/// `RangeSliderExternal::new()` selects the full span (boot = no zoom).
fn brush_extras() -> Vec<ExtraExternal> {
    vec![ExtraExternal::new(
        BRUSH_TAG,
        Box::new(RangeSliderExternal::new()),
    )]
}

/// Read the brush window `(low, high)` from the sibling external. A
/// missing external falls back to the full span.
fn read_brush(scene: &Scene) -> (f32, f32) {
    let Some(node) = scene.find_external_with_tag(BRUSH_TAG) else {
        return (0.0, 1.0);
    };
    let Some(intro) = node.handle.introspect() else {
        return (0.0, 1.0);
    };
    let read = |field: &str, fallback: f32| {
        intro
            .query(field)
            .and_then(|v| v.as_f32())
            .unwrap_or(fallback)
    };
    (read("low", 0.0), read("high", 1.0))
}

/// Map the brush fractions onto the data x extent, enforcing a minimum
/// span so the domain never collapses.
fn brush_domain(low: f32, high: f32) -> (f64, f64) {
    let (low, high) = if low <= high {
        (low, high)
    } else {
        (high, low)
    };
    let high = high.max(low + MIN_BRUSH_SPAN).min(1.0);
    let low = low.min(high - MIN_BRUSH_SPAN).max(0.0);
    let (x_min, x_max) = x_extent();
    let span = x_max - x_min;
    (
        x_min + f64::from(low) * span,
        x_min + f64::from(high) * span,
    )
}

/// Resolve the theme into a [`ChartStyle`].
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

/// The brush strip: a `chart_brush`-tagged track (the `RangeSlider` capture
/// basis) holding the selected span + two thumbs, aligned to the plot x
/// range so the strip reads as an overview of the full series.
fn brush_strip(theme: &pinion_core::Theme, style: &ChartStyle, low: f32, high: f32) -> Scene {
    let m = style.margin;
    let track_x = CHART_RECT.x + m.left;
    let track_w = CHART_RECT.w.saturating_sub(m.left + m.right).max(1);
    let track_y = CHART_RECT.y + CHART_RECT.h + BRUSH_GAP;

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "fraction * track width -> px offset; both are display-bounded"
    )]
    let (span_x, span_w) = {
        let w = track_w as f32;
        let lo = (low.clamp(0.0, 1.0) * w).round() as u32;
        let hi = (high.clamp(0.0, 1.0) * w).round() as u32;
        (lo, hi.saturating_sub(lo).max(2))
    };

    let accent = theme.resolve(ColorRole::Accent);
    let selected = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(accent.with_alpha(0x66))).with_layout(
            LayoutStyle::new()
                .with_absolute_position(span_x, 0)
                .with_size(Size::px(span_w, BRUSH_H)),
        ),
    );
    let thumb = |x: u32| {
        Scene::Box(
            BoxNode::new(
                Rect::default(),
                BoxStyle::filled(accent).with_corner_radius(2),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x.saturating_sub(2), 0)
                    .with_size(Size::px(4, BRUSH_H)),
            ),
        )
    };

    Scene::Container(
        ContainerNode::new(vec![selected, thumb(span_x), thumb(span_x + span_w)])
            .with_tag(BRUSH_TAG)
            .with_aria_label("Chart x-window brush")
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_corner_radius(BRUSH_H / 2),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(track_x, track_y)
                    .with_size(Size::px(track_w, BRUSH_H)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping. `scrub` is the inspect fraction
/// across [`CHART_RECT`]; `(low, high)` is the brushed x window.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: SliderState, scrub: f32, low: f32, high: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let style = chart_style(&theme);
    let (x_lo, x_hi) = brush_domain(low, high);

    let title = Scene::Text(
        TextNode::styled(
            "Throughput (pkt/s) — drag plot to inspect, strip to zoom",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 14)),
    );

    let chart = LineChart::new(sample_series())
        .filled(true)
        .inspect(Some(scrub))
        .with_x_domain(x_lo, x_hi)
        .build(CHART_RECT, &style);

    // Transparent capture surface over the plot — the `chart_scrub`
    // primary tag. On top so a press anywhere on the plot drives the
    // scrub; transparent so the chart shows through, pointer-opaque so it
    // captures (geometric hit-test is alpha-independent).
    let scrub_surface = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
            .with_tag(SCRUB_TAG)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(CHART_RECT.x, CHART_RECT.y)
                    .with_size(Size::px(CHART_RECT.w, CHART_RECT.h)),
            ),
    );

    let status = Scene::Text(
        TextNode::styled(
            format!(
                "{} | scrub {scrub:.2} | x {x_lo:.1}..{x_hi:.1}",
                state.as_name()
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, WIN_H - 22)),
    );

    Scene::Container(
        ContainerNode::new(vec![
            chart,
            scrub_surface,
            brush_strip(&theme, &style, low, high),
            title,
            status,
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. The Slider is the scrub position (primary); the
/// RangeSlider is the brush window (R1249 sibling external).
#[widget(
    // Literal (the macro's `tag` takes no const); `SCRUB_TAG` mirrors it
    // and `scrub_reports_slider_role_and_value` pins the two together.
    tag = "chart_scrub",
    state = (SliderState, f32, f32, f32),
    event = SliderEvent,
    title = "pinion hello-chart (R1354-R1357 dataviz: scrub + brush zoom)",
    renderer = HelloChartRenderer,
    initial_size = (WIN_W, WIN_H),
    external = scrub_external,
    extra_externals = brush_extras,
    apply_key,
    keybinding,
    event_name_derive,
    fmt_state_log,
    a11y_manual,
)]
struct ChartView;

impl ChartView {
    /// Reads both externals by tag (the `Container([primary, ...extras])`
    /// shape extras impose rules out the derived single-External read).
    fn read_state(scene: &Scene) -> (SliderState, f32, f32, f32) {
        let (state, scrub) =
            read_slider_state(scene, SCRUB_TAG).unwrap_or((SliderState::Idle, 0.5));
        let (low, high) = read_brush(scene);
        (state, scrub, low, high)
    }

    fn view(state: (SliderState, f32, f32, f32), frame: Frame) -> Scene {
        view(state.0, state.1, state.2, state.3, &frame)
    }

    /// ARIA slider keyboard scrub, mirrored through the RPC
    /// `scene/intervene` value channel (the lifted `slider_apply_key`).
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

    fn fmt_state_log(state: (SliderState, f32, f32, f32)) -> String {
        format!(
            "{} / scrub {:.2} / brush {:.2}..{:.2}",
            state.0.as_name(),
            state.1,
            state.2,
            state.3
        )
    }
}

// The scrub surface carries `AriaRole::Slider` with the scrub fraction as
// its `AccessValue::Float`. The brush is a sibling external with its own
// tag; its two-thumb value has no single-Float AT shape, so it is exposed
// by tag + role only.
impl pinion_a11y::WidgetA11y for ChartView {
    fn access_node(state: &(SliderState, f32, f32, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, scrub, low, high) = (state.0, state.1, state.2, state.3);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            ..AccessState::from_interaction(interaction, None)
        };
        let (x_lo, x_hi) = brush_domain(low, high);
        let readout = LineChart::new(sample_series())
            .inspect(Some(scrub))
            .with_x_domain(x_lo, x_hi)
            .inspect_readout(CHART_RECT, &ChartStyle::default());

        let control = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Slider)
            .with_value(AccessValue::Float {
                value: scrub,
                min: 0.0,
                max: 1.0,
            })
            .with_state(access_state);
        let mut nodes = describedby_region(
            control,
            "chart.inspect.tooltip",
            AriaRole::Tooltip,
            readout,
            true,
        );
        // The brush window is a sibling external. A two-thumb range has no
        // single-Float shape — but `AccessValue::Text` states it plainly
        // rather than leaving the zoom inaudible.
        nodes.push(
            AccessNode::new(BRUSH_TAG, AriaRole::Slider)
                .with_name("Chart x-window brush".to_string())
                .with_value(AccessValue::Text(format!("x from {x_lo:.1} to {x_hi:.1}"))),
        );
        nodes
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

    fn rendered(scrub: f32, low: f32, high: f32) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(SliderState::Idle, scrub, low, high, &Frame::new()))
    }

    fn series_x_range(scene: &Scene) -> (f32, f32) {
        let Scene::Path(p) = find(scene, "chart.series.0").expect("series") else {
            panic!("path")
        };
        let xs: Vec<f32> = p
            .commands
            .iter()
            .filter_map(|c| match *c {
                PathCommand::MoveTo(pt) | PathCommand::LineTo(pt) => Some(pt.x),
                _ => None,
            })
            .collect();
        (
            xs.iter().copied().fold(f32::INFINITY, f32::min),
            xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        )
    }

    #[test]
    fn chart_scrub_and_brush_surfaces_present() {
        let scene = rendered(0.5, 0.0, 1.0);
        assert!(find(&scene, "chart").is_some());
        assert!(find(&scene, SCRUB_TAG).is_some());
        assert!(find(&scene, BRUSH_TAG).is_some());
    }

    #[test]
    fn inspect_overlay_tracks_the_scrub_value() {
        let scene = rendered(0.5, 0.0, 1.0);
        assert!(find(&scene, "chart.inspect.crosshair").is_some());
        assert!(find(&scene, "chart.inspect.tooltip").is_some());
        for i in 0..3 {
            assert!(find(&scene, &format!("chart.inspect.marker.{i}")).is_some());
        }
    }

    #[test]
    fn brushing_narrows_the_domain_and_keeps_series_in_the_plot() {
        // Full span vs a narrow brush: the series must stay inside the
        // plot's x range in BOTH cases (R1356 clipping), and the zoomed
        // polyline must still span the plot width.
        let full = rendered(0.5, 0.0, 1.0);
        let zoom = rendered(0.5, 0.4, 0.6);
        for scene in [&full, &zoom] {
            let (min_x, max_x) = series_x_range(scene);
            assert!(
                min_x >= -0.5 && max_x <= f32::from(u16::try_from(WIN_W).unwrap()) + 0.5,
                "series stays within the window: {min_x}..{max_x}"
            );
        }
    }

    #[test]
    fn full_brush_domain_covers_every_data_point() {
        // The R1354 SSOT defect this guards: the x extent used to be a
        // hand-written `X_MAX = 11.0` beside `sample_series()`. Adding a
        // 13th bucket would have left the domain pinned at 0..11 and R1356
        // clipping would then drop the newest point SILENTLY — no error, and
        // the demo's `len == 12` assertion would have concealed it. The
        // extent is derived now; this pins the invariant either way.
        let (lo, hi) = brush_domain(0.0, 1.0);
        for s in sample_series() {
            for p in &s.points {
                assert!(
                    p.x >= lo && p.x <= hi,
                    "point x={} falls outside the full-brush domain {lo}..{hi}",
                    p.x
                );
            }
        }
    }

    #[test]
    fn brush_domain_enforces_a_minimum_span() {
        let (lo, hi) = brush_domain(0.5, 0.5);
        assert!(hi > lo, "collapsed brush is widened, got {lo}..{hi}");
        let (lo, hi) = brush_domain(0.8, 0.2); // reversed
        assert!(hi > lo, "reversed brush is normalised, got {lo}..{hi}");
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ChartView>(
            (SliderState::Idle, 0.5, 0.0, 1.0),
            &Frame::new(),
        );
    }

    #[test]
    fn scrub_reports_slider_role_and_value() {
        let nodes =
            <ChartView as WidgetA11y>::access_node(&(SliderState::Idle, 0.5, 0.0, 1.0), None);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert_eq!(nodes[0].tag, SCRUB_TAG);
    }

    #[test]
    fn at_hears_the_readout_not_just_the_fraction() {
        // The R1355 defect this guards: the inspector's values were computed
        // and painted, then dropped at the a11y boundary — a screen reader
        // heard "slider, 0.5" and never the numbers the chart exists to show.
        let nodes =
            <ChartView as WidgetA11y>::access_node(&(SliderState::Idle, 0.5, 0.0, 1.0), None);
        assert_eq!(
            nodes[0].described_by.as_deref(),
            Some("chart.inspect.tooltip"),
            "scrub is describedby the inspect region"
        );
        let region = nodes
            .iter()
            .find(|n| n.tag == "chart.inspect.tooltip")
            .expect("the described region is IN the tree (no dangling reference)");
        let name = region.name.as_deref().expect("region carries the readout");
        assert!(
            name.starts_with("x = "),
            "readout names the focus: {name:?}"
        );
        for series in ["ingress", "egress", "errors"] {
            assert!(name.contains(series), "readout names {series}: {name:?}");
        }
    }

    #[test]
    fn brush_window_is_audible() {
        // Was: role + label only, so the zoom was inaudible. A two-thumb range
        // has no Float shape, but `AccessValue::Text` states it plainly.
        let nodes =
            <ChartView as WidgetA11y>::access_node(&(SliderState::Idle, 0.5, 0.3, 0.6), None);
        let brush = nodes
            .iter()
            .find(|n| n.tag == BRUSH_TAG)
            .expect("brush is in the a11y tree");
        let Some(AccessValue::Text(text)) = brush.value.as_ref() else {
            panic!("brush value is Text, got {:?}", brush.value)
        };
        assert!(
            text.starts_with("x from "),
            "brush states its window: {text:?}"
        );
    }
}
