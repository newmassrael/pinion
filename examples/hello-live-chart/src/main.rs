//! `hello-live-chart` — R1398: a **live telemetry strip chart**. The R1000
//! off-thread `RepaintSink` seam feeding a `pinion-chart` `LineChart` that
//! slides its x-window and (R1397) auto-fits its y-axis to that window.
//!
//! ## What this demonstrates
//!
//! A background producer thread (the R1000 PTY-reader analog) owns a `Send`
//! ring buffer of `DataPoint`s. Each `Tick` poke makes it append the next
//! sample and call `RepaintSink::request_repaint`, exactly as
//! `hello-live-data` does for a log — but here the samples are CHARTED, not
//! listed. The signal is a large startup transient (`seq <= 3`, y ~940) over a
//! long steady state of small ripples (y ~30-50).
//!
//! `view` reads the producer-authoritative buffer each frame and charts it over
//! a **sliding x-window** (the last [`WINDOW_SPAN`] of x): once the data
//! outgrows the window the domain follows the newest sample, so the startup
//! transient **scrolls off the left** as time advances. Paired with
//! [`LineChart::rescale_y_to_x_window`] (R1397, this is its 2nd consumer), the
//! y-axis then auto-fits to whatever is *in* the window — so once the transient
//! scrolls off, the y-axis falls from ~1000 to ~50 and the steady-state ripples
//! resolve. That is the canonical live strip chart (an oscilloscope, a
//! monitoring dashboard): a scrolling x-axis with a value axis that tracks it.
//!
//! ## Architecture (producer-authoritative, off-thread, event-driven)
//!
//! Identical to `hello-live-data`'s seam: the ring + producer thread + poke
//! channel are self-created once in a `use_live_samples` `Owner::cache` hook;
//! [`use_repaint_sink`] and `use_scene_revision` are pre-resolved before the
//! cache factory (the R666 nested-factory guard). The reducer only *pokes* the
//! producer; the thread appends + wakes; the next frame's `view` re-reads the
//! ring. Event-driven, no polling. The ring is bounded ([`MAX_POINTS`]) so a
//! long run stays memory-bounded.
//!
//! ## Verification
//!
//! `tools/demos/r1398_live_chart.py`: boot (empty) -> Tick into the transient
//! (the y-axis reaches ~1000) -> Tick until the window slides past it (the
//! window's left edge advances, the y-axis auto-fits down to ~tens) — all
//! observed as scene-as-data (§2 #7), no pixels; the off-thread appends are
//! awaited with a bounded poll (ZERO-FLAKE, the R1000 pattern).

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::external::External;
use pinion_core::reactive::Owner;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::use_repaint_sink;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::use_scene_revision;
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, read_button_focused, read_button_state,
};
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloLiveChartRenderer, HelloLiveChartRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";

const TICK_TAG: &str = "tick";
const TICK_HOVER_KEY: &str = "tick_hover";
const STATUS_TAG: &str = "live_status";

/// `Owner::cache` key for the shared producer state (ring + poke channel +
/// retained thread handle).
const SAMPLES_KEY: &str = "live_chart.samples";

/// Window-absolute plot region (the `pinion-chart` `build` coordinate contract).
const CHART_RECT: Rect = Rect::new(14, 46, WIN_W - 28, WIN_H - 128);

/// The width of the visible x-window. Once the data spans more than this, the
/// domain follows the newest sample (the strip scrolls); before that it shows
/// `[first, first + WINDOW_SPAN]` so the data fills the window left-to-right.
const WINDOW_SPAN: f64 = 12.0;

/// Max resident samples (the ring cap) — a live chart windows its history so a
/// long run stays memory-bounded. Well above `WINDOW_SPAN` so the visible
/// window is always fully backed.
const MAX_POINTS: usize = 48;

// ── the signal shape (the single SSOT the producer + tests share) ──
const SIGNAL_BASE: f64 = 40.0;
const RIPPLE_AMP: f64 = 10.0;
const RIPPLE_FREQ: f64 = 0.8;
const TRANSIENT_AMP: f64 = 900.0;
/// The startup transient covers `1..=SPIKE_END_SEQ`; after it the signal is the
/// bare steady-state ripple.
const SPIKE_END_SEQ: usize = 3;

