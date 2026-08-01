//! `hello-autoscale-y` — R1397 consumer of the `pinion-chart` dataviz
//! substrate: a chart whose **y-axis auto-fits the brushed x-window**.
//!
//! ## What this demonstrates
//!
//! One signal — a large startup transient (`x = 2`, `y = 5000`) sitting over a
//! long steady state of small ripples (`y` around 60, amplitude ~18). Seen
//! whole, the ripples are a flat line pinned to the bottom of the plot: the
//! transient owns the y-axis, so the detail that matters is invisible.
//!
//! A **brush** over the x-axis ([`Brush`], the primary [`RangeSliderExternal`])
//! selects a visible x-window. Dragging it *past* the transient does two things
//! at once:
//!
//! * [`LineChart::with_x_domain`] re-domains the chart to the window (the R1356
//!   clip drops the off-window transient), and
//! * [`LineChart::rescale_y_to_x_window`] (R1397) snaps the **y**-axis to just
//!   the points *inside* that window — so the steady-state ripples expand to
//!   fill the plot and the y-axis labels fall from thousands to tens.
//!
//! This is the canonical "auto-scale Y to the visible X range" of a monitoring
//! chart (an oscilloscope zoom, an auto-scaling price/latency chart): zoom the
//! time axis and the value axis follows. It is distinct from R1381's
//! [`LineChart::rescale_to_visible`], which rescales to the visible *series*;
//! this rescales the y-fit to the visible *x-range*.
//!
//! ## Why the brush is the only control
//!
//! The whole point is one gesture — brush the x-window — so this binding makes
//! the `RangeSliderExternal` its **primary** external (the `hello-range-slider`
//! shape: a manual [`WidgetCore`], not the scrub-primary macro the rest of the
//! chart family uses) rather than adding a fifth copy of the scrub wiring. The
//! brush strip carries the primary tag, so a drag on it routes to the range
//! external; it is RPC-drivable (`scene/intervene /external/low|high`) and
//! introspectable.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the rescale as tagged data: the y-tick label text
//! (`chart.label.y.{k}`) falls from a kilo magnitude to tens, and the series
//! polyline (`chart.series.0`) lifts its in-window samples up the plot as the
//! y-domain shrinks. Driving the brush over RPC is observed structurally,
//! without OCR (§2 #1 / #7 / #2). See `tools/demos/r1397_autoscale_y.py`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{
    Brush, BrushStripColors, ChartStyle, DataPoint, LineChart, Series, data_bounds,
};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::range_slider::RangeSliderExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloAutoscaleYRenderer, HelloAutoscaleYRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 460;

const THEME_TAG: &str = "app";
/// The primary range-slider tag: the brush strip's hit-test root and the
/// `Scene::External` registration tag. A drag on the strip routes here; over
/// RPC the window is `scene/intervene /external/low|high`.
const BRUSH_TAG: &str = "signal_brush";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// Window-absolute plot region (the `pinion-chart` `build` coordinate
/// contract — the chart is handed its rect before layout runs).
const CHART_RECT: Rect = Rect::new(14, 52, WIN_W - 28, WIN_H - 134);

/// The brush strip sits under the plot, aligned to the plot's x range.
const BRUSH_H: u32 = 14;
const BRUSH_GAP: u32 = 8;

/// Signal shape. A steady state of small ripples around [`BASELINE_Y`] with a
/// single large transient spiking at [`SPIKE_X`]. Kept as constants so the
/// demo's expectations are pinned to the data, not hand-synced.
const STEPS: usize = 40;
const SPIKE_X: usize = 2;
const TRANSIENT_Y: f64 = 5000.0;
const BASELINE_Y: f64 = 60.0;
const RIPPLE_AMP: f64 = 18.0;
const RIPPLE_FREQ: f64 = 0.7;

