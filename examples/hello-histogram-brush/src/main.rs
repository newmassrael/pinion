//! `hello-histogram-brush` — one brush **cross-filters two different chart
//! GEOMETRIES** (R1395).
//!
//! The completing leg of the cross-filter matrix: categorical bar-click
//! (R1384), numeric same-type scatter brush (R1391), arbitrary legend (R1392),
//! numeric cross-TYPE line + scatter (R1394), and now — R1395 — a numeric brush
//! over a DIFFERENT geometry. A single x-window brush drives a
//! [`ScatterChart`] on top (numeric point marks) AND a [`BarChart`] histogram
//! below (a categorical bar layout): dragging the overview strip narrows the
//! window, and BOTH panels dim the marks whose x falls outside it — the scatter
//! its points (their `x` outside the window), the histogram its BINS (whose
//! numeric `[lo, hi)` extent does not overlap the window). Muted marks stay
//! drawn as context.
//!
//! ## The DIFFERENT-geometry leg: [`BarChart::select_x_range`]
//!
//! A histogram bar is laid out CATEGORICALLY (evenly-spaced slots), but a
//! histogram bin genuinely covers a numeric interval `[k, k+1)`. R1395 gives
//! each [`Bar`] an optional numeric [`bin`](Bar::bin) extent (via
//! [`Bar::with_bin`]) and adds [`BarChart::select_x_range`] — the numeric-range
//! peer of [`ScatterChart::select_x_range`] (R1391), but muting a
//! categorical-layout bar rather than a positioned point. So the SAME window
//! feeds both panels' `select_x_range`: one numeric brush, two unlike
//! geometries.
//!
//! ## Aligned by construction
//!
//! Both panels share the x-axis `[0, 12]` and the window edges sit on integers,
//! so a scatter point at bucket centre `k + 0.5` and the histogram bin `[k,
//! k+1)` are classified identically by any integer-edged window — the two
//! panels dim exactly the same buckets, which is what makes the cross-filter
//! read as one coherent selection.
//!
//! The brush is a sibling [`RangeSliderExternal`](pinion_core::widgets::range_slider::RangeSliderExternal)
//! (the R1249 `extra_externals` slot), the **fourth** consumer of the
//! [`Brush`] substrate lifted at R1394 — it only CALLS the SSOT
//! (`extras`/`read`/`domain`/`strip`), no new wiring. A primary
//! [`SliderExternal`](pinion_core::widgets::slider) scrub over the scatter is
//! the secondary interaction (the R1355 inspector).
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes both panels as tagged data under distinct prefixes
//! (`scatter.*` / `hist.*`) so the two never collide: the scatter's
//! `scatter.point.{i}.{j}` and the histogram's `hist.bar.{k}`. Driving the
//! brush over RPC (`scene/intervene /hist_brush/external/{low,high}`) re-mutes
//! both panels, observed structurally without OCR (§2 #1 / #7). See
//! `tools/demos/r1395_histogram_brush.py`.

use pinion_a11y::described::describedby_region;
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{
    Bar, BarChart, Brush, BrushStripColors, ChartStyle, DataPoint, ScatterChart, Series,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::slider::{SliderEvent, SliderExternal, SliderState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::slider::{read_slider_state, slider_apply_key};

// pinion-forge codegen (build.rs -> $OUT_DIR/app.rs): the
// `HelloHistogramBrushRenderer` Vello wrapper struct + its error type.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(
    HelloHistogramBrushRenderer,
    HelloHistogramBrushRendererError
);

const WIN_W: u32 = 760;
const WIN_H: u32 = 560;

const THEME_TAG: &str = "app";
/// Primary Slider: a scrub over the scatter (the R1355 inspector).
const SCRUB_TAG: &str = "hist_scrub";
/// Sibling `RangeSlider`: the shared x-window brush (the R1394 [`Brush`]).
const BRUSH_TAG: &str = "hist_brush";
const BRUSH_H: u32 = 14;
const BRUSH_GAP: u32 = 8;

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// The shared data x-extent both panels are pinned to, so one brush fraction
/// maps to the identical x-slice on each: the scatter's numeric x-domain and
/// the histogram's `N_BINS` unit bins tile it exactly. Hand-stated because it
/// is a contract *between* the two datasets, not a property of either alone.
const X_DOMAIN: (f64, f64) = (0.0, 12.0);
/// The histogram's bin count — one unit-width bin `[k, k+1)` per time bucket
/// across [`X_DOMAIN`], so the two panels' buckets line up.
const N_BINS: usize = 12;

/// The scatter panel (top) — also the scrub capture basis: the `hist_scrub`
/// box covers exactly this rect, so the slider value `0.0..=1.0` is the cursor
/// fraction across it.
const SCATTER_RECT: Rect = Rect::new(10, 44, WIN_W - 20, 190);
/// The histogram panel (bottom) — window-absolute, pinned (`build`).
const HIST_RECT: Rect = Rect::new(10, 254, WIN_W - 20, 190);

/// The histogram (bottom) — an error-count distribution over the `N_BINS` time
/// buckets, each bar a numeric bin `[k, k+1)` so the brush can cross-filter it
/// ([`Bar::with_bin`]). This is the x-DISTRIBUTION the same window filters.
#[allow(
    clippy::cast_precision_loss,
    reason = "the bin index (0..12) -> f64 is exact"
)]
fn histogram() -> Vec<Bar> {
    let counts = [
        3.0, 5.0, 8.0, 12.0, 15.0, 18.0, 16.0, 13.0, 9.0, 6.0, 4.0, 2.0,
    ];
    counts
        .iter()
        .take(N_BINS)
        .enumerate()
        .map(|(k, &v)| Bar::new(k.to_string(), v).with_bin(k as f64, (k + 1) as f64))
        .collect()
}

