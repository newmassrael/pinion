//! `hello-time-chart` — R1529 §5.38 the x-axis can be **UTC time**.
//!
//! The forcing consumer for [`pinion_chart::LineChart::x_time`], the crate's third axis kind (the toolkit's
//! date time axis, d3's `scaleUtc`). Until R1529 a timestamp reaching a `pinion-chart` axis was
//! a plain number, and the axis every monitoring chart has could not be drawn.
//!
//! ## What the toggle shows, and why it is two defects
//!
//! One dataset — four hours of request latency across a real incident window,
//! `2026-03-02 22:00` to `2026-03-03 02:00` UTC — plotted twice.
//!
//! On the **numeric** x-axis both halves of the axis are wrong at once:
//!
//! * the *ticks* come from the `1 / 2 / 5 x 10^n` nice-number step, which
//!   assumes the quantity subdivides decimally. Above a second time is
//!   mixed-radix, so the gridlines land on multiples of 2,000,000 ms — times
//!   no clock shows.
//! * the *labels* compact by magnitude, and one decimal at the giga scale is
//!   27-hour resolution, so all nine gridlines print `1772.5G`. Nine lines,
//!   one string.
//!
//! Toggling to **UTC time** puts the ticks on the half hour and gives each
//! label the finest calendar field that distinguishes it. The window straddles
//! midnight on purpose: the date appears exactly **once**, on the tick that
//! crosses into `Mar 03`, and every other label is a clock time. That is the
//! multi-resolution property — a reader gets the date where the axis changes
//! day, without every label repeating it.
//!
//! ## The scrub readout is not a tick label
//!
//! The crosshair header shows the full stamp (`2026-03-03 00:40:00`), not the
//! axis's `00:30`. An axis label is *relative* — legible because its
//! neighbours are on screen beside it — and a scrub has no neighbours, so the
//! same string would leave a reader unable to say which day was scrubbed.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the axis as tagged data — `chart.label.x.{k}` are
//! the tick labels and `chart.grid.x.{k}` their gridlines. The whole round is
//! read off those strings; no pixels are sampled (§2 #1 / §2 #7). See
//! `tools/demos/r1529_time_axis.py`.

use pinion_a11y::{AccessNode, ToggleSegment, WidgetA11y, toggle_button_group_nodes};
use pinion_chart::{ChartStyle, DataPoint, LineChart, PlotWindow, Series, map_window, plot_area};
use pinion_core::event::LINE_HEIGHT_PX;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, IntrospectSchema,
    IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTimeChartRenderer, HelloTimeChartRendererError);

const WIN_W: u32 = 680;
const WIN_H: u32 = 430;
const THEME_TAG: &str = "app";

/// The axis toggle's dispatch + Tab-stop tag.
const AXIS_TAG: &str = "axis_toggle";

/// The WAI-ARIA §3.6 `group` label for the AccessKit toggle button.
const GROUP_TAG: &str = "axis_group";

/// Time at boot: the axis this round exists for is what the window should
/// open on, and the toggle is how a reader reaches the comparison.
const BOOT_TIME: bool = true;

/// R1534 — the wheel-zoom target laid over the axis area, and the
/// `ExtraExternal` tag the router routes a wheel on the plot to.
const PLOT_TAG: &str = "plot";

/// R1534 — the status line naming the current window.
const ZOOM_TAG: &str = "zoom_status";

/// R1534 — magnification per wheel notch. The `hello-node-editor` canvas'
/// `ZOOM_STEP` peer, and like it a *continuous* exponent of the notch count, so
/// a trackpad's fractional notches zoom smoothly instead of banking (the
/// R1533 `WheelStepper` is for consumers that step DISCRETELY).
const ZOOM_PER_NOTCH: f32 = 1.25;

/// R1534 — pan per wheel notch, as a fraction of the VISIBLE window, so a
/// notch travels the same share of the plot at every zoom level.
const PAN_PER_NOTCH: f32 = 0.15;