/// The sample the producer writes for poke `seq` (`seq` starts at 1) — the
/// single SSOT shared by the producer thread and the tests. A big startup
/// transient over a long steady state of small ripples, so once the window
/// slides past the transient the y auto-fit has something to reveal.
#[allow(
    clippy::cast_precision_loss,
    reason = "poke seq -> f64 x/phase is exact for the demo's range"
)]
fn producer_sample(seq: usize) -> DataPoint {
    let x = seq as f64;
    let transient = if seq <= SPIKE_END_SEQ {
        TRANSIENT_AMP
    } else {
        0.0
    };
    let y = SIGNAL_BASE + RIPPLE_AMP * (x * RIPPLE_FREQ).sin() + transient;
    DataPoint::new(x, y)
}

// ─── producer-authoritative shared state (ring + thread + poke channel) ───

/// The off-thread producer's shared state — the R1000 `LiveLog` shape with a
/// `DataPoint` ring instead of a `String` log. The ring is written by the
/// producer thread and read by `view` on the UI thread (producer-authoritative);
/// `tx` pokes the producer; `_thread` is retained so the producer outlives boot
/// (it exits when the last `Sender` drops with the `Owner`).
struct LiveSamples {
    points: Arc<Mutex<Vec<DataPoint>>>,
    tx: Sender<()>,
    _thread: std::thread::JoinHandle<()>,
}

/// Self-create (once) the ring + poke channel + producer thread, and return the
/// cached handle. Spawns the thread on first call — invoked from
/// `create_extra_externals` (boot), never the pure `view`.
///
/// `use_repaint_sink()` / `use_scene_revision()` are resolved BEFORE the
/// `Owner::cache` factory so it never re-enters `Owner::cache` (the R666
/// nested-factory guard).
fn use_live_samples() -> Rc<LiveSamples> {
    let owner = Owner::current().expect("use_live_samples() requires an active Owner scope");
    let sink = use_repaint_sink();
    let scene_revision = use_scene_revision();
    owner.cache(SAMPLES_KEY, move || {
        let points: Arc<Mutex<Vec<DataPoint>>> = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel::<()>();
        let points_for_thread = Arc::clone(&points);
        let thread = std::thread::Builder::new()
            .name("hello-live-chart-producer".to_owned())
            .spawn(move || {
                // Block until poked, append the next sample to the ring, then
                // wake the shell. `recv` errors when every `Sender` has dropped
                // (the Owner went away) — the thread then exits.
                let mut seq = 0_usize;
                while rx.recv().is_ok() {
                    seq += 1;
                    if let Ok(mut guard) = points_for_thread.lock() {
                        guard.push(producer_sample(seq));
                        let overflow = guard.len().saturating_sub(MAX_POINTS);
                        if overflow > 0 {
                            guard.drain(0..overflow);
                        }
                    }
                    sink.request_repaint();
                    scene_revision.bump();
                }
            })
            .expect("spawn hello-live-chart producer thread");
        LiveSamples {
            points,
            tx,
            _thread: thread,
        }
    })
}

/// Snapshot the producer-authoritative ring for this frame (a poisoned lock
/// degrades to empty rather than panicking the pure view).
fn samples(live: &LiveSamples) -> Vec<DataPoint> {
    live.points.lock().map(|g| g.clone()).unwrap_or_default()
}

/// The sliding x-window over `points`: `[first, first + span]` until the data
/// outgrows the window, then `[last - span, last]` so the domain follows the
/// newest sample (the strip scrolls). Empty data shows `[0, span]`.
fn window_domain(points: &[DataPoint], span: f64) -> (f64, f64) {
    match (points.first(), points.last()) {
        (Some(first), Some(last)) => {
            if last.x - first.x <= span {
                (first.x, first.x + span)
            } else {
                (last.x - span, last.x)
            }
        }
        _ => (0.0, span),
    }
}

