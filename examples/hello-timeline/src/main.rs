//! `hello-timeline` — R1389 §5.16 §5.7 consumer of `pinion-chart`'s
//! [`Timeline`]: a **flame / track view** of the real per-frame render phases,
//! with a draggable playhead scrubber.
//!
//! ## What this demonstrates
//!
//! `pinion-chart` had value forms only (line / bar / scatter / donut / treemap /
//! sparkline — "how big"). R1389 adds the crate's first TIME form: labelled
//! [`Span`]s on horizontal [`Lane`]s over a shared time ruler, plus a playhead —
//! the track view a sequencer and a capture-replay tool need (gap #4 of the
//! dashboard substrate audit). This binding is its forcing consumer, and it
//! reuses the profiler's already-streaming REAL data rather than a synthetic
//! series: `use_frame_timings` (R1361, the seam `hello-frame-profiler` reads)
//! publishes the last frames' phase durations, and this lays them out as a
//! **flame**.
//!
//! Each painted frame is bracketed into four disjoint sub-phases — **build**
//! (`view` + layout), **encode** (scene → `vello`), **acquire** (the vsync block) and
//! **render** (GPU submit). They run SEQUENTIALLY within a frame, so the
//! honest layout is a flame: frame `k` starts at the cumulative sum of the
//! prior frames' totals, and its four phases abut end-to-end from there, each
//! on its own lane. At any instant exactly one phase of one frame is running,
//! so a vertical playhead crosses one lane's span — which is precisely the
//! reading a tracing tool (Chrome tracing / the engine Insights) exists to
//! give: "at t, the render loop was in frame N's acquire phase".
//!
//! ## Why a Slider, and pinned geometry
//!
//! The §5.38 [`SliderExternal`] (a captured 1-D fraction) is reused as the
//! playhead position — RPC-drivable (`scene/intervene`) and introspectable, no
//! new external invented, exactly as `hello-scatter`. The timeline is PINNED
//! ([`Timeline::build`]) to a const [`CHART_RECT`]: the span / ruler / playhead
//! geometry is what this binding exercises, and the layout-native `build_fill`
//! seam is already proven by `hello-frame-profiler` — a const rect keeps the
//! a11y-readout parity a one-liner (the readout resolves the same geometry the
//! paint does, because the two share the rect + the default margins).
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the flame + overlay as tagged data —
//! `timeline.lane.{i}.span.{j}`, `timeline.axis.x`, `timeline.grid.x.{k}`,
//! `timeline.tick.{k}`, `timeline.playhead` / `.tooltip` / `.header` /
//! `.value.{i}`. Driving the playhead over RPC re-reads a different frame /
//! phase, observed structurally without OCR (§2 #1 / #7). Because the data is
//! MEASURED (not reproducible run to run), every assertion is a structural
//! invariant, not a pinned value — the discipline a live tool needs. See
//! `tools/demos/r1389_frame_timeline.py`.

use pinion_a11y::described::describedby_region;
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, Lane, Span, Timeline};
use pinion_core::scene::{ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::widgets::slider::{SliderEvent, SliderExternal, SliderState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, use_theme};
use pinion_derive::widget;
use pinion_runtime::{FrameTiming, FrameTimingsView, use_frame_timings};
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::slider::{read_slider_state, slider_apply_key};

// pinion-forge codegen output: `pub struct HelloTimelineRenderer` + …
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTimelineRenderer, HelloTimelineRendererError);

const WIN_W: u32 = 780;
const WIN_H: u32 = 420;

const THEME_TAG: &str = "app";
const SCRUB_TAG: &str = "timeline_scrub";

const TITLE_FONT_PX: u32 = 16;
const STATUS_FONT_PX: u32 = 12;

/// Window-absolute timeline region. The timeline is pinned to it, and it is
/// also the scrub capture basis: the `timeline_scrub` box covers exactly this
/// rect, so the slider value `0.0..=1.0` is the playhead fraction across it.
const CHART_RECT: Rect = Rect::new(10, 42, WIN_W - 20, WIN_H - 78);

/// How many of the rolling window's most-recent frames the flame shows. The
/// `FRAME_TIMINGS` ring holds up to 120; at the full window each frame's phases
/// would be a couple of pixels wide (unreadable), so this binding shows the
/// last `RECENT` — a consumer legibility choice, NOT a substrate cap (the
/// `Timeline` itself is uncapped: the crate tests drive many-span lanes).
const RECENT: usize = 24;

