//! `hello-scatter` — R1377 consumer of the `pinion-chart` correlation form:
//! a [`pinion_chart::ScatterChart`] of two labelled sample series with a
//! **scrub inspector**.
//!
//! ## What this demonstrates
//!
//! A scatter chart plots each sample as an isolated point rather than joining a
//! series into a line — the correlation view (does y track x?). It is the
//! crate's THIRD cartesian chart, and being the third is what forced the shared
//! axis furniture (gridlines / axes / y-tick labels) to factor out of the line
//! and bar charts — and, as the third legend and circle-marker consumer,
//! collapsed the legend row and the circle geometry to one definition too. This
//! binding is the consumer that pays for that lift. The chart is built from
//! retained primitives the paint adapter already rasterizes: filled
//! [`Scene::Path`] circles (one per point), [`Scene::Box`] legend swatches, and
//! [`Scene::Text`] labels. Two interactions sit on top:
//!
//! * **Scrub inspect** (R1355's overlay, now for scatter) — press-drag across
//!   the chart SCRUBS the x-axis; a vertical crosshair marks the scrubbed x, a
//!   ring frames each series' nearest point, and a tooltip shows their values.
//!   pinion forwards a continuous pointer position only under capture, so the
//!   gesture is a capture drag whose 1-D fraction the chart maps through its
//!   margins + domain to a data x (a 2-D nearest-point hover would need a 2-D
//!   pointer external — deferred).
//! * **Brush cross-filter** (R1391) — drag the overview strip UNDER the plot to
//!   select an x-window; the scatter mutes every point outside it (dimmed, still
//!   drawn as context) through [`ScatterChart::select_x_range`]. The strip and
//!   the plot are two distinct widgets, so this is a numeric cross-filter (a
//!   brush in one widget dims marks in another) — the continuous-range twin of
//!   `hello-cross-filter`'s categorical bar-click (R1384).
//!
//! ## Why a Slider + a `RangeSlider`
//!
//! The §5.38 [`SliderExternal`] (a captured 1-D fraction) is the scrub position,
//! and the [`RangeSliderExternal`](pinion_core::widgets::range_slider::RangeSliderExternal)
//! (a captured 1-D pair) is the brush window — the latter a sibling in the
//! R1249 `extra_externals` slot, so the router dispatches each drag by tag (the
//! lifted [`Brush`] substrate, shared with `hello-chart`). Both are RPC-drivable
//! (`scene/intervene`) and introspectable, no new external invented.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the scatter + overlay as tagged data —
//! `chart.point.{i}.{j}`, `chart.inspect.crosshair` / `.ring.{i}` / `.tooltip` /
//! `.header` / `.value.{i}`, `chart.legend.{i}.*`. Driving the scrub over RPC
//! re-rings a different point, observed structurally without OCR (§2 #1 / #7).
//! See `tools/demos/r1377_scatter_scrub.py`.

