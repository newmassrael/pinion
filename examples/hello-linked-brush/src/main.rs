//! `hello-linked-brush` — one brush **cross-filters two different chart
//! types** (R1394).
//!
//! A single x-window brush drives the *numeric cross-type* cross-filter: the
//! completing leg of the cross-filter matrix (categorical bar-click R1384,
//! numeric same-type scatter brush R1391, arbitrary legend R1392). Dragging
//! the overview strip narrows the window, and BOTH panels react — a
//! [`LineChart`] on top and a [`ScatterChart`] below dim every mark whose x
//! falls outside it (their marks stay drawn as context), so a selection in one
//! control emphasises the same x-slice across two unlike widgets.
//!
//! ## Two `select_x_range`s, one window
//!
//! The brush is a sibling [`RangeSliderExternal`](pinion_core::widgets::range_slider::RangeSliderExternal)
//! (the R1249 `extra_externals` slot), wired through the lifted [`Brush`]
//! substrate — the **third** brush consumer, which is what triggered the lift
//! out of `hello-chart` (R1357 zoom) and `hello-scatter` (R1391 same-type
//! cross-filter). Its `(low, high)` fractions map once, through
//! [`Brush::domain`], onto the shared data x-extent, and the resulting
//! `(x_lo, x_hi)` feeds both [`LineChart::select_x_range`] (new in R1394 — a
//! muted context line with a full-colour in-window overdraw) and
//! [`ScatterChart::select_x_range`] (R1391 — muted context points).
//!
//! A primary [`SliderExternal`](pinion_core::widgets::slider) scrub over the
//! scatter is the secondary interaction (the R1355 inspector), so the primary
//! surface stays meaningful while the brush does the cross-filtering.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes both panels as tagged data under distinct prefixes
//! (`line.*` / `scatter.*`) so the two never collide: the line's muted context
//! `line.series.{i}` plus its focus overdraw `line.focus.series.{i}`, and the
//! scatter's `scatter.point.{i}.{j}`. Driving the brush over RPC
//! (`scene/intervene /linked_brush/external/{low,high}`) re-mutes both panels,
//! observed structurally without OCR (§2 #1 / #7). See
//! `tools/demos/r1394_linked_brush.py`.

use pinion_a11y::described::describedby_region;
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{
    Brush, BrushStripColors, ChartStyle, DataPoint, LineChart, ScatterChart, Series,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::slider::{SliderEvent, SliderExternal, SliderState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::slider::{read_slider_state, slider_apply_key};

// pinion-forge codegen (build.rs -> $OUT_DIR/app.rs): the `HelloLinkedBrushRenderer`
// Vello wrapper struct + its error type.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloLinkedBrushRenderer, HelloLinkedBrushRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 560;

const THEME_TAG: &str = "app";
/// Primary Slider: a scrub over the scatter (the R1355 inspector).
const SCRUB_TAG: &str = "linked_scrub";
/// Sibling `RangeSlider`: the shared x-window brush (the lifted [`Brush`]).
const BRUSH_TAG: &str = "linked_brush";
const BRUSH_H: u32 = 14;
const BRUSH_GAP: u32 = 8;

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// The shared data x-extent both panels are pinned to, so one brush fraction
/// maps to the identical x-slice on each. Hand-stated because it is a contract
/// *between* the two datasets, not a property of either alone.
const X_DOMAIN: (f64, f64) = (0.0, 11.0);

/// The line panel (top) — window-absolute, pinned (`build`).
const LINE_RECT: Rect = Rect::new(10, 44, WIN_W - 20, 190);
/// The scatter panel (bottom) — also the scrub capture basis: the
/// `linked_scrub` box covers exactly this rect, so the slider value
/// `0.0..=1.0` is the cursor fraction across it.
const SCATTER_RECT: Rect = Rect::new(10, 254, WIN_W - 20, 190);

/// The line panel's trend — one aggregate series over the shared x-axis.
fn trend() -> Vec<Series> {
    let throughput = [
        820.0, 910.0, 1150.0, 1400.0, 1320.0, 1600.0, 2100.0, 2400.0, 2200.0, 1900.0, 2600.0,
        3100.0,
    ];
    #[allow(
        clippy::cast_precision_loss,
        reason = "bucket index (0..12) -> f64 x is exact"
    )]
    let points = throughput
        .iter()
        .enumerate()
        .map(|(i, &y)| DataPoint::new(i as f64, y))
        .collect();
    vec![Series::new("throughput", points)]
}