/// The four disjoint render sub-phases, in the order they run within a frame —
/// which is the order they abut in the flame. `total` and `work` (the profiler's
/// envelopes) are deliberately excluded: they OVERLAP the sub-phases, so plotting
/// them as lanes would break the flame's "one phase active at a time" reading.
type PhasePick = fn(&FrameTiming) -> u64;
const PHASES: [(&str, PhasePick); 4] = [
    ("build", |s| s.build_us),
    ("encode", |s| s.encode_us),
    ("acquire", |s| s.acquire_us),
    ("render", |s| s.render_us),
];

/// Microseconds → milliseconds (the unit the ruler reads in).
#[allow(
    clippy::cast_precision_loss,
    reason = "a frame span in µs is far below 2^53; the conversion is exact"
)]
fn ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

/// Lay the last [`RECENT`] frames' phase durations out as a flame: one [`Lane`]
/// per phase, one [`Span`] per frame placed at its cumulative-time offset, the
/// four phases of a frame abutting end-to-end from that frame's start. Each
/// span is labelled with the frame's index in the shown window (`f0`…), which
/// the playhead readout surfaces. No samples yet ⇒ empty lanes (the timeline
/// still draws its ruler).
fn phase_lanes(view: &FrameTimingsView) -> Vec<Lane> {
    let start = view.samples.len().saturating_sub(RECENT);
    let recent = &view.samples[start..];
    let mut lanes: Vec<Lane> = PHASES
        .iter()
        .map(|(name, _)| Lane::new(*name, Vec::new()))
        .collect();
    let mut frame_start = 0.0_f64;
    for (k, frame) in recent.iter().enumerate() {
        let mut cursor = frame_start;
        for (li, (_, pick)) in PHASES.iter().enumerate() {
            let dur = ms(pick(frame));
            lanes[li]
                .spans
                .push(Span::new(cursor, cursor + dur, format!("f{k}")));
            cursor += dur;
        }
        // The next frame starts a whole `total` later — the tail between the
        // last phase and `total` is unaccounted "other" time, an honest gap.
        frame_start += ms(frame.total_us);
    }
    lanes
}

/// The timeline over the current window's flame, at the given playhead fraction.
fn timeline(view: &FrameTimingsView, scrub: f32) -> Timeline {
    Timeline::new(phase_lanes(view)).playhead(Some(scrub))
}

/// The status readout. Frame count + the shown window's wall-clock span come
/// from the same seam the flame is built from, so the line cannot disagree with
/// the picture.
fn status_line(view: &FrameTimingsView, scrub: f32) -> String {
    // R1754 — by reference: the snapshot stopped being `Copy` when it started
    // carrying the adapter that produced its numbers.
    let Some(s) = view.snapshot.as_ref() else {
        return "no frames measured yet — the first paint records the first sample".to_string();
    };
    let shown = view.samples.len().min(RECENT);
    let span_ms: f64 = view
        .samples
        .iter()
        .rev()
        .take(RECENT)
        .map(|f| ms(f.total_us))
        .sum();
    format!(
        "frame {} | showing last {shown} frames | {span_ms:.1}ms window | playhead {scrub:.2}",
        s.frame_count,
    )
}

/// Resolve the theme into a [`ChartStyle`]. Only COLOURS are overridden — the
/// margins / tick targets stay the defaults, which is what lets the a11y readout
/// (computed with the default style) resolve the identical playhead geometry.
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

/// Reused as the playhead-position holder, seeded to the centre so the boot
/// frame shows a scrubbed playhead.
fn scrub_external() -> SliderExternal {
    let mut slider = SliderExternal::new();
    slider.set_value(0.5);
    slider
}