use pinion_a11y::described::describedby_region;
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{
    Brush, BrushStripColors, ChartStyle, DataPoint, ScatterChart, Series, data_bounds,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::slider::{SliderEvent, SliderExternal, SliderState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::slider::{read_slider_state, slider_apply_key};

// pinion-forge codegen output: `pub struct HelloScatterRenderer` + …
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloScatterRenderer, HelloScatterRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 460;

const THEME_TAG: &str = "app";
const SCRUB_TAG: &str = "scatter_scrub";

/// R1391/R1394 — the brush strip is a sibling `scatter_brush` external whose
/// selected x-window cross-filters the SCATTER (a different widget) rather than
/// zooming itself, muting the point marks outside the window. The wiring is the
/// lifted [`Brush`] substrate (R1394), shared with `hello-chart` and
/// `hello-linked-brush`.
const BRUSH_TAG: &str = "scatter_brush";
const BRUSH_H: u32 = 14;
const BRUSH_GAP: u32 = 8;

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// Window-absolute scatter region. The chart is pinned (`ScatterChart::build`) —
/// the point geometry is what this demo exercises, not the layout-native seam
/// (the profiler already proves `build_fill`), so a const rect keeps it simple.
/// It is also the scrub capture basis: the `scatter_scrub` box covers exactly
/// this rect, so the slider value `0.0..=1.0` is the cursor fraction across it.
const CHART_RECT: Rect = Rect::new(10, 40, WIN_W - 20, WIN_H - 104);

/// Two illustrative sample series — the canonical correlation a scatter shows.
/// Fixed sample data (like `hello-chart`'s `sample_series`): the demo exists to
/// prove the point-mark geometry + the scrub inspector, not to measure anything.
/// Series A rises with x, B falls, so the two clouds separate legibly.
fn samples() -> Vec<Series> {
    vec![
        Series::new(
            "rising",
            vec![
                DataPoint::new(1.0, 2.0),
                DataPoint::new(2.0, 3.5),
                DataPoint::new(3.0, 3.0),
                DataPoint::new(4.0, 5.0),
                DataPoint::new(5.0, 6.5),
                DataPoint::new(6.0, 6.0),
                DataPoint::new(7.0, 8.0),
                DataPoint::new(8.0, 9.0),
            ],
        ),
        Series::new(
            "falling",
            vec![
                DataPoint::new(1.5, 9.0),
                DataPoint::new(2.5, 7.5),
                DataPoint::new(3.5, 8.0),
                DataPoint::new(4.5, 6.0),
                DataPoint::new(5.5, 5.0),
                DataPoint::new(6.5, 4.5),
                DataPoint::new(7.5, 3.0),
                DataPoint::new(8.5, 2.0),
            ],
        ),
    ]
}

/// Reused as the scrub-position holder, seeded to the centre so the boot frame
/// shows an inspected point.
fn scrub_external() -> SliderExternal {
    let mut slider = SliderExternal::new();
    slider.set_value(0.5);
    slider
}

/// The brush over the scatter's x-axis — the lifted [`Brush`] substrate
/// (R1394). Its `(low, high)` window maps onto the data x-extent and drives
/// [`ScatterChart::select_x_range`].
fn brush() -> Brush {
    Brush::new(BRUSH_TAG, x_extent())
}

/// R1391 — the brush window as a sibling `External` ([`Brush::extras`]); the fn
/// the `#[widget]` `extra_externals` attribute points at. A full-span boot
/// selection cross-filters nothing until the user drags.
fn brush_extras() -> Vec<ExtraExternal> {
    brush().extras()
}

/// Read the brush window `(low, high)` fractions from the sibling external
/// ([`Brush::read`]); a missing external falls back to the full span.
fn read_brush(scene: &Scene) -> (f32, f32) {
    brush().read(scene)
}

/// The full x-extent of [`samples`] — the domain the brush fractions map onto.
/// Derived from the data (never hand-written), the `data_bounds` SSOT.
fn x_extent() -> (f64, f64) {
    data_bounds(&samples()).map_or((0.0, 1.0), |b| b.x)
}

/// Map the brush fractions onto the data x-extent ([`Brush::domain`]) — the
/// window that feeds [`ScatterChart::select_x_range`] so points outside it mute.
fn brush_domain(low: f32, high: f32) -> (f64, f64) {
    brush().domain(low, high)
}

/// The brush strip under the plot ([`Brush::strip`]), aligned to the full
/// scatter rect (the scatter draws no axis margins) so it reads as an overview
/// of the x-axis.
fn brush_strip(theme: &pinion_core::Theme, low: f32, high: f32) -> Scene {
    let track = Rect::new(
        CHART_RECT.x,
        CHART_RECT.y + CHART_RECT.h + BRUSH_GAP,
        CHART_RECT.w,
        BRUSH_H,
    );
    let colors = BrushStripColors {
        track_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        accent: theme.resolve(ColorRole::Accent),
    };
    brush().strip(track, low, high, colors, "Scatter x-window brush")
}

/// Resolve the theme into a [`ChartStyle`]. Only colours are overridden — the
/// margins / tick targets stay the defaults, which is what lets the a11y readout
/// (computed with the default style) resolve the identical focus point.
fn chart_style(theme: &pinion_core::Theme) -> ChartStyle {
    ChartStyle {
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        crosshair: theme.resolve(ColorRole::OnSurface),
        tooltip_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        tooltip_fg: theme.resolve(ColorRole::OnSurface),
        ..ChartStyle::default()
    }
}

/// view-fn (§6.3): pure sync mapping. `scrub` is the inspect fraction across
/// [`CHART_RECT`] that scrubs the x-axis; `(low, high)` is the brushed x-window
/// that cross-filters the point marks (R1391).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: SliderState, scrub: f32, low: f32, high: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let (x_lo, x_hi) = brush_domain(low, high);

    let title = Scene::Text(
        TextNode::styled(
            "Two samples — drag the plot to inspect, the strip to brush-filter",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, 12)),
    );

    // R1391 — the brush window cross-filters the SCATTER: points outside
    // `[x_lo, x_hi]` mute (dimmed, still drawn), so brushing the overview strip
    // highlights the corresponding points in the plot above.
    let scatter = ScatterChart::new(samples())
        .inspect(Some(scrub))
        .select_x_range(Some((x_lo, x_hi)))
        .build(CHART_RECT, &chart_style(&theme));

    let brush = brush_strip(&theme, low, high);

    // Transparent capture surface over the plot — the `scatter_scrub` primary
    // tag. On top so a press anywhere on the chart drives the scrub; transparent
    // so the points show through, pointer-opaque so it captures.
    // R1417 capture_surface lift.
    let scrub_surface = capture_surface(SCRUB_TAG, CHART_RECT, false);

    let status = Scene::Text(
        TextNode::styled(
            format!(
                "{} | scrub {scrub:.2} | brush x {x_lo:.1}..{x_hi:.1}",
                state.as_name()
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, WIN_H - 22)),
    );

    Scene::Container(
        ContainerNode::new(vec![scatter, scrub_surface, brush, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. The Slider is the scrub position (primary); the
/// RangeSlider is the brush window (R1391 sibling external). `#[widget]` derives
/// WidgetCore + WidgetView, and `a11y_manual` provides the hand-written
/// [`WidgetA11y`] below (the scrub Slider describedby the inspect region).
#[widget(
    tag = "scatter_scrub",
    state = (SliderState, f32, f32, f32),
    event = SliderEvent,
    title = "pinion hello-scatter (R1377 scatter + scrub + R1391 brush-filter)",
    renderer = HelloScatterRenderer,
    initial_size = (WIN_W, WIN_H),
    external = scrub_external,
    extra_externals = brush_extras,
    apply_key,
    keybinding,
    event_name_derive,
    a11y_manual,
)]
struct ScatterView;

impl ScatterView {
    /// Reads both externals by tag — the scrub fraction (primary Slider) and the
    /// brush window (sibling `RangeSlider`). The `Container([primary, ...extras])`
    /// shape extras impose rules out the derived single-External read.
    fn read_state(scene: &Scene) -> (SliderState, f32, f32, f32) {
        let (state, scrub) =
            read_slider_state(scene, SCRUB_TAG).unwrap_or((SliderState::Idle, 0.5));
        let (low, high) = read_brush(scene);
        (state, scrub, low, high)
    }

    fn view(state: (SliderState, f32, f32, f32), frame: Frame) -> Scene {
        view(state.0, state.1, state.2, state.3, &frame)
    }

    /// ARIA slider keyboard scrub, mirrored through the RPC `scene/intervene`
    /// value channel (the lifted `slider_apply_key`).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        slider_apply_key(scene, focused, Self::tag(), |current| match key {
            "ArrowLeft" | "ArrowDown" => Some((current - 0.1).clamp(0.0, 1.0)),
            "ArrowRight" | "ArrowUp" => Some((current + 0.1).clamp(0.0, 1.0)),
            "Home" => Some(0.0),
            "End" => Some(1.0),
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
}

/// The scrub surface carries `AriaRole::Slider` with the scrub fraction as its
/// `AccessValue::Float`, and is `describedby` the scatter's inspect region so the
/// x + per-series values a sighted user reads in the tooltip reach a screen
/// reader too (the R1355 parity, now for scatter).
impl WidgetA11y for ScatterView {
    fn access_node(state: &(SliderState, f32, f32, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, scrub, low, high) = (state.0, state.1, state.2, state.3);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            ..AccessState::from_interaction(interaction, None)
        };
        // The readout must name the SAME point the painted tooltip does. Unlike
        // the donut's slice scrub (which is rect- and style-invariant), a
        // scatter focus DOES depend on the plot geometry — but the geometry is
        // set by the rect + margins + tick targets + data, NOT the colours, and
        // `chart_style` overrides only colours. So this `CHART_RECT` /
        // default-style call resolves the identical point the themed
        // `build(CHART_RECT)` overlay rings. (If a future `chart_style` ever
        // overrode `margin` / `x_ticks`, this call would have to take the themed
        // style too — the R1355 same-frame parity.)
        let readout = ScatterChart::new(samples())
            .inspect(Some(scrub))
            .inspect_readout(CHART_RECT, &ChartStyle::default());
        // R1692 — a transparent capture surface has no contents to be named
        // from, so an unauthored name reaches a reader as "slider" and nothing.
        let control = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Slider)
            .with_name("Scrub position".to_owned())
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
        // The brush window is a sibling external (R1391). A two-thumb range has
        // no single-Float shape, so `AccessValue::Text` states the filtered
        // x-window plainly rather than leaving the cross-filter inaudible.
        let (x_lo, x_hi) = brush_domain(low, high);
        nodes.push(
            AccessNode::new(BRUSH_TAG, AriaRole::Slider)
                .with_name("Scatter x-window brush".to_string())
                .with_value(AccessValue::Text(format!("x from {x_lo:.1} to {x_hi:.1}"))),
        );
        nodes
    }
}