const TITLE_FONT_PX: u32 = 17;
const CAPTION_FONT_PX: u32 = 12;
const TOGGLE_FONT_PX: u32 = 13;

/// Window-absolute plot region. The chart must be handed its final geometry
/// before layout runs (see the `pinion-chart` coordinate contract), so the
/// rect is a constant; the caption sits in the gap below it.
const CHART_RECT: Rect = Rect::new(16, 60, WIN_W - 32, WIN_H - 130);

/// Epoch milliseconds at `2026-03-02 22:00:00 UTC` — the incident window's
/// start. A literal rather than a computed "now", so the axis a demo reads is
/// the axis the tests assert (a host-clock-dependent chart would be the
/// R1500 failure: a test that reads its environment).
const T0_MS: f64 = 1_772_488_800_000.0;

/// Sample interval — ten minutes.
const STEP_MS: f64 = 600_000.0;

/// Samples: four hours at ten-minute resolution, inclusive of both ends.
const SAMPLES: usize = 25;

/// Series names.
const LABELS: [&str; 2] = ["p50", "p99"];

/// Series colours, pinned so each legend swatch matches its line.
const SERIES_COLORS: [Color; 2] = [Color::rgb(0x42, 0x85, 0xf4), Color::rgb(0xea, 0x43, 0x35)];

/// Request latency (ms) over the incident window, sampled every ten minutes.
///
/// The x-channel is an epoch millisecond — the unit
/// [`LineChart::x_time`](pinion_chart::LineChart::x_time) reads, matching the toolkit's date
/// time axis and d3's `scaleUtc`. The shape is incidental to the round; what matters
/// is that x is a real instant.
#[allow(
    clippy::cast_precision_loss,
    reason = "sample index (0..25) -> f64 offset is exact"
)]
fn latency_series() -> Vec<Series> {
    let p50: [f64; SAMPLES] = [
        41.0, 38.0, 44.0, 40.0, 43.0, 39.0, 42.0, 58.0, 91.0, 140.0, 176.0, 168.0, 151.0, 133.0,
        108.0, 84.0, 61.0, 48.0, 44.0, 41.0, 39.0, 42.0, 40.0, 38.0, 41.0,
    ];
    let p99: [f64; SAMPLES] = [
        280.0, 265.0, 291.0, 274.0, 288.0, 270.0, 296.0, 410.0, 690.0, 1150.0, 1420.0, 1360.0,
        1180.0, 1020.0, 820.0, 610.0, 430.0, 340.0, 305.0, 288.0, 276.0, 294.0, 281.0, 268.0,
        285.0,
    ];
    [p50, p99]
        .iter()
        .enumerate()
        .map(|(i, ys)| {
            let points = ys
                .iter()
                .enumerate()
                .map(|(k, &y)| DataPoint::new(T0_MS + k as f64 * STEP_MS, y))
                .collect();
            Series::new(LABELS[i], points).with_color(SERIES_COLORS[i])
        })
        .collect()
}

/// The full x-extent of the dataset — what a window fraction is a fraction OF.
fn x_extent() -> (f64, f64) {
    #[allow(
        clippy::cast_precision_loss,
        reason = "sample count (25) -> f64 is exact"
    )]
    let last = T0_MS + (SAMPLES - 1) as f64 * STEP_MS;
    (T0_MS, last)
}

/// R1534 — whether a window still covers the whole extent.
fn window_is_full(window: (f32, f32)) -> bool {
    window.1 - window.0 >= 1.0
}