/// view-fn (§6.3): pure sync `f32 -> Scene` of PUBLISHED state. `scrub` is the
/// playhead fraction across [`CHART_RECT`]. The wall-clock non-determinism
/// enters at the shell's `use_frame_timings` publish, not here (the Slider's
/// interaction state drives only the a11y node, so the paint takes only the
/// fraction).
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the §5.16 inherent-view shim hands `&Frame` in; the signature \
              mirrors the other chart bindings' view fns"
)]
fn view(scrub: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);

    let timings = use_frame_timings();

    let title = Scene::Text(
        TextNode::styled(
            "Frame timeline — render phases as a flame, drag to scrub the playhead",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, 12)),
    );

    let flame = timeline(&timings, scrub).build(CHART_RECT, &chart_style(&theme));

    // Transparent capture surface over the plot — the `timeline_scrub` primary
    // tag. On top so a press anywhere on the timeline drives the playhead;
    // transparent so the flame shows through, pointer-opaque so it captures.
    // R1417 capture_surface lift.
    let scrub_surface = capture_surface(SCRUB_TAG, CHART_RECT, false);

    let status = Scene::Text(
        TextNode::styled(
            status_line(&timings, scrub),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, WIN_H - 22)),
    );

    Scene::Container(
        ContainerNode::new(vec![flame, scrub_surface, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. The Slider is the playhead position (primary);
/// `#[widget]` derives WidgetCore + WidgetView, and `a11y_manual` provides the
/// hand-written [`WidgetA11y`] below (the scrub Slider describedby the playhead
/// readout region).
#[widget(
    tag = "timeline_scrub",
    state = (SliderState, f32),
    event = SliderEvent,
    title = "pinion hello-timeline (R1389 §5.16 render-phase flame + playhead)",
    renderer = HelloTimelineRenderer,
    initial_size = (WIN_W, WIN_H),
    external = scrub_external,
    apply_key,
    keybinding,
    event_name_derive,
    a11y_manual,
)]
struct TimelineView;

impl TimelineView {
    /// Reads the playhead fraction from the primary Slider external.
    fn read_state(scene: &Scene) -> (SliderState, f32) {
        read_slider_state(scene, SCRUB_TAG).unwrap_or((SliderState::Idle, 0.5))
    }

    fn view(state: (SliderState, f32), frame: Frame) -> Scene {
        view(state.1, &frame)
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
            "ArrowLeft" | "ArrowDown" => Some((current - 0.05).clamp(0.0, 1.0)),
            "ArrowRight" | "ArrowUp" => Some((current + 0.05).clamp(0.0, 1.0)),
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

/// The scrub surface carries `AriaRole::Slider` with the playhead fraction as
/// its `AccessValue::Float`, and is `describedby` the timeline's playhead region
/// so the scrubbed time + the span under it a sighted user reads in the tooltip
/// reach a screen reader too (the R1355 parity, now for the timeline). Computed
/// with the DEFAULT style but the SAME `CHART_RECT` the paint pins to — the
/// geometry is set by the rect + margins + tick targets, and `chart_style`
/// overrides only colours, so this resolves the identical playhead the themed
/// overlay draws (the same reasoning `hello-scatter` documents). It runs inside
/// the owner scope (`access_node` is wrapped by `collect_access_emit_inputs`),
/// so `use_frame_timings` here reads the same published window the view built.
impl WidgetA11y for TimelineView {
    fn access_node(state: &(SliderState, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, scrub) = (state.0, state.1);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            ..AccessState::from_interaction(interaction, None)
        };
        let timings = use_frame_timings();
        let readout =
            timeline(&timings, scrub).playhead_readout(CHART_RECT, &ChartStyle::default());
        let has_readout = readout.is_some();
        // R1692 — a transparent capture surface has no contents to be named
        // from, so an unauthored name reaches a reader as "slider" and nothing.
        let control = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Slider)
            .with_name("Playhead position".to_owned())
            .with_value(AccessValue::Float {
                value: scrub,
                min: 0.0,
                max: 1.0,
            })
            .with_state(access_state);
        describedby_region(
            control,
            "timeline.playhead.tooltip",
            AriaRole::Tooltip,
            readout,
            has_readout,
        )
    }
}

fn main() {
    pinion_shell::run::<TimelineView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_runtime::{FRAME_TIMINGS, FrameTimingStats};

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

    /// A `FrameTimingsView` from a run of total frame times (a plausible phase
    /// partition each — the same seed shape `hello-frame-profiler` uses).
    fn seeded(totals: &[u64]) -> FrameTimingsView {
        let mut stats = FrameTimingStats::new();
        for &total in totals {
            let (b, e, r) = (total / 4, total / 8, total / 8);
            let acquire = total.saturating_sub(b + e + r) / 2;
            stats.record(FrameTiming::new(b, e, acquire, r, total));
        }
        FrameTimingsView {
            samples: stats.samples().copied().collect(),
            snapshot: stats.snapshot(None),
        }
    }

    fn rendered(view: &FrameTimingsView, scrub: f32) -> Scene {
        let owner = Owner::new();
        // Publish the seam so `use_frame_timings` inside the view resolves it.
        FRAME_TIMINGS.resolve(&owner).publish(view.clone());
        owner.run(|| super::view(scrub, &Frame::new()))
    }

    #[test]
    fn flame_has_one_span_per_phase_per_shown_frame() {
        let view = seeded(&[4_000, 5_000, 6_000]);
        let scene = rendered(&view, 0.5);
        assert!(find(&scene, "timeline").is_some(), "the timeline root");
        assert!(
            find(&scene, SCRUB_TAG).is_some(),
            "the scrub capture surface"
        );
        // 4 phases x 3 frames = 12 span boxes, and a lane label per phase.
        assert_eq!(
            count_prefix(&scene, "timeline.lane.0.span."),
            3,
            "build lane: 3 spans"
        );
        for (i, (name, _)) in PHASES.iter().enumerate() {
            let label = find(&scene, &format!("timeline.lane.{i}.label"));
            let Some(Scene::Text(t)) = label else {
                panic!("lane {i} has a name label")
            };
            assert_eq!(t.content, *name, "lane {i} is named {name}");
        }
    }

    #[test]
    fn a_longer_frame_makes_a_wider_acquire_span() {
        // acquire = (total - build - encode - render)/2, so it grows with total.
        let view = seeded(&[4_000, 20_000]);
        let scene = rendered(&view, 0.5);
        let acquire_w = |j: usize| {
            let Scene::Box(b) = find(&scene, &format!("timeline.lane.2.span.{j}")).unwrap() else {
                panic!("acquire span is a box")
            };
            b.rect.w
        };
        assert!(
            acquire_w(1) > acquire_w(0),
            "the 20ms frame's acquire span is wider"
        );
    }

    #[test]
    fn the_flame_shows_only_the_last_recent_frames() {
        // More than RECENT frames -> only the last RECENT are laid out.
        let totals: Vec<u64> = (0..(RECENT as u64 + 10)).map(|k| 4_000 + k * 10).collect();
        let view = seeded(&totals);
        let scene = rendered(&view, 0.5);
        assert_eq!(
            count_prefix(&scene, "timeline.lane.0.span."),
            RECENT,
            "the flame is capped at the last RECENT frames"
        );
    }

    #[test]
    fn playhead_reads_the_active_frame_and_phase() {
        let view = seeded(&[4_000, 5_000, 6_000]);
        let scene = rendered(&view, 0.5);
        assert!(
            find(&scene, "timeline.playhead").is_some(),
            "the playhead line"
        );
        assert!(
            find(&scene, "timeline.playhead.header").is_some(),
            "the time header"
        );
        // Somewhere in the flame a lane's span is under the playhead.
        assert!(
            count_prefix(&scene, "timeline.playhead.value.") >= 1,
            "the playhead names at least one active lane/frame"
        );
    }

    #[test]
    fn no_frames_yet_draws_the_ruler_but_no_spans() {
        let empty = FrameTimingsView::default();
        let scene = rendered(&empty, 0.5);
        assert!(
            find(&scene, "timeline.axis.x").is_some(),
            "the ruler survives an empty window"
        );
        assert!(
            find(&scene, "timeline.lane.0.span.0").is_none(),
            "no spans without frames"
        );
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TimelineView>(
            (SliderState::Idle, 0.5),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<TimelineView>(
            (SliderState::Idle, 0.5),
            &Frame::new(),
        );
    }

    #[test]
    fn scrub_reports_slider_role_and_is_describedby_the_playhead_region() {
        let view = seeded(&[4_000, 5_000, 6_000]);
        let owner = Owner::new();
        owner.run(|| FRAME_TIMINGS.resolve(&owner).publish(view.clone()));
        let nodes = owner
            .run(|| <TimelineView as WidgetA11y>::access_node(&(SliderState::Idle, 0.5), None));
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert_eq!(nodes[0].tag, SCRUB_TAG);
        assert_eq!(
            nodes[0].described_by.as_deref(),
            Some("timeline.playhead.tooltip"),
            "scrub is describedby the playhead region"
        );
        let region = nodes
            .iter()
            .find(|n| n.tag == "timeline.playhead.tooltip")
            .expect("the described region is in the tree");
        let name = region.name.as_deref().expect("region carries the readout");
        assert!(
            name.starts_with("t = "),
            "the readout leads with the scrubbed time: {name:?}"
        );
    }
}