/// The scatter panel's samples (top) — one representative `(x, y)` per time
/// bucket, plotted at the bucket CENTRE `k + 0.5` so an integer-edged brush
/// window classifies a point and its matching histogram bin identically.
fn samples() -> Vec<Series> {
    let throughput = [2.0, 3.5, 5.0, 6.5, 8.0, 9.5, 8.5, 7.0, 5.5, 4.0, 3.0, 2.5];
    #[allow(
        clippy::cast_precision_loss,
        reason = "the bucket index (0..12) -> f64 is exact"
    )]
    let points = throughput
        .iter()
        .take(N_BINS)
        .enumerate()
        .map(|(k, &y)| DataPoint::new(k as f64 + 0.5, y))
        .collect();
    vec![Series::new("throughput", points)]
}

/// Seeded to the plot centre so the boot frame shows an inspected point.
fn scrub_external() -> SliderExternal {
    let mut slider = SliderExternal::new();
    slider.set_value(0.5);
    slider
}

/// The shared x-window brush — the [`Brush`] substrate (R1394), whose FOURTH
/// consumer this is (a call to the lifted SSOT, not a new lift).
fn brush() -> Brush {
    Brush::new(BRUSH_TAG, X_DOMAIN)
}

/// R1395 — the brush window as a sibling `External` ([`Brush::extras`]); the fn
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

/// Map the brush fractions onto the shared data x-extent ([`Brush::domain`]) —
/// the window that feeds both panels' `select_x_range`.
fn brush_domain(low: f32, high: f32) -> (f64, f64) {
    brush().domain(low, high)
}

/// Resolve the theme into a [`ChartStyle`] shared by both panels.
fn chart_style(theme: &pinion_core::Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        crosshair: theme.resolve(ColorRole::OnSurface),
        tooltip_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        tooltip_fg: theme.resolve(ColorRole::OnSurface),
        ..ChartStyle::default()
    }
}

/// The brush strip under the histogram ([`Brush::strip`]), aligned to the
/// shared x-axis so it reads as an overview of both panels at once.
fn brush_strip(theme: &pinion_core::Theme, low: f32, high: f32) -> Scene {
    let track = Rect::new(
        HIST_RECT.x,
        HIST_RECT.y + HIST_RECT.h + BRUSH_GAP,
        HIST_RECT.w,
        BRUSH_H,
    );
    let colors = BrushStripColors {
        track_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        accent: theme.resolve(ColorRole::Accent),
    };
    brush().strip(track, low, high, colors, "Histogram x-window brush")
}