/// The chart for one axis choice and one view window — the ONE place `x_time`
/// is applied, so the painted axis, the caption, and the tests all read the
/// same chart rather than three separately-configured ones.
///
/// R1534 — an unzoomed window does NOT re-domain the chart, and the reason is
/// MEASURED rather than assumed (the first draft asserted it in the wrong
/// place, and a counterfactual caught that): a derived domain is nice-rounded
/// to its own tick step, so pinning `x_extent()` verbatim is not the same
/// input. On the **time** axis it happens to make no difference — this
/// dataset starts on a nice time boundary, so `nice_time_domain` returns the
/// extent it was given. On the **numeric** axis it does: the decimal
/// nice-number step widens the domain, and the derived axis draws **9**
/// gridlines where the pinned one draws **7**. So a zoom that always
/// re-domained would silently change an unzoomed plot, on one axis kind out of
/// two. `r1534_an_unzoomed_plot_is_the_chart_it_was_before` holds this.
fn chart_for(time: bool, window: (f32, f32)) -> LineChart {
    let chart = LineChart::new(latency_series()).inspect(Some(0.5));
    let chart = if time { chart.x_time() } else { chart };
    if window_is_full(window) {
        chart
    } else {
        let (lo, hi) = map_window(window, x_extent());
        chart.with_x_domain(lo, hi)
    }
}

/// The themed chart style.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        legend: true,
        label_size_px: 13,
        x_ticks: 7,
        y_ticks: 6,
        ..ChartStyle::default()
    }
}

/// Every x-tick label the chart PAINTED, in axis order.
///
/// Read off the scene rather than recomputed, so a caption that disagreed
/// with the plot would be a bug in the crate and not in this string — the
/// same discipline `hello-log-chart`'s caption follows by reading
/// `off_scale()`.
fn x_tick_labels(scene: &Scene) -> Vec<String> {
    let mut out = Vec::new();
    for k in 0..32 {
        match find(scene, &format!("chart.label.x.{k}")) {
            Some(Scene::Text(t)) => out.push(t.content.clone()),
            _ => break,
        }
    }
    out
}

/// The caption under the plot: how many gridlines the axis drew, and how many
/// distinct strings it managed to label them with.
///
/// That ratio IS the defect. It is derived from the painted labels, so the
/// caption cannot claim a legibility the axis does not have.
fn caption(scene: &Scene, time: bool) -> String {
    let labels = x_tick_labels(scene);
    let distinct: std::collections::BTreeSet<&String> = labels.iter().collect();
    if time {
        format!(
            "UTC time x-axis — {} gridlines, {} distinct labels; the date is \
             named once, where the axis crosses into a new day",
            labels.len(),
            distinct.len(),
        )
    } else {
        format!(
            "numeric x-axis — {} gridlines, {} distinct label ({}): a decimal \
             step off the clock, and an epoch millisecond compacted by magnitude",
            labels.len(),
            distinct.len(),
            labels.first().map_or("-", String::as_str),
        )
    }
}

/// The axis toggle: a focusable tagged container the router dispatches clicks
/// to, painted as a chip with an on/off swatch.
fn axis_toggle(on: bool, theme: &Theme) -> Scene {
    let swatch = Scene::Box(
        pinion_core::scene::BoxNode::new(
            Rect::default(),
            BoxStyle::filled(if on {
                theme.resolve(ColorRole::Accent)
            } else {
                theme.resolve(ColorRole::Outline)
            })
            .with_corner_radius(3),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(TOGGLE_FONT_PX, TOGGLE_FONT_PX))),
    );
    let label = Scene::Text(TextNode::styled(
        "UTC time x-axis",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TOGGLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![swatch, label])
            .with_tag(AXIS_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(6)
                    .with_focusable(true)
                    .with_absolute_position(WIN_W - 176, 22)
                    .with_size(Size::px(162, TOGGLE_FONT_PX + 8)),
            ),
    )
}