fn main() {
    pinion_shell::run::<ScatterView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_core::style::Color;

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

    fn count_prefix(scene: &Scene, prefix: &str) -> usize {
        let mut n = 0;
        if scene.tag().is_some_and(|t| t.starts_with(prefix)) {
            n += 1;
        }
        if let Scene::Container(c) = scene {
            for ch in &c.children {
                n += count_prefix(ch, prefix);
            }
        }
        n
    }

    fn rendered(scrub: f32) -> Scene {
        rendered_brushed(scrub, 0.0, 1.0)
    }

    fn rendered_brushed(scrub: f32, low: f32, high: f32) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(SliderState::Idle, scrub, low, high, &Frame::new()))
    }

    #[test]
    fn scatter_and_scrub_surface_present() {
        let scene = rendered(0.5);
        assert!(find(&scene, "chart").is_some(), "the scatter root");
        assert!(
            find(&scene, SCRUB_TAG).is_some(),
            "the scrub capture surface"
        );
        // 8 + 8 = 16 points across the two series.
        assert_eq!(count_prefix(&scene, "chart.point."), 16);
    }

    #[test]
    fn inspect_overlay_tracks_the_scrub_value() {
        // A left-edge scrub and a right-edge scrub focus different x's, so the
        // crosshair (a vertical line at the focus x) moves.
        use pinion_core::scene::PathCommand;
        let crosshair_x = |scrub: f32| {
            let scene = rendered(scrub);
            let Scene::Path(p) = find(&scene, "chart.inspect.crosshair").expect("crosshair") else {
                panic!("crosshair is a path")
            };
            let PathCommand::MoveTo(m) = p.commands[0] else {
                panic!("crosshair starts with MoveTo")
            };
            m.x.to_bits()
        };
        assert_ne!(
            crosshair_x(0.0),
            crosshair_x(1.0),
            "the crosshair sits at a different x at each end of the scrub"
        );
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ScatterView>(
            (SliderState::Idle, 0.5, 0.0, 1.0),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<ScatterView>(
            (SliderState::Idle, 0.5, 0.0, 1.0),
            &Frame::new(),
        );
    }

    #[test]
    fn scrub_reports_slider_role_and_is_describedby_the_readout() {
        let nodes =
            <ScatterView as WidgetA11y>::access_node(&(SliderState::Idle, 0.5, 0.0, 1.0), None);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert_eq!(nodes[0].tag, SCRUB_TAG);
        assert_eq!(
            nodes[0].described_by.as_deref(),
            Some("chart.inspect.tooltip"),
            "scrub is describedby the inspect region"
        );
        let region = nodes
            .iter()
            .find(|n| n.tag == "chart.inspect.tooltip")
            .expect("the described region is in the tree");
        let name = region.name.as_deref().expect("region carries the readout");
        assert!(
            name.starts_with("x = "),
            "the readout leads with the focus x: {name:?}"
        );
        assert!(
            name.contains("rising") && name.contains("falling"),
            "the readout names both series: {name:?}"
        );
    }

    // ── R1391 numeric brush-range cross-filter ────────────────────────

    /// A point mark's fill colour (each point is a filled circle `Scene::Path`).
    fn point_fill(scene: &Scene, tag: &str) -> Color {
        let Scene::Path(p) = find(scene, tag).expect("a point") else {
            panic!("a point mark is a path")
        };
        p.style.fill.expect("a point is filled")
    }

    #[test]
    fn r1391_brush_mutes_out_of_range_points_but_keeps_them_drawn() {
        // Brush the lower half (fractions 0..0.5 -> x in ~[1, 4.75]): rising's
        // point 0 (x=1) is inside the window, point 7 (x=8) is outside.
        let scene = rendered_brushed(0.5, 0.0, 0.5);
        // Muting DIMS, it does not DROP — all 16 marks still emit a node.
        assert_eq!(
            count_prefix(&scene, "chart.point."),
            16,
            "every point stays drawn"
        );
        assert_ne!(
            point_fill(&scene, "chart.point.0.0"),
            point_fill(&scene, "chart.point.0.7"),
            "an in-range and an out-of-range point of the SAME series differ (one muted)",
        );
    }

    #[test]
    fn r1391_full_brush_filters_nothing() {
        // The boot / full-span brush leaves every point at full colour, so two
        // points of one series share the identical fill.
        let scene = rendered_brushed(0.5, 0.0, 1.0);
        assert_eq!(
            point_fill(&scene, "chart.point.0.0"),
            point_fill(&scene, "chart.point.0.7"),
            "full span = no filter => same-series fills are equal",
        );
    }

    #[test]
    fn r1391_brush_strip_present_and_announced() {
        let scene = rendered_brushed(0.5, 0.2, 0.6);
        assert!(
            find(&scene, BRUSH_TAG).is_some(),
            "the brush strip is in the scene"
        );
        let nodes =
            <ScatterView as WidgetA11y>::access_node(&(SliderState::Idle, 0.5, 0.2, 0.6), None);
        let brush = nodes
            .iter()
            .find(|n| n.tag == BRUSH_TAG)
            .expect("the brush a11y node");
        assert_eq!(brush.role, AriaRole::Slider);
        assert_eq!(
            brush.name.as_deref(),
            Some("Scatter x-window brush"),
            "the brush announces itself",
        );
    }
}