/// view-fn (§6.3): pure sync mapping. `scrub` is the inspect fraction across
/// [`SCATTER_RECT`]; `(low, high)` is the brushed x-window that cross-filters
/// BOTH the scatter points and the histogram bins (R1395).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: SliderState, scrub: f32, low: f32, high: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let style = chart_style(&theme);
    let (x_lo, x_hi) = brush_domain(low, high);

    let title = Scene::Text(
        TextNode::styled(
            "One brush, two geometries — drag the strip to cross-filter scatter + histogram",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, 12)),
    );

    // The scatter panel (top): points outside the window mute (R1391), and the
    // scrub rings the nearest point (R1355).
    let scatter = ScatterChart::new(samples())
        .with_tag_prefix("scatter")
        .with_x_domain(X_DOMAIN.0, X_DOMAIN.1)
        .inspect(Some(scrub))
        .select_x_range(Some((x_lo, x_hi)))
        .build(SCATTER_RECT, &style);

    // The histogram panel (bottom): the bins whose numeric `[k, k+1)` extent
    // falls outside the window mute (R1395 `BarChart::select_x_range`) — the
    // DIFFERENT geometry the same window filters.
    let hist = BarChart::new(histogram())
        .with_tag_prefix("hist")
        .select_x_range(Some((x_lo, x_hi)))
        .build(HIST_RECT, &style);

    // Transparent capture surface over the scatter — the `hist_scrub` primary
    // tag. On top so a press drives the scrub; transparent so the points show
    // through, pointer-opaque so it captures.
    // R1417 capture_surface lift.
    let scrub_surface = capture_surface(SCRUB_TAG, SCATTER_RECT, false);

    let brush = brush_strip(&theme, low, high);

    let status = Scene::Text(
        TextNode::styled(
            format!(
                "{} | scrub {scrub:.2} | brush x {x_lo:.1}..{x_hi:.1} filters scatter + histogram",
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
        ContainerNode::new(vec![scatter, hist, scrub_surface, brush, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. The Slider is the scrub position (primary); the
/// RangeSlider is the shared brush window (R1395 sibling external).
/// `a11y_manual` provides the hand-written [`WidgetA11y`] (the scrub Slider
/// describedby the scatter inspect region).
#[widget(
    tag = "hist_scrub",
    state = (SliderState, f32, f32, f32),
    event = SliderEvent,
    title = "pinion hello-histogram-brush (R1395 numeric different-geometry cross-filter)",
    renderer = HelloHistogramBrushRenderer,
    initial_size = (WIN_W, WIN_H),
    external = scrub_external,
    extra_externals = brush_extras,
    apply_key,
    keybinding,
    event_name_derive,
    a11y_manual,
)]
struct HistogramBrushView;

impl HistogramBrushView {
    /// Reads both externals by tag — the scrub fraction (primary Slider) and the
    /// brush window (sibling `RangeSlider`). The `Container([primary, ...extras])`
    /// shape rules out the derived single-External read.
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
/// `AccessValue::Float`, describedby the scatter's inspect region (R1355
/// parity); the brush window states its filtered x-window as `AccessValue::Text`
/// so the cross-filter is not inaudible.
impl WidgetA11y for HistogramBrushView {
    fn access_node(state: &(SliderState, f32, f32, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, scrub, low, high) = (state.0, state.1, state.2, state.3);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            ..AccessState::from_interaction(interaction, None)
        };
        // The readout names the SAME point the painted tooltip does: the scatter
        // focus depends on the plot geometry (rect + margins + ticks + data +
        // domain), NOT the colours, so this default-style call over the pinned
        // `SCATTER_RECT` resolves the identical point the themed overlay rings.
        let readout = ScatterChart::new(samples())
            .with_tag_prefix("scatter")
            .with_x_domain(X_DOMAIN.0, X_DOMAIN.1)
            .inspect(Some(scrub))
            .inspect_readout(SCATTER_RECT, &ChartStyle::default());
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
            "scatter.inspect.tooltip",
            AriaRole::Tooltip,
            readout,
            true,
        );
        let (x_lo, x_hi) = brush_domain(low, high);
        nodes.push(
            AccessNode::new(BRUSH_TAG, AriaRole::Slider)
                .with_name("Histogram x-window brush".to_string())
                .with_value(AccessValue::Text(format!("x from {x_lo:.1} to {x_hi:.1}"))),
        );
        nodes
    }
}

fn main() {
    pinion_shell::run::<HistogramBrushView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

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

    /// A `hist.bar.{k}` box's fill alpha — 255 full, `MUTED_ALPHA` when the bin
    /// is outside the brush window.
    fn bar_alpha(scene: &Scene, k: usize) -> u8 {
        let Some(Scene::Box(b)) = find(scene, &format!("hist.bar.{k}")) else {
            panic!("hist.bar.{k} is a box")
        };
        b.style.fill.a
    }

    /// A `scatter.point.0.{j}` circle's fill alpha.
    fn point_alpha(scene: &Scene, j: usize) -> u8 {
        let Some(Scene::Path(p)) = find(scene, &format!("scatter.point.0.{j}")) else {
            panic!("scatter.point.0.{j} is a path")
        };
        p.style.fill.expect("a point is filled").a
    }

    fn rendered(low: f32, high: f32) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(SliderState::Idle, 0.5, low, high, &Frame::new()))
    }

    #[test]
    fn both_panels_and_the_brush_strip_are_present() {
        let scene = rendered(0.0, 1.0);
        assert!(
            find(&scene, "scatter").is_some(),
            "the scatter panel is present"
        );
        assert!(
            find(&scene, "hist").is_some(),
            "the histogram panel is present"
        );
        assert!(
            find(&scene, BRUSH_TAG).is_some(),
            "the brush strip is present"
        );
        assert!(
            find(&scene, SCRUB_TAG).is_some(),
            "the scrub surface is present"
        );
    }

    #[test]
    fn panels_use_distinct_prefixes_so_tags_never_collide() {
        // Both charts default to the "chart" prefix; this demo re-prefixes them
        // so a snapshot can address each panel unambiguously.
        let scene = rendered(0.0, 1.0);
        assert_eq!(
            count_prefix(&scene, "chart."),
            0,
            "no node keeps the default prefix"
        );
        assert!(
            count_prefix(&scene, "scatter.") > 0,
            "scatter panel tags exist"
        );
        assert!(
            count_prefix(&scene, "hist.") > 0,
            "histogram panel tags exist"
        );
    }

    #[test]
    fn a_full_brush_filters_neither_panel() {
        // The boot / full-span window leaves every histogram bin and scatter
        // point at full alpha.
        let scene = rendered(0.0, 1.0);
        for k in 0..N_BINS {
            assert_eq!(bar_alpha(&scene, k), 255, "full span: bin {k} full");
            assert_eq!(point_alpha(&scene, k), 255, "full span: point {k} full");
        }
    }

    #[test]
    fn a_narrow_brush_cross_filters_both_geometries_identically() {
        // A window over x in [3, 6] (fractions 0.25..0.5) keeps buckets 3,4,5 and
        // mutes the rest — in BOTH panels, by construction (point centre k+0.5,
        // bin [k, k+1)). One window, two geometries, the same buckets.
        let scene = rendered(0.25, 0.5);
        for k in 3..=5 {
            assert_eq!(bar_alpha(&scene, k), 255, "in-window bin {k} full");
            assert_eq!(point_alpha(&scene, k), 255, "in-window point {k} full");
        }
        for k in [0, 1, 2, 6, 7, 8, 9, 10, 11] {
            assert!(bar_alpha(&scene, k) < 255, "out-of-window bin {k} muted");
            assert!(
                point_alpha(&scene, k) < 255,
                "out-of-window point {k} muted"
            );
        }
    }

    #[test]
    fn muting_dims_it_never_drops_a_bin_or_a_point() {
        // Every bar and point still emits a node when muted (unlike a pinned
        // domain, which would drop out-of-domain marks).
        let scene = rendered(0.25, 0.5);
        assert_eq!(
            count_prefix(&scene, "hist.bar."),
            N_BINS,
            "every histogram bin stays drawn"
        );
        assert_eq!(
            count_prefix(&scene, "scatter.point.0."),
            N_BINS,
            "every scatter point stays drawn"
        );
    }

    #[test]
    fn the_filter_shifts_with_the_window() {
        // Slide the window up to x in [6, 9]: buckets 6,7,8 go full, the earlier
        // in-window buckets 3,4,5 now mute — on both panels together.
        let scene = rendered(0.5, 0.75);
        for k in 6..=8 {
            assert_eq!(bar_alpha(&scene, k), 255, "shifted-in bin {k} full");
            assert_eq!(point_alpha(&scene, k), 255, "shifted-in point {k} full");
        }
        for k in 3..=5 {
            assert!(bar_alpha(&scene, k) < 255, "shifted-out bin {k} muted");
            assert!(point_alpha(&scene, k) < 255, "shifted-out point {k} muted");
        }
    }

    #[test]
    fn the_brush_window_is_announced() {
        let nodes = HistogramBrushView::access_node(&(SliderState::Idle, 0.5, 0.25, 0.5), None);
        let brush = nodes
            .iter()
            .find(|n| n.tag == BRUSH_TAG)
            .expect("the brush a11y node");
        assert_eq!(brush.role, AriaRole::Slider);
        assert_eq!(brush.name.as_deref(), Some("Histogram x-window brush"));
        let Some(AccessValue::Text(text)) = brush.value.as_ref() else {
            panic!("brush value is Text, got {:?}", brush.value)
        };
        assert!(text.starts_with("x from"), "states its window: {text:?}");
    }

    #[test]
    fn view_carries_the_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<HistogramBrushView>(
            (SliderState::Idle, 0.5, 0.0, 1.0),
            &Frame::new(),
        );
    }
}