/// R1534 §5.38 §5.45 — the plot's view window, and the wheel that moves it.
///
/// ## Why the gesture is on the plot and not on a strip
///
/// A brush strip ([`pinion_chart::Brush`], R1357) could already window this
/// axis, and every brush consumer here drags one below the plot. What a strip
/// cannot do is zoom **about a point**: the reader's cursor is over the minute
/// of the incident they care about, and the whole feel of a wheel zoom is that
/// that minute keeps its pixel while four hours spread around it.
///
/// ## The wheel vocabulary is the node canvas'
///
/// Deliberately identical to `hello-node-editor`'s (R877), because a second
/// dialect for the same input would be the divergence this repo keeps
/// catching:
///
/// * `Ctrl`+wheel — zoom, anchored at the cursor.
/// * `Shift`+wheel — pan (the vertical notches drive the x window).
/// * a horizontal wheel / trackpad axis — pan, no modifier needed. R1533 left
///   `dx` unread on the value widgets; here it has an obvious meaning.
/// * a plain vertical wheel — **declined**, so a chart in a scrolling
///   dashboard never steals the page scroll.
///
/// Every arm answers with [`PlotWindow`]'s "did it move", so a gesture the
/// window cannot spend (already at the magnification ceiling, already flush
/// against the extent) is handed back rather than swallowed — the R1533
/// verdict, on a second kind of consumer.
#[derive(Debug, Clone, Copy, Default)]
struct PlotZoomExternal {
    window: PlotWindow,
}

impl External for PlotZoomExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn wheel(&mut self, reading: &pinion_core::widgets::wheel::WheelReading) -> bool {
        let (x_rel, y_rel) = reading.at;
        let (dx, dy) = (reading.dx(), reading.dy());
        let modifiers = reading.modifiers;
        // R799 bounds guard, the node-canvas precedent: a wheel routed here
        // from a composite sibling lands outside `[0, 1]` and is not ours.
        if !(0.0..=1.0).contains(&x_rel) || !(0.0..=1.0).contains(&y_rel) {
            return false;
        }
        // Forward wheel (W3C `dy < 0`) is a POSITIVE notch count: zooming in,
        // and panning back in time.
        let notches = -dy / LINE_HEIGHT_PX;
        if modifiers.command_key() {
            // `x_rel` is a fraction of THIS node, which is laid over
            // `plot_area` — the axis itself, not the outer chart rect. Anchor
            // on the outer rect and the zoom pivots about a value the cursor
            // is not on, off by the y-label gutter.
            return self.window.zoom_about(x_rel, ZOOM_PER_NOTCH.powf(notches));
        }
        if modifiers.shift_key() {
            return self.window.pan_by(-notches * PAN_PER_NOTCH);
        }
        if dx.abs() > f32::EPSILON {
            return self.window.pan_by(dx / LINE_HEIGHT_PX * PAN_PER_NOTCH);
        }
        false
    }

    /// R1703 §5.45 §5.15 — the plot window zooms and pans under a wheel, and
    /// says so, which is also what makes the hook above reachable: the router
    /// offers `wheel` only where an intent is declared.
    ///
    /// Declared over the SAME bounds the hook guards, so the published answer
    /// and the behaviour agree at every point rather than only in the middle —
    /// this node is laid over the plot area inside a larger chart, and a wheel
    /// routed here from a composite sibling is not this widget's.
    fn wheel_intent(&self, at: (f32, f32)) -> Option<pinion_core::widgets::wheel::WheelIntent> {
        let (x_rel, y_rel) = at;
        ((0.0..=1.0).contains(&x_rel) && (0.0..=1.0).contains(&y_rel))
            .then_some(pinion_core::widgets::wheel::WheelIntent::Zoom)
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    /// The window changes only through a wheel or a `reset`, and the framework
    /// repaints after both (the router requests a redraw whenever the hovered
    /// widget consumed a wheel). Never self-dirty.
    fn is_dirty(&self) -> bool {
        false
    }
}