/// Resolve the theme into a [`ChartStyle`] (the deliberate consumer-side
/// theme->style mapping — `pinion-chart` stays theme-independent).
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

/// The status line text — the single SSOT for the visible status *and* the
/// `role=status` live-region accessible name. States the sliding window, the
/// resident sample count, and the latest value (so a screen reader hears the
/// live update).
fn status_line(points: &[DataPoint], window: (f64, f64)) -> String {
    if points.is_empty() {
        return "No samples yet \u{2014} press Tick to stream".to_owned();
    }
    let latest = points.last().map_or(0.0, |p| p.y);
    format!(
        "window x {:.0}..{:.0}  |  {} samples  |  latest {:.0}",
        window.0,
        window.1,
        points.len(),
        latest
    )
}

/// view-fn (§6.3): pure sync mapping `(button posture) -> Scene`, reading the
/// producer-authoritative ring for the chart + status. No side-effects (the
/// producer thread is spawned in `create_extra_externals`).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: LiveChartState, _frame: &Frame) -> Scene {
    let (posture, focused) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let style = chart_style(&theme);

    let points = samples(&use_live_samples());
    let window = window_domain(&points, WINDOW_SPAN);

    let title = Scene::Text(
        TextNode::styled(
            "Live throughput — Tick to stream; the window scrolls and the y-axis auto-fits",
            Rect::default(),
            TextStyle::new().with_size_px(18).with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 14)),
    );

    // The R1398 pairing: a sliding x-window (follows the newest sample) + the
    // R1397 y-auto-fit to that window, so the transient scrolls off and the
    // y-axis tracks the visible detail.
    let chart = LineChart::new(vec![Series::new("signal", points.clone())])
        .filled(true)
        .with_x_domain(window.0, window.1)
        .rescale_y_to_x_window(true)
        .build(CHART_RECT, &style);

    let status = Scene::Text(
        TextNode::styled(
            status_line(&points, window),
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(18, WIN_H - 22)),
    );

    let button = pinion_widget_paint::button::button_scene(
        "Tick",
        posture,
        focused,
        TICK_HOVER_KEY,
        &ButtonColors::filled_tonal(&theme),
        &ButtonStyle::m3_default(TICK_TAG)
            .with_size(Size::px(120, 36))
            .with_label_font_size_px(15),
    );
    let button = Scene::Container(
        ContainerNode::new(vec![button])
            .with_layout(LayoutStyle::new().with_absolute_position(WIN_W - 140, WIN_H - 46)),
    );

    Scene::Container(
        ContainerNode::new(vec![chart, title, status, button])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// Cached posture of the Tick button + its focus flag. The samples live in the
/// producer-authoritative ring the owner-scoped view reads directly.
type LiveChartState = (ButtonState, bool);

struct LiveChartView;

impl WidgetCore for LiveChartView {
    type State = LiveChartState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Spawn the producer thread FIRST (boot) so the data layer is live
        // before the first paint — never inside the pure view-fn.
        let _live = use_live_samples();
        Vec::new()
    }

    fn tag() -> &'static str {
        TICK_TAG
    }

    fn read_state(scene: &Scene) -> LiveChartState {
        (
            read_button_state(scene, TICK_TAG),
            read_button_focused(scene, TICK_TAG),
        )
    }

    fn view(state: LiveChartState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-live-chart (R1398 live strip chart: scroll + y-auto-fit)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        apply_aria_activate(scene, focused, key, TICK_TAG)
    }

    /// A Tick click only *pokes* the producer thread through the channel (the
    /// R1000 reducer shape) — it never touches the ring. The producer appends
    /// the next sample and calls `request_repaint`; the next frame re-reads it.
    fn update(
        _state: LiveChartState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == "tick.click" {
            // A dropped poke (producer gone) is harmless — the app is tearing down.
            let _ = use_live_samples().tx.send(());
        }
        Vec::new()
    }
}