/// The scatter panel's raw samples — two clouds over the SAME x-axis, so the
/// shared brush window selects the matching slice of both panels.
fn samples() -> Vec<Series> {
    vec![
        Series::new(
            "sensor A",
            vec![
                DataPoint::new(0.0, 2.0),
                DataPoint::new(1.0, 3.5),
                DataPoint::new(2.0, 3.0),
                DataPoint::new(3.0, 5.0),
                DataPoint::new(4.0, 4.5),
                DataPoint::new(5.0, 6.5),
                DataPoint::new(6.0, 7.0),
                DataPoint::new(7.0, 6.0),
                DataPoint::new(8.0, 8.5),
                DataPoint::new(9.0, 8.0),
                DataPoint::new(10.0, 9.5),
                DataPoint::new(11.0, 9.0),
            ],
        ),
        Series::new(
            "sensor B",
            vec![
                DataPoint::new(0.5, 9.0),
                DataPoint::new(1.5, 8.0),
                DataPoint::new(2.5, 8.5),
                DataPoint::new(3.5, 6.5),
                DataPoint::new(4.5, 7.0),
                DataPoint::new(5.5, 5.0),
                DataPoint::new(6.5, 5.5),
                DataPoint::new(7.5, 4.0),
                DataPoint::new(8.5, 3.5),
                DataPoint::new(9.5, 2.5),
                DataPoint::new(10.5, 3.0),
            ],
        ),
    ]
}

/// Seeded to the plot centre so the boot frame shows an inspected point.
fn scrub_external() -> SliderExternal {
    let mut slider = SliderExternal::new();
    slider.set_value(0.5);
    slider
}

/// The shared x-window brush — the lifted [`Brush`] substrate (R1394), the
/// third consumer after `hello-chart` and `hello-scatter`.
fn brush() -> Brush {
    Brush::new(BRUSH_TAG, X_DOMAIN)
}

/// R1394 — the brush window as a sibling `External` ([`Brush::extras`]); the fn
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

/// The brush strip under the scatter ([`Brush::strip`]), aligned to the shared
/// x-axis so it reads as an overview of both panels at once.
fn brush_strip(theme: &pinion_core::Theme, low: f32, high: f32) -> Scene {
    let track = Rect::new(
        SCATTER_RECT.x,
        SCATTER_RECT.y + SCATTER_RECT.h + BRUSH_GAP,
        SCATTER_RECT.w,
        BRUSH_H,
    );
    let colors = BrushStripColors {
        track_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        accent: theme.resolve(ColorRole::Accent),
    };
    brush().strip(track, low, high, colors, "Linked x-window brush")
}