/// Deterministic sample data — one throughput series: a startup transient over
/// a long steady state of small ripples. The transient owns the full-view
/// y-axis; brushing past it (and [`LineChart::rescale_y_to_x_window`]) is what
/// resolves the steady-state detail.
#[allow(
    clippy::cast_precision_loss,
    reason = "step index (0..=40) -> f64 x-coordinate is exact"
)]
fn signal_series() -> Vec<Series> {
    let points: Vec<DataPoint> = (0..=STEPS)
        .map(|i| {
            let x = i as f64;
            let y = if i == SPIKE_X {
                TRANSIENT_Y
            } else {
                BASELINE_Y + RIPPLE_AMP * (x * RIPPLE_FREQ).sin()
            };
            DataPoint::new(x, y)
        })
        .collect();
    vec![Series::new("throughput", points)]
}

/// Full x extent of [`signal_series`] — the domain the brush fractions address.
/// Derived from the data (the crate's SSOT), never hand-written.
fn x_extent() -> (f64, f64) {
    data_bounds(&signal_series()).map_or((0.0, 1.0), |b| b.x)
}

/// The brush over the chart's x-axis — its `(low, high)` window maps onto the
/// data x-extent and re-domains the chart ([`LineChart::with_x_domain`]) for the
/// zoom that drives the y auto-fit.
fn brush() -> Brush {
    Brush::new(BRUSH_TAG, x_extent())
}

/// Read the brush window `(low, high)` from the primary external
/// ([`Brush::read`]); a missing external falls back to the full span.
fn read_brush(scene: &Scene) -> (f32, f32) {
    brush().read(scene)
}

/// Map the brush fractions onto the data x-extent ([`Brush::domain`]) — the
/// window that re-domains the chart for a zoom.
fn brush_domain(low: f32, high: f32) -> (f64, f64) {
    brush().domain(low, high)
}

/// Resolve the theme into a [`ChartStyle`].
fn chart_style(theme: &Theme) -> ChartStyle {
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

/// The brush strip ([`Brush::strip`]) aligned to the plot's x range — inset by
/// the chart's axis `margin` so it sits under the data, reading as an overview
/// of the full series.
fn brush_strip(theme: &Theme, style: &ChartStyle, low: f32, high: f32) -> Scene {
    // R1534 — the axis's own pixel span, from the crate that draws it
    // (`plot_area`), rather than re-deriving the margin insets here. Two
    // bindings carried that arithmetic and a third was about to.
    let axis = pinion_chart::plot_area(CHART_RECT, style.margin);
    let track = Rect::new(
        axis.x,
        CHART_RECT.y + CHART_RECT.h + BRUSH_GAP,
        axis.w,
        BRUSH_H,
    );
    let colors = BrushStripColors {
        track_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        accent: theme.resolve(ColorRole::Accent),
    };
    brush().strip(track, low, high, colors, "Signal x-window brush")
}

/// view-fn (§6.3): pure sync mapping. `(low, high)` is the brushed x window; the
/// chart re-domains to it AND auto-fits its y-axis to just that window's points.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(low: f32, high: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let style = chart_style(&theme);
    let (x_lo, x_hi) = brush_domain(low, high);

    let title = Scene::Text(
        TextNode::styled(
            "Throughput — drag the strip past the startup transient; the y-axis auto-fits",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 16)),
    );

    // The R1397 pairing: re-domain x to the brush window, and auto-fit the
    // y-axis to just that window's points.
    let chart = LineChart::new(signal_series())
        .filled(true)
        .with_x_domain(x_lo, x_hi)
        .rescale_y_to_x_window(true)
        .build(CHART_RECT, &style);

    let status = Scene::Text(
        TextNode::styled(
            format!("x {x_lo:.1}..{x_hi:.1} (drag the strip to zoom the x-window)"),
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
            brush_strip(&theme, &style, low, high),
            title,
            status,
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// The binding. The `RangeSliderExternal` brush is the **primary** external (the
/// manual `hello-range-slider` shape) so the demo needs no scrub — brushing the
/// x-window is the one interaction.
struct AutoscaleView;

impl WidgetCore for AutoscaleView {
    /// The brush window `(low, high)` as read from the primary external.
    type State = (f32, f32);
    // Value mutation flows through drag (pointer_move) + RPC intervene; no
    // keybinding-channel typed events.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // Full span at boot — the transient owns the y-axis until the user
        // brushes past it.
        Box::new(RangeSliderExternal::new())
    }

    fn tag() -> &'static str {
        BRUSH_TAG
    }

    fn read_state(scene: &Scene) -> (f32, f32) {
        read_brush(scene)
    }

    fn view(state: (f32, f32), frame: &Frame) -> Scene {
        view(state.0, state.1, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-autoscale-y (R1397 dataviz: y-axis auto-fits the brushed x-window)"
    }

    /// The brush is drag- and RPC-driven (the chart-family convention); it has
    /// no keyboard channel, so no key is consumed here.
    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &(f32, f32)) -> String {
        let (x_lo, x_hi) = brush_domain(state.0, state.1);
        format!(
            "brush {:.2}..{:.2} / x {x_lo:.1}..{x_hi:.1}",
            state.0, state.1
        )
    }
}