impl WidgetA11y for LiveChartView {
    /// A `role=status` live region announcing the window + latest value (so a
    /// screen reader hears each streamed update), plus the Tick `button`. The
    /// chart geometry itself is read as data via `scene/snapshot`.
    fn access_node(state: &LiveChartState, focused: Option<&str>) -> Vec<AccessNode> {
        let (posture, _focused) = *state;
        let points = samples(&use_live_samples());
        let window = window_domain(&points, WINDOW_SPAN);
        vec![
            AccessNode::new(STATUS_TAG, AriaRole::Status)
                .with_name(status_line(&points, window))
                .with_value(AccessValue::Text(status_line(&points, window))),
            AccessNode::new(TICK_TAG, AriaRole::Button)
                .with_state(button_a11y_state(posture, focused == Some(TICK_TAG))),
        ]
    }
}

impl WidgetView for LiveChartView {
    type Renderer = HelloLiveChartRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<LiveChartView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> LiveChartState {
        (ButtonState::Idle, false)
    }

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

    fn text_under(scene: &Scene, tag: &str) -> Option<String> {
        match find(scene, tag)? {
            Scene::Text(t) => Some(t.content.clone()),
            Scene::Container(c) => c.children.iter().find_map(|ch| match ch {
                Scene::Text(t) => Some(t.content.clone()),
                _ => None,
            }),
            _ => None,
        }
    }

    /// Render the view over an owner-scoped ring seeded with `seq` samples.
    fn rendered_with(seqs: std::ops::RangeInclusive<usize>) -> Scene {
        let owner = Owner::new();
        owner.run(|| {
            let live = use_live_samples();
            {
                let mut g = live.points.lock().unwrap();
                for s in seqs.clone() {
                    g.push(producer_sample(s));
                }
            }
            view(idle(), &Frame::new())
        })
    }

    /// Render the view over an empty (un-poked) ring.
    fn rendered_empty() -> Scene {
        Owner::new().run(|| {
            let _ = use_live_samples();
            view(idle(), &Frame::new())
        })
    }

    /// The largest numeric y-tick label value on the chart (SI 'k' = *1000).
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
    fn producer_sample_is_the_signal_ssot() {
        let p1 = producer_sample(1);
        assert!((p1.x - 1.0).abs() < 1e-9);
        assert!(
            p1.y > 900.0,
            "seq 1 is inside the transient, got y={}",
            p1.y
        );
        let p10 = producer_sample(10);
        assert!(p10.y < 100.0, "seq 10 is steady-state, got y={}", p10.y);
    }

    #[test]
    fn window_fills_then_scrolls() {
        // Few points: the window is anchored at the first sample.
        let few: Vec<DataPoint> = (1..=5).map(producer_sample).collect();
        assert_eq!(window_domain(&few, WINDOW_SPAN), (1.0, 1.0 + WINDOW_SPAN));
        // Many points: the window follows the newest sample (the strip scrolls).
        let many: Vec<DataPoint> = (1..=30).map(producer_sample).collect();
        let (lo, hi) = window_domain(&many, WINDOW_SPAN);
        assert!((hi - 30.0).abs() < 1e-9, "right edge is the newest x");
        assert!(
            (lo - (30.0 - WINDOW_SPAN)).abs() < 1e-9,
            "left edge trails by the span"
        );
    }

    #[test]
    fn empty_ring_shows_the_placeholder_status() {
        let scene = rendered_empty();
        assert!(
            text_under(&scene, STATUS_TAG)
                .as_deref()
                .is_some_and(|t| t.contains("No samples yet")),
            "empty ring shows the placeholder"
        );
        assert!(
            find(&scene, "chart").is_some(),
            "the chart root is present even when empty"
        );
    }

    #[test]
    fn early_window_reaches_the_transient_magnitude() {
        // 8 samples: the window [1, 13] still covers the transient (seq 1..3),
        // so the y-axis reaches a kilo magnitude.
        let scene = rendered_with(1..=8);
        assert!(
            max_y_tick(&scene) >= 500.0,
            "the early window's y-axis reaches the transient, got {}",
            max_y_tick(&scene)
        );
    }