impl ExternalIntrospect for PlotZoomExternal {
    fn schema(&self) -> IntrospectSchema {
        // `low` / `high` are the brush's own field names, so one wire
        // vocabulary describes a window however it was moved.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("low", "float"),
                    SchemaField::new("high", "float"),
                    SchemaField::new("span", "float"),
                    SchemaField::action("reset", "bool"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "low" => Ok(IntrospectValue::Float(f64::from(self.window.low()))),
            "high" => Ok(IntrospectValue::Float(f64::from(self.window.high()))),
            "span" => Ok(IntrospectValue::Float(f64::from(self.window.span()))),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    /// The window is read-only through the state channel — a wheel or a
    /// `reset` moves it, and both keep its invariants. A raw `low` / `high`
    /// write could not, which is why the field pair is queryable but not
    /// settable (the R1533 discipline: every mutation funnels through the one
    /// path that clamps).
    fn intervene(
        &mut self,
        _path: &str,
        _value: IntrospectValue,
    ) -> Result<(), pinion_core::external::InterveneError> {
        Err(pinion_core::external::InterveneError::ReadOnly)
    }

    /// `reset` — `the toolkit's charting module`' `zoomReset`. Answers whether it had to do anything, so a key binding
    /// can decline `Escape` on an unzoomed plot instead of swallowing it.
    fn invoke(
        &mut self,
        path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "reset" => Ok(IntrospectValue::Bool(self.window.reset())),
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Read the view window off the [`PlotZoomExternal`]; the full extent when the
/// external is absent, so the chart is never blank because a lookup missed.
fn read_window(scene: &Scene) -> (f32, f32) {
    let Some(intro) = scene
        .find_external_with_tag(PLOT_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return (0.0, 1.0);
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a window fraction 0.0..=1.0 loses no meaningful precision as f32"
    )]
    let read = |field: &str, fallback: f32| {
        intro
            .query(field)
            .and_then(|v| match v {
                IntrospectValue::Float(f) => Ok(f as f32),
                _ => Err(ReadRefusal::UnknownPath),
            })
            .unwrap_or(fallback)
    };
    (read("low", 0.0), read("high", 1.0))
}

/// R1534 — the status line: what the window is, and how to move it.
///
/// Derived from the window the scene was built with, so it cannot claim a zoom
/// the plot does not have.
fn zoom_status(window: (f32, f32)) -> String {
    if window_is_full(window) {
        "full 4-hour window — Ctrl+wheel zooms about the cursor, \
         Shift+wheel pans"
            .to_string()
    } else {
        let (lo, hi) = map_window(window, x_extent());
        format!(
            "zoomed to {} - {} ({:.0}x) — Shift+wheel pans, Esc resets",
            pinion_chart::format_time_stamp(lo),
            pinion_chart::format_time_stamp(hi),
            1.0 / f64::from(window.1 - window.0),
        )
    }
}

/// Find the first node carrying `tag`.
fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
    if scene.tag() == Some(tag) {
        return Some(scene);
    }
    if let Scene::Container(c) = scene {
        return c.children.iter().find_map(|ch| find(ch, tag));
    }
    None
}

/// view-fn (§6.3): pure sync `AxisState -> Scene`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: AxisState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Request latency during an incident (ms)",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 22)),
    );

    // Built first: the caption reports what this very scene painted.
    let style = chart_style(&theme);
    let chart = chart_for(state.time, state.window).build(CHART_RECT, &style);
    let caption_text = caption(&chart, state.time);

    let caption = Scene::Text(
        TextNode::styled(
            caption_text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(CAPTION_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("caption".to_string())
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(18, WIN_H - 58)
                .with_size(Size::px(WIN_W - 36, 44)),
        ),
    );

    let status = Scene::Text(
        TextNode::styled(
            zoom_status(state.window),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CAPTION_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(ZOOM_TAG.to_string())
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(18, WIN_H - 26)
                .with_size(Size::px(WIN_W - 36, 20)),
        ),
    );

    // R1534 — the wheel target, laid over `plot_area` (the axis) and not over
    // `CHART_RECT` (the axis plus its label gutters), because `x_rel` is the
    // zoom's anchor and an anchor measured against the wrong rect pivots about
    // the wrong instant. Last in the tree so it sits above the marks; it paints
    // nothing.
    let zoom_target = capture_surface(PLOT_TAG, plot_area(CHART_RECT, style.margin), false);

    Scene::Container(
        ContainerNode::new(vec![
            chart,
            title,
            axis_toggle(state.time, &theme),
            caption,
            status,
            zoom_target,
        ])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_size(Size::px(WIN_W, WIN_H)),
        ),
    )
}