/// view-fn (§6.3): pure sync mapping. `scrub` is the inspect fraction across
/// [`SCATTER_RECT`]; `(low, high)` is the brushed x-window that cross-filters
/// BOTH panels (R1394).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: SliderState, scrub: f32, low: f32, high: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let style = chart_style(&theme);
    let (x_lo, x_hi) = brush_domain(low, high);

    let title = Scene::Text(
        TextNode::styled(
            "One brush, two chart types — drag the strip to cross-filter both",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, 12)),
    );

    // The line panel: its out-of-window portion mutes to a context ghost while
    // the in-window slice keeps full colour (R1394 `LineChart::select_x_range`).
    let line = LineChart::new(trend())
        .with_tag_prefix("line")
        .filled(true)
        .with_x_domain(X_DOMAIN.0, X_DOMAIN.1)
        .select_x_range(Some((x_lo, x_hi)))
        .build(LINE_RECT, &style);

    // The scatter panel: points outside the window mute (R1391), and the scrub
    // rings the nearest point (R1355).
    let scatter = ScatterChart::new(samples())
        .with_tag_prefix("scatter")
        .with_x_domain(X_DOMAIN.0, X_DOMAIN.1)
        .inspect(Some(scrub))
        .select_x_range(Some((x_lo, x_hi)))
        .build(SCATTER_RECT, &style);

    // Transparent capture surface over the scatter — the `linked_scrub` primary
    // tag. On top so a press drives the scrub; transparent so the points show
    // through, pointer-opaque so it captures.
    let scrub_surface = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
            .with_tag(SCRUB_TAG)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(SCATTER_RECT.x, SCATTER_RECT.y)
                    .with_size(Size::px(SCATTER_RECT.w, SCATTER_RECT.h)),
            ),
    );

    let brush = brush_strip(&theme, low, high);

    let status = Scene::Text(
        TextNode::styled(
            format!(
                "{} | scrub {scrub:.2} | brush x {x_lo:.1}..{x_hi:.1} filters line + scatter",
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
        ContainerNode::new(vec![line, scatter, scrub_surface, brush, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. The Slider is the scrub position (primary); the
/// RangeSlider is the shared brush window (R1394 sibling external). `a11y_manual`
/// provides the hand-written [`WidgetA11y`] (the scrub Slider describedby the
/// scatter inspect region).
#[widget(
    tag = "linked_scrub",
    state = (SliderState, f32, f32, f32),
    event = SliderEvent,
    title = "pinion hello-linked-brush (R1394 numeric cross-type cross-filter)",
    renderer = HelloLinkedBrushRenderer,
    initial_size = (WIN_W, WIN_H),
    external = scrub_external,
    extra_externals = brush_extras,
    apply_key,
    keybinding,
    event_name_derive,
    a11y_manual,
)]
struct LinkedBrushView;

impl LinkedBrushView {
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
impl WidgetA11y for LinkedBrushView {
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
        let control = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Slider)
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
                .with_name("Linked x-window brush".to_string())
                .with_value(AccessValue::Text(format!("x from {x_lo:.1} to {x_hi:.1}"))),
        );
        nodes
    }
}

fn main() {
    pinion_shell::run::<LinkedBrushView>();
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

    fn rendered(low: f32, high: f32) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(SliderState::Idle, 0.5, low, high, &Frame::new()))
    }

    #[test]
    fn both_panels_and_the_brush_strip_are_present() {
        let scene = rendered(0.0, 1.0);
        assert!(find(&scene, "line").is_some(), "the line panel is present");
        assert!(
            find(&scene, "scatter").is_some(),
            "the scatter panel is present"
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
        assert!(count_prefix(&scene, "line.") > 0, "line panel tags exist");
        assert!(
            count_prefix(&scene, "scatter.") > 0,
            "scatter panel tags exist"
        );
    }

    #[test]
    fn a_full_brush_filters_neither_panel() {
        // The boot / full-span window leaves the line un-muted (no focus
        // overdraw) and every scatter point full.
        let scene = rendered(0.0, 1.0);
        assert_eq!(
            count_prefix(&scene, "line.focus."),
            0,
            "full span: no line focus overdraw"
        );
    }

    #[test]
    fn a_narrow_brush_cross_filters_both_panels() {
        // A window over the middle mutes the line into a context ghost + a focus
        // overdraw, and mutes the scatter points outside it — one window, two
        // chart types (R1394).
        let scene = rendered(0.3, 0.6);
        assert!(
            count_prefix(&scene, "line.focus.series.") > 0,
            "the line grows a full-colour focus segment"
        );
        assert!(
            find(&scene, "line.series.0").is_some(),
            "the muted context line is still drawn"
        );
        assert!(
            count_prefix(&scene, "scatter.point.") > 0,
            "the scatter points are still drawn (muted as context)"
        );
    }

    #[test]
    fn the_brush_window_is_announced() {
        let nodes = LinkedBrushView::access_node(&(SliderState::Idle, 0.5, 0.3, 0.6), None);
        let brush = nodes
            .iter()
            .find(|n| n.tag == BRUSH_TAG)
            .expect("the brush a11y node");
        assert_eq!(brush.role, AriaRole::Slider);
        assert_eq!(brush.name.as_deref(), Some("Linked x-window brush"));
        let Some(AccessValue::Text(text)) = brush.value.as_ref() else {
            panic!("brush value is Text, got {:?}", brush.value)
        };
        assert!(text.starts_with("x from"), "states its window: {text:?}");
    }

    #[test]
    fn view_carries_the_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<LinkedBrushView>(
            (SliderState::Idle, 0.5, 0.0, 1.0),
            &Frame::new(),
        );
    }
}