impl WidgetA11y for AutoscaleView {
    /// The brush window as a single `Slider` node. A two-thumb range has no
    /// single-`Float` shape, so — as `hello-chart` does — `AccessValue::Text`
    /// states the window plainly rather than leaving the zoom inaudible.
    fn access_node(state: &(f32, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (x_lo, x_hi) = brush_domain(state.0, state.1);
        vec![
            AccessNode::new(BRUSH_TAG, AriaRole::Slider)
                .with_name("Signal x-window brush".to_string())
                .with_value(AccessValue::Text(format!("x from {x_lo:.1} to {x_hi:.1}")))
                .with_state(pinion_a11y::AccessState {
                    focused: focused == Some(BRUSH_TAG),
                    ..pinion_a11y::AccessState::default()
                }),
        ]
    }
}

impl WidgetView for AutoscaleView {
    type Renderer = HelloAutoscaleYRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<AutoscaleView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_core::scene::{ExternalNode, PathCommand};

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

    fn rendered(low: f32, high: f32) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(low, high, &Frame::new()))
    }

    /// Series 0's vertical extent in WINDOW px: `(min_y, max_y)` over its path
    /// vertices. R1358 — a vertex's window y is `rect.y + command.y`; reading the
    /// bare command would turn a position claim into a span claim.
    fn series_window_y_range(scene: &Scene) -> (f32, f32) {
        let Scene::Path(p) = find(scene, "chart.series.0").expect("series") else {
            panic!("path")
        };
        let oy = f32::from(u16::try_from(p.rect.y).expect("chart y fits u16"));
        let ys: Vec<f32> = p
            .commands
            .iter()
            .filter_map(|c| match *c {
                PathCommand::MoveTo(pt) | PathCommand::LineTo(pt) => Some(oy + pt.y),
                _ => None,
            })
            .collect();
        (
            ys.iter().copied().fold(f32::INFINITY, f32::min),
            ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        )
    }

    /// The largest numeric y-tick label value on the chart, parsed from the
    /// `chart.label.y.{k}` text (SI `k` = *1000). Proxies the y-domain top.
    fn max_y_tick(scene: &Scene) -> f64 {
        let mut best = 0.0_f64;
        for k in 0..8 {
            let Some(Scene::Text(t)) = find(scene, &format!("chart.label.y.{k}")) else {
                continue;
            };
            let raw = t.content.trim();
            let (num, mul) = raw.strip_suffix('k').map_or((raw, 1.0), |n| (n, 1000.0));
            if let Ok(v) = num.trim().parse::<f64>() {
                best = best.max(v.abs() * mul);
            }
        }
        best
    }

    #[test]
    fn chart_and_brush_present() {
        let scene = rendered(0.0, 1.0);
        assert!(find(&scene, "chart").is_some());
        assert!(find(&scene, BRUSH_TAG).is_some());
        assert!(find(&scene, "chart.series.0").is_some());
    }

    #[test]
    fn boot_full_span_the_transient_owns_the_y_axis() {
        // Full brush: the y-axis spans the x=2 transient (5000), so its top tick
        // reaches a kilo magnitude.
        let scene = rendered(0.0, 1.0);
        assert!(
            max_y_tick(&scene) >= 1000.0,
            "boot y-axis reaches the transient magnitude, got {}",
            max_y_tick(&scene)
        );
    }

    #[test]
    fn brushing_past_the_transient_fits_the_y_axis_to_the_ripples() {
        // Brush to x >= ~8 (fraction 0.2 -> x_lo = 8), past the x=2 transient:
        // the y-axis auto-fits to the steady-state ripples (tens, not thousands).
        let scene = rendered(0.2, 1.0);
        assert!(
            max_y_tick(&scene) < 200.0,
            "brushed y-axis fits the ripples, got {}",
            max_y_tick(&scene)
        );
        // With the transient x-clipped away and the y-axis fitted to them, the
        // ripples now occupy a wide vertical band of the plot.
        let (rmin, rmax) = series_window_y_range(&scene);
        assert!(
            rmax - rmin > 40.0,
            "the ripples expand to fill the plot, band = {}",
            rmax - rmin
        );
    }

    #[test]
    fn a_ripple_point_lifts_up_the_plot_when_the_y_axis_fits() {
        // The rightmost sample (x=40) is inside BOTH the full and the brushed
        // window, so it is the same data point under two y-domains. When the
        // y-axis shrinks to the ripples it maps far higher up the plot (a
        // smaller window-y) than under the transient-owned domain.
        let boot = rendered(0.0, 1.0);
        let zoom = rendered(0.2, 1.0);
        let last_y = |scene: &Scene| -> f32 {
            let Scene::Path(p) = find(scene, "chart.series.0").expect("series") else {
                panic!("path")
            };
            let oy = f32::from(u16::try_from(p.rect.y).expect("y fits u16"));
            let (PathCommand::LineTo(pt) | PathCommand::MoveTo(pt)) =
                *p.commands.last().expect("at least one vertex")
            else {
                panic!("last command carries a point")
            };
            oy + pt.y
        };
        assert!(
            last_y(&zoom) < last_y(&boot) - 20.0,
            "x=40 lifts up the plot under the fitted y-axis: boot {} -> zoom {}",
            last_y(&boot),
            last_y(&zoom)
        );
    }

    #[test]
    fn full_brush_is_a_no_op_for_the_fit() {
        // A full-width window includes the transient, so the y-fit changes
        // nothing vs. boot — both reach the kilo magnitude.
        let a = rendered(0.0, 1.0);
        let b = rendered(0.0, 1.0);
        assert!((max_y_tick(&a) - max_y_tick(&b)).abs() < f64::EPSILON);
        assert!(max_y_tick(&a) >= 1000.0);
    }

    #[test]
    fn boot_external_seeds_the_full_span() {
        let scene = Scene::External(
            ExternalNode::new(AutoscaleView::create_external()).with_tag(BRUSH_TAG),
        );
        let (low, high) = read_brush(&scene);
        assert!(low.abs() < 1e-5, "boot low = 0, got {low}");
        assert!((high - 1.0).abs() < 1e-5, "boot high = 1, got {high}");
    }

    #[test]
    fn view_carries_the_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<AutoscaleView>(
            (0.0, 1.0),
            &Frame::new(),
        );
    }

    #[test]
    fn brush_window_is_audible() {
        let nodes = <AutoscaleView as WidgetA11y>::access_node(&(0.2, 0.8), None);
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