/// Which kind the x-axis is on, plus the toggle chip's visual state.
#[derive(Copy, Clone, PartialEq, Debug)]
struct AxisState {
    toggle: ToggleState,
    time: bool,
    /// R1534 — the view window as `(low, high)` fractions of the x-extent.
    /// The fractions and not a [`PlotWindow`]: the window's *gestures* live on
    /// the external that receives them, and the view needs only the pair —
    /// exactly the split `Brush::read` established for the brush.
    window: (f32, f32),
}

struct TimeChartView;

impl WidgetCore for TimeChartView {
    type State = AxisState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(toggle_group::boot_toggle(BOOT_TIME))
    }

    fn tag() -> &'static str {
        AXIS_TAG
    }

    fn read_state(scene: &Scene) -> AxisState {
        let (toggle, time) = toggle_group::read_toggle(scene, AXIS_TAG);
        AxisState {
            toggle,
            time,
            window: read_window(scene),
        }
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            PLOT_TAG,
            Box::new(PlotZoomExternal::default()),
        )]
    }

    fn view(state: AxisState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-time-chart (R1529 §5.38 UTC time axis)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // R1534 — Escape resets the zoom (the toolkit's charting module' `zoomReset`),
        // and only CONSUMES the key when there was a zoom to reset: a binding
        // that swallowed Escape on an unzoomed plot would shadow whatever else
        // the app wants it for.
        if key == "Escape" {
            let reset = scene
                .find_external_with_tag_mut(PLOT_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .and_then(|i| i.invoke("reset", IntrospectValue::Null).ok())
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if reset {
                return true;
            }
        }
        toggle_group::apply_key(scene, focused, key, &[AXIS_TAG])
    }

    fn fmt_state_log(state: &AxisState) -> String {
        format!(
            "{}{} window {:.3}..{:.3}",
            state.toggle.as_name(),
            if state.time { " time" } else { " numeric" },
            state.window.0,
            state.window.1,
        )
    }
}

impl WidgetA11y for TimeChartView {
    fn access_node(state: &AxisState, focused: Option<&str>) -> Vec<AccessNode> {
        let segments = [ToggleSegment {
            tag: AXIS_TAG,
            label: "UTC time x-axis",
            state: state.toggle,
            on: state.time,
        }];
        toggle_button_group_nodes(GROUP_TAG, "Horizontal axis kind", &segments, focused)
    }
}

impl WidgetView for TimeChartView {
    type Renderer = HelloTimeChartRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TimeChartView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_chart::{format_time_stamp, format_time_tick};
    use pinion_core::Owner;

    /// A settled (non-hovered, non-pressed) chip on the given axis kind.
    const fn idle(time: bool) -> AxisState {
        windowed(time, (0.0, 1.0))
    }

    /// R1534 — a settled chip on the given axis kind, at an explicit window.
    const fn windowed(time: bool, window: (f32, f32)) -> AxisState {
        AxisState {
            toggle: ToggleState::Idle,
            time,
            window,
        }
    }

    fn render(time: bool) -> Scene {
        Owner::new().run(|| view(idle(time), &Frame::new()))
    }

    fn text_of(scene: &Scene, tag: &str) -> String {
        match find(scene, tag) {
            Some(Scene::Text(t)) => t.content.clone(),
            _ => panic!("no text node tagged {tag}"),
        }
    }

    /// ★ The whole round, read off the painted labels. The numeric axis draws
    /// nine gridlines and manages ONE distinct string for them; the time axis
    /// gives every gridline its own.
    #[test]
    fn r1529_the_numeric_axis_labels_nine_gridlines_with_one_string() {
        let numeric = x_tick_labels(&render(false));
        let distinct: std::collections::BTreeSet<&String> = numeric.iter().collect();
        assert_eq!(numeric.len(), 9, "nine gridlines: {numeric:?}");
        assert_eq!(distinct.len(), 1, "one distinct label: {numeric:?}");
        assert_eq!(numeric[0], "1772.5G");

        let timed = x_tick_labels(&render(true));
        let distinct: std::collections::BTreeSet<&String> = timed.iter().collect();
        assert_eq!(timed.len(), 9, "the same nine positions: {timed:?}");
        assert_eq!(distinct.len(), 9, "each one labelled: {timed:?}");
    }