    #[test]
    fn scrolled_window_auto_fits_the_y_axis_down() {
        // 30 samples: the window [18, 30] has scrolled past the transient, so
        // the y-axis auto-fits to the steady-state ripple (tens, not thousands).
        let scene = rendered_with(1..=30);
        assert!(
            max_y_tick(&scene) < 200.0,
            "the scrolled window's y-axis fits the ripples, got {}",
            max_y_tick(&scene)
        );
        // The status states the scrolled window.
        let status = text_under(&scene, STATUS_TAG).expect("status text");
        assert!(
            status.contains("30 samples"),
            "status counts the samples: {status}"
        );
        assert!(
            status.contains("window x 18..30"),
            "status states the scrolled window: {status}"
        );
    }

    #[test]
    fn the_transient_scrolls_off_the_left() {
        // The window's DATA left edge advances as samples arrive (the strip
        // scrolls): the transient (x=1..3) is inside the early window's domain,
        // outside the late one's. Read the domain from the status SSOT + the
        // y-axis (early reaches the transient, late does not).
        let early = rendered_with(1..=8);
        let late = rendered_with(1..=30);
        assert!(
            text_under(&early, STATUS_TAG).is_some_and(|t| t.contains("window x 1..")),
            "the early window is anchored at the first sample"
        );
        assert!(
            text_under(&late, STATUS_TAG).is_some_and(|t| t.contains("window x 18..30")),
            "the late window has scrolled past the transient"
        );
        assert!(
            max_y_tick(&early) > max_y_tick(&late) * 3.0,
            "the transient scrolled off: the y-axis fell from {} to {}",
            max_y_tick(&early),
            max_y_tick(&late)
        );
    }

    #[test]
    fn tick_pokes_producer_which_appends_off_thread() {
        // The end-to-end path: a Tick poke crosses the thread boundary and the
        // producer appends. Bounded poll (no wall-clock assertion), the R1000
        // executor-test pattern; the thread responds in microseconds.
        let owner = Owner::new();
        owner.run(|| {
            let live = use_live_samples();
            assert!(live.points.lock().unwrap().is_empty());
            let intent = pinion_core::Intent::new_owned(
                "tick.click".to_owned(),
                pinion_core::external::IntrospectValue::Null,
            );
            let _ = LiveChartView::update(idle(), &intent);
            let mut appended = false;
            for _ in 0..200 {
                if live.points.lock().unwrap().len() == 1 {
                    appended = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(appended, "producer thread appended a sample after the poke");
            assert!(
                live.points.lock().unwrap()[0].y > 900.0,
                "the first sample is the transient"
            );
        });
    }

    #[test]
    fn ring_windows_to_max_points() {
        let owner = Owner::new();
        owner.run(|| {
            let live = use_live_samples();
            let mut g = live.points.lock().unwrap();
            for s in 1..=(MAX_POINTS + 6) {
                g.push(producer_sample(s));
                let overflow = g.len().saturating_sub(MAX_POINTS);
                if overflow > 0 {
                    g.drain(0..overflow);
                }
            }
            assert_eq!(g.len(), MAX_POINTS, "the ring windows to MAX_POINTS");
        });
    }

    #[test]
    fn a11y_status_live_region_and_button() {
        let owner = Owner::new();
        let nodes = owner.run(|| {
            let live = use_live_samples();
            live.points.lock().unwrap().push(producer_sample(1));
            <LiveChartView as WidgetA11y>::access_node(&idle(), None)
        });
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == STATUS_TAG && n.role == AriaRole::Status),
            "the window/latest is a role=status live region"
        );
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == TICK_TAG && n.role == AriaRole::Button)
        );
    }

    #[test]
    fn tick_button_is_focusable() {
        let scene = Owner::new().run(|| view(idle(), &Frame::new()));
        assert_eq!(scene.collect_focusable_tags(), vec![TICK_TAG.to_owned()]);
    }

    #[test]
    fn view_carries_the_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<LiveChartView>(
            idle(),
            &Frame::new(),
        );
    }
}