    /// ★ Multi-resolution: the ticks are half-hourly clock times, and the ONE
    /// that crosses into a new day names the day instead. That single label is
    /// the property a fixed format string cannot produce — `HH:MM` everywhere
    /// would lose the date, and `YYYY-MM-DD HH:MM` everywhere would repeat it
    /// on all nine.
    #[test]
    fn r1529_the_date_is_named_once_at_the_midnight_crossing() {
        let labels = x_tick_labels(&render(true));
        assert_eq!(
            labels,
            [
                "22:00", "22:30", "23:00", "23:30", "Mar 03", "00:30", "01:00", "01:30", "02:00"
            ]
        );
        let dated = labels.iter().filter(|l| l.starts_with("Mar")).count();
        assert_eq!(dated, 1, "exactly one label carries the date");
    }

    /// ★ A readout is not a tick label: the scrub header carries the full
    /// stamp, because it has no neighbouring labels to read the day from.
    #[test]
    fn r1529_the_scrub_header_is_a_full_stamp_not_an_axis_label() {
        // The scrub sits mid-plot, on the 23:50 sample.
        let focus = T0_MS + 11.0 * STEP_MS;
        let header = text_of(&render(true), "chart.inspect.header");
        assert_eq!(header, format!("x = {}", format_time_stamp(focus)));
        assert!(
            header.contains("2026-03-02"),
            "the scrub says which day: {header}"
        );
        // The axis label for that same instant is relative, and shorter.
        assert_eq!(format_time_tick(focus), "23:50");
        assert!(
            !x_tick_labels(&render(true))
                .iter()
                .any(|l| header.ends_with(l.as_str())),
            "no tick label is the full stamp"
        );

        // Off a time axis the two forms coincide, which is why the
        // distinction did not exist before this round.
        let numeric = text_of(&render(false), "chart.inspect.header");
        assert_eq!(numeric, "x = 1772.5G");
    }

    /// The numeric axis is unchanged — this is an opt-in, not a
    /// reinterpretation of every chart's x-channel.
    #[test]
    fn r1529_the_caption_reports_what_each_axis_achieved() {
        let numeric = text_of(&render(false), "caption");
        assert!(numeric.starts_with("numeric x-axis"), "got {numeric}");
        assert!(
            numeric.contains("9 gridlines, 1 distinct label"),
            "{numeric}"
        );

        let timed = text_of(&render(true), "caption");
        assert!(timed.starts_with("UTC time x-axis"), "got {timed}");
        assert!(timed.contains("9 gridlines, 9 distinct labels"), "{timed}");
    }

    #[test]
    fn r1529_the_axis_toggle_is_a_focusable_tagged_hit_region() {
        let scene = render(true);
        let Some(Scene::Container(chip)) = find(&scene, AXIS_TAG) else {
            panic!("the axis toggle is a focusable container")
        };
        assert!(chip.layout.focusable, "it is a Tab / click target");
    }

    #[test]
    fn r1529_a11y_exposes_the_axis_kind_as_one_toggle_button() {
        let nodes = TimeChartView::access_node(&idle(true), None);
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, pinion_a11y::AriaRole::Button))
            .count();
        assert_eq!(buttons, 1, "one aria-pressed button for the axis kind");
    }

    // ─────────────────────────────────────────────────────────────────
    // R1534 §5.38 — the plot's view window.
    // ─────────────────────────────────────────────────────────────────

    /// The chart the same data and window would build if the binding pinned the
    /// extent instead of letting the axis derive it — the counterfactual
    /// `chart_for` deliberately avoids.
    fn pinned_to_extent(time: bool) -> Scene {
        let (lo, hi) = x_extent();
        Owner::new().run(|| {
            let c = LineChart::new(latency_series()).inspect(Some(0.5));
            let c = if time { c.x_time() } else { c };
            c.with_x_domain(lo, hi)
                .build(CHART_RECT, &chart_style(&Theme::default()))
        })
    }

    fn chart_of(state: AxisState) -> Scene {
        Owner::new().run(|| {
            chart_for(state.time, state.window).build(CHART_RECT, &chart_style(&Theme::default()))
        })
    }

    /// ★ Why an unzoomed window must NOT re-domain, measured on both axis
    /// kinds rather than asserted from the idea.
    ///
    /// A derived domain is nice-rounded to its own tick step, so handing the
    /// axis `x_extent()` verbatim is a different input. On the time axis this
    /// dataset happens to start on a nice boundary and nothing changes — which
    /// is exactly why the claim needs the other axis to be tested at all.
    #[test]
    fn r1534_an_unzoomed_plot_is_the_chart_it_was_before() {
        let full = (0.0, 1.0);
        assert_eq!(
            x_tick_labels(&chart_of(windowed(true, full))),
            x_tick_labels(&pinned_to_extent(true)),
            "the TIME axis is indifferent — this window starts on a nice \
             boundary, so `nice_time_domain` returns what it was given"
        );
        let derived = x_tick_labels(&chart_of(windowed(false, full)));
        let pinned = x_tick_labels(&pinned_to_extent(false));
        assert_eq!(derived.len(), 9, "the derived numeric axis: {derived:?}");
        assert_eq!(pinned.len(), 7, "the pinned numeric axis: {pinned:?}");
        assert_ne!(
            derived, pinned,
            "so a zoom that re-domained even at full extent would change an \
             unzoomed plot on this axis kind"
        );
    }

    /// A zoomed window reaches the axis, not merely the marks: the ticks are
    /// re-picked at the finer step the narrower span deserves.
    #[test]
    fn r1534_a_zoomed_window_re_picks_the_ticks() {
        let full = x_tick_labels(&chart_of(windowed(true, (0.0, 1.0))));
        let zoomed = x_tick_labels(&chart_of(windowed(true, (0.4, 0.6))));
        assert_ne!(full, zoomed, "a 5x window is labelled differently");
        assert!(!zoomed.is_empty(), "and is still labelled: {zoomed:?}");
        assert!(
            zoomed
                .iter()
                .all(|l| l.contains(':') || l.starts_with("Mar")),
            "every label is still a clock time or a date: {zoomed:?}"
        );
    }

    /// The status line is derived from the window the scene was built with, so
    /// it cannot claim a zoom the plot does not have.
    #[test]
    fn r1534_the_status_line_reports_the_window_it_painted() {
        let full = zoom_status((0.0, 1.0));
        assert!(full.contains("full"), "{full}");
        assert!(full.contains("Ctrl+wheel"), "and says how to zoom: {full}");
        let zoomed = zoom_status((0.4, 0.6));
        assert!(zoomed.contains("5x"), "names the magnification: {zoomed}");
        assert!(zoomed.contains("Esc"), "and the way out: {zoomed}");
        assert!(
            zoomed.contains("23:36") || zoomed.contains("23:3"),
            "and the instants it is showing: {zoomed}"
        );
    }

    /// The wheel target covers the AXIS, not the chart rect. An anchor measured
    /// across the y-label gutter pivots about an instant the cursor is not on,
    /// and this is the only place that geometry is stated.
    #[test]
    fn r1534_the_wheel_target_covers_the_axis_not_the_chart_rect() {
        let scene = Owner::new().run(|| view(idle(true), &Frame::new()));
        let target = find(&scene, PLOT_TAG).expect("the zoom target is in the tree");
        let axis = plot_area(CHART_RECT, chart_style(&Theme::default()).margin);
        let layout = match target {
            Scene::Box(b) => &b.layout,
            _ => panic!("the target is the shared capture_surface (a Box)"),
        };
        assert_eq!(
            layout.size,
            Size::px(axis.w, axis.h),
            "the target is the axis's own size, not the chart rect's"
        );
        assert!(axis.w < CHART_RECT.w, "premise: the axis is inset");
    }
}
