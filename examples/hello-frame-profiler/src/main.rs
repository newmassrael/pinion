//! `hello-frame-profiler` — R907 §5.16 §5.7 consumer of the frame-timing
//! profiler substrate, retrofitted at **R1361** into a real frame-time
//! *chart*: `pinion-chart`'s forcing consumer with genuinely measured data.
//!
//! ## What this demonstrates
//!
//! The §1 northern-star's pro-tool-performance axis is "measure first": a
//! frame-budget tuning round can only be evidence-based if the frame cost is
//! *readable*. R907 instruments `AppShell::render_window` to bracket every
//! painted frame into phases — **build** (`view` fn + layout), **encode**
//! (structured scene → `vello` fragments), **acquire** (the vsync block,
//! split out by R1361.1) and **render** (GPU command-buffer submit) — plus
//! the **total** productive frame span. Each sample feeds a per-window
//! rolling window (`FrameTimingStats`).
//!
//! R907 printed those numbers as a text table. This binding plots them: the
//! last 120 frames as one series per phase, plus `work` beside `total`,
//! against the window's declared frame budget. That is `stat unit` / the Chrome frame-history strip — the shape
//! every pro tool reaches for, because the *series* carries information the
//! aggregate destroys (a steady 8ms window and one alternating 1ms/15ms
//! report the same mean; only the chart tells them apart).
//!
//! ## Why this binding, and not synthetic data
//!
//! `hello-chart` / `hello-chart-fill` feed the chart a programmatic `sin`
//! wave. That proves the geometry but nothing about fitness for purpose: a
//! chart crate is only shown to work when something real is *forced* through
//! it. This binding is that forcing consumer — the data is measured, arrives
//! one sample per painted frame, and is not reproducible run to run. Every
//! property the demo asserts therefore has to be a *structural* invariant
//! rather than a pinned value, which is exactly the discipline a live
//! dashboard needs.
//!
//! It also closes R1360's declared gap. `hello-chart-fill` proved
//! `build_fill` under a window resize; nothing proved it under a **flex
//! slot** whose size is decided by siblings. Here the chart's slot is a
//! `flex_grow` row sharing a column with a title, a status line and a chip,
//! so its height is "whatever the chrome left over" — layout-derived in a
//! way no window constant could supply.
//!
//! ## The two seams this needs (R1361)
//!
//! 1. **`use_frame_timings`** — the view fn is pure and sync (§6.3) and had
//!    no way to reach the profiler at all: the stats live on `ShellCore`.
//!    The seam publishes the rolling history + the same aggregate snapshot
//!    `scene/frame_timings` returns, so this HUD and an AI client read
//!    identical numbers.
//! 2. **The measured-rect seam** (R1012), reused exactly as
//!    `hello-chart-fill` documents it: `use_pane_viewport_size(CHART_TAG)`
//!    → `build_fill` → the shell's post-layout publish + same-frame re-pass.
//!
//! ## Pacing, and why the budget line is conditional
//!
//! A budget line is only honest when the window *has* a deadline. An idle
//! retained window has none — `budget_us` is `None` and "missed budget" is
//! meaningless — so a hardcoded 16.7ms rule would draw a deadline that does
//! not exist. The line therefore appears if and only if a frame target is
//! declared, via the already-ratified `scene/set_fps`.
//!
//! That single declaration does both jobs, which is why they are not two
//! knobs: `scene/set_fps 60` makes `frame_budget_for_window` return
//! `Some(16_666)`, which (a) makes `about_to_wait` re-arm a redraw and pace
//! to a `WaitUntil` deadline, and (b) is the very same value the profiler
//! judges jank against and this HUD draws. Set `scene/set_fps 0` and both go
//! away together, and both halves are **measured**, not argued: with zero
//! input for one second, an unpaced window paints `+0` frames (it sleeps) and
//! a `set_fps 60` window paints `+64` at a reported `mean_fps` of 61.9.
//! `r907_frame_profiler.py` asserts both.
//!
//! That `+0` is also the end-to-end proof of this binding's load-bearing
//! design decision: a HUD reading `use_frame_timings` every paint does **not**
//! make the window paint. Had the seam been a `Signal`, the idle row would
//! read `+hundreds` instead of `+0`.
//!
//! Nothing here declares a target at startup, deliberately: an unpaced
//! window is the correct default for a retained tree, and `r925_frame_budget`
//! pins that contract (section A: no target ⇒ no budget).
//!
//! ## What this chart found on its first run, and R1361.1 fixed
//!
//! The point of a forcing consumer is that it finds things, and this one did
//! so immediately: the plot came up dominated by a huge flat `render` line
//! against a `build` of well under a millisecond.
//!
//! That was not this HUD being slow, nor the chart lying. `render_us`
//! brackets `WidgetRenderer::render`, and the forge-generated `render` calls
//! `surface.get_current_texture()` *inside* it under
//! `PresentMode::AutoVsync` — so the phase contained a **blocking wait for a
//! swapchain image**. `render_us` had measured "render + wait-for-image"
//! since R907 while its doc claimed "CPU-side cost only", which made a
//! vsync-blocked window read exactly like a GPU-bound one.
//!
//! **R1361.1 split it out.** The forge template records its own acquire span,
//! `WidgetRenderer::last_acquire_us` reports it (defaulting to `0` for
//! backends that cannot block), and the shell subtracts it. Measured on this
//! window, idle machine, in µs:
//!
//! | | total | work | build | encode | acquire | render |
//! |---|---|---|---|---|---|---|
//! | unpaced | 1137 | 1107 | 139 | 105 | **9** | 863 |
//! | `scene/set_fps 60` | 16273 | **696** | 214 | 83 | **15559** | 399 |
//!
//! The paced frame is 96% block and 0.7ms of work — the textbook
//! vsync-bound picture (theory pins `total` at one 60Hz interval, `16_667µs`; the acquire is
//! what is left of it after the work, `15559 ≈ 16667 - 696 - other`).
//! Pre-R1361.1 its `render_us` would have read **`15_958µs`**: "rendering
//! takes 16ms", the opposite of the truth. One machine, one run — the ratio
//! is the claim, not the absolutes.
//!
//! The chart plots `acquire` and `render` separately, plus `work` beside
//! `total`: **the gap between the `total` and `work` lines is the vsync
//! block**, which is the reading a profiler exists to give.
//!
//! ## The observer effect, stated plainly
//!
//! A profiler HUD measures a window it is itself drawing into: its
//! 120-point paths re-hash every frame (a readout that cached would be a
//! readout that stopped updating), so `build_us` here includes this chart's
//! own construction. These numbers describe *this* window and are not a
//! baseline for an idle pinion app. Every in-app profiler shares this;
//! naming it keeps a later round from quoting these figures as framework
//! performance.
//!
//! ## Verification
//!
//! `tools/demos/r907_frame_profiler.py` (structural invariants on
//! non-deterministic data over `scene/frame_timings`) and
//! `tools/demos/r925_frame_budget.py` (budget / jank) both still drive this
//! binding unchanged. The tests below add the scene-side half: that the
//! chart is fed by the seam, tracks its flex slot, and draws a budget line
//! exactly when a budget exists.

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue,
    TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{
    ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_pane_viewport_size, use_theme,
};
use pinion_derive::widget;
use pinion_runtime::{FrameTiming, FrameTimingsView, use_frame_timings};
use pinion_shell::{SizeStrategy, vello_renderer_impl};

// pinion-forge codegen output: `pub struct HelloFrameProfilerRenderer`
// + error + async `new` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can drive it.
vello_renderer_impl!(HelloFrameProfilerRenderer, HelloFrameProfilerRendererError);

/// Opening size only. Unlike the pre-R1361 cut — whose window was
/// `SizeStrategy::Fixed`, so its "resizable" claim was never true — nothing
/// about the chart's geometry derives from these; drag the window to any
/// size and the plot re-scales.
const WIN_W: u32 = 760;
const WIN_H: u32 = 520;

const THEME_TAG: &str = "app";
const REPAINT_TAG: &str = "repaint";

/// The chart root's tag: BOTH the introspection prefix (`chart.series.0` …)
/// and the measured-rect seam's key, so the thing measured is by
/// construction the thing painted.
const CHART_TAG: &str = "chart";

const TITLE_FONT_PX: u32 = 16;
const ROW_FONT_PX: u32 = 14;
const STATUS_FONT_PX: u32 = 12;

const CHIP_W: u32 = 240;
const CHIP_H: u32 = 24;

/// Material 3 state-layer overlay weights for the repaint chip chrome.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// The plotted phases, in the canonical `stat unit` reading order: the
/// headline span first, then the disjoint sub-intervals that partition it.
/// `total >= build + encode + acquire + render` holds by construction, so
/// the total line is an envelope over the rest — the visual form of the
/// substrate's own invariant.
///
/// `work` (= `build + encode + render`, the acquire excluded) is plotted
/// beside `total` because the gap between the two lines IS the vsync block:
/// when they separate, the window is waiting, not labouring. That gap is
/// what R1361.1 made visible — before it, the block hid inside `render`.
type PhasePick = fn(&FrameTiming) -> u64;
const PHASES: [(&str, PhasePick); 6] = [
    ("total", |s| s.total_us),
    ("work", |s| s.work_us()),
    ("build", |s| s.build_us),
    ("encode", |s| s.encode_us),
    ("acquire", |s| s.acquire_us),
    ("render", |s| s.render_us),
];

/// Microseconds → milliseconds.
///
/// ms because that is the unit a frame budget is quoted in — "16.7ms", not
/// "16666µs" — and the y-axis exists to be read against that budget.
#[allow(
    clippy::cast_precision_loss,
    reason = "a frame span in µs is far below 2^53; the conversion is exact"
)]
fn ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

/// Sample index → x. Lossless: the ring is capped at `FRAME_TIMING_WINDOW`
/// (120), so the index always fits a `u32` exactly.
fn sample_x(index: usize) -> f64 {
    f64::from(u32::try_from(index).unwrap_or(u32::MAX))
}

/// The plotted series: one per phase, oldest sample on the left, plus the
/// budget line when the window has a declared deadline.
///
/// The budget rides the *same* `Series` primitive as the data rather than a
/// bespoke "threshold" chart API. Reusing it is not a shortcut — it buys
/// three things a special case would have to re-earn: the line joins the
/// auto-derived y-domain (so the budget is always in frame, and a window
/// running well under it visibly reads as headroom), it earns a legend entry
/// naming its value, and it needs no new `pinion-chart` surface for a single
/// consumer ([[abstraction-needs-second-consumer]]).
fn timing_series(view: &FrameTimingsView, budget: Color) -> Vec<Series> {
    let mut series: Vec<Series> = PHASES
        .iter()
        .map(|(name, pick)| {
            let points = view
                .samples
                .iter()
                .enumerate()
                .map(|(i, s)| DataPoint::new(sample_x(i), ms(pick(s))))
                .collect();
            Series::new(*name, points)
        })
        .collect();

    // Only when the window actually has a deadline — see the module doc.
    if let Some(budget_us) = view.snapshot.and_then(|s| s.budget_us) {
        let last = sample_x(view.samples.len().saturating_sub(1));
        let y = ms(budget_us);
        series.push(
            Series::new(
                format!("{y:.1}ms budget"),
                vec![DataPoint::new(0.0, y), DataPoint::new(last, y)],
            )
            .with_color(budget),
        );
    }
    series
}

/// The status readout. Every number here comes from the published snapshot —
/// the same fold `scene/frame_timings` returns — so the HUD cannot disagree
/// with what an AI client reads over RPC.
fn status_line(view: &FrameTimingsView) -> String {
    let Some(s) = view.snapshot else {
        return "no frames measured yet — the first paint records the first sample".to_string();
    };
    let pacing = match s.budget_us {
        None => "unpaced (no deadline; `scene/set_fps 60` to declare one)".to_string(),
        Some(b) => format!(
            "budget {:.1}ms | {} of {} frames over ({:.0}% jank)",
            ms(b),
            s.over_budget_frames,
            s.window_len,
            f64::from(s.jank_ratio) * 100.0,
        ),
    };
    format!(
        "frame {} | {:.1} fps | last {:.2}ms | window min {:.2} / mean {:.2} / max {:.2}ms | {pacing}",
        s.frame_count,
        f64::from(s.mean_fps),
        ms(s.last.total_us),
        ms(s.min_total_us),
        ms(s.mean_total_us),
        ms(s.max_total_us),
    )
}

/// The chart style, resolved from the live theme so the plot tracks
/// light/dark (series colours come from the crate's Okabe-Ito palette and
/// are theme-independent by design).
fn chart_style(theme: &pinion_core::Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        ..ChartStyle::default()
    }
}

/// view-fn (§6.3): pure sync `(ToggleState, bool) -> Scene`.
///
/// Pure *of published state*, which is what the `dry_run` guarantee needs:
/// the wall-clock non-determinism enters at the shell's publish, exactly as
/// an animation's `dt` enters at `tick_animations_for_window` rather than
/// here.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let chip_fill = theme.resolve(ColorRole::SurfaceContainerHighest);

    // The two seams: what to draw, and how big to draw it.
    let timings = use_frame_timings();
    let (cw, ch) = use_pane_viewport_size(CHART_TAG);

    let title = Scene::Text(
        TextNode::styled(
            "Frame profiler — total / work / build / encode / acquire / render, last 120 frames",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(
            LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );

    let chart = LineChart::new(timing_series(&timings, theme.resolve(ColorRole::Error)))
        .build_fill((cw, ch), &chart_style(&theme));

    // The chart's slot: a flex-grow row. Its height is whatever the title,
    // status and chip left over — layout-derived, which is the property
    // R1360 shipped but had no flex-slot consumer to prove.
    let slot = Scene::Container(
        ContainerNode::new(vec![chart]).with_layout(
            LayoutStyle::new()
                .with_flex_grow(1.0)
                .with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );

    let status = Scene::Text(
        TextNode::styled(
            status_line(&timings),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(
            LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );

    // Repaint trigger — the Toggle hit handle. Each click flips `on`,
    // re-runs the view, and produces one more measured frame. It is the
    // manual driver for an UNPACED window; declaring a frame target
    // (`scene/set_fps`) makes frames stream on their own instead.
    let repaint_fill: Color = match state {
        ToggleState::Idle => chip_fill,
        ToggleState::Hover => chip_fill.lerp(on_surface, HOVER_OVERLAY_T),
        ToggleState::Pressed => chip_fill.lerp(on_surface, PRESSED_OVERLAY_T),
        ToggleState::Disabled => chip_fill.lerp(surface, DISABLED_OVERLAY_T),
    };
    let repaint_label = Scene::Text(TextNode::styled(
        format!(
            "Repaint ({}) — {}",
            if on { "B" } else { "A" },
            state.as_name()
        ),
        Rect::default(),
        TextStyle::new()
            .with_size_px(ROW_FONT_PX)
            .with_fg(on_surface),
    ));
    let repaint = Scene::Container(
        ContainerNode::new(vec![repaint_label])
            .with_tag(REPAINT_TAG)
            .with_aria_label("Trigger a repaint")
            .with_style(BoxStyle::filled(repaint_fill).with_corner_radius(6))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(CHIP_W, CHIP_H)),
            ),
    );

    // The root MUST fill an OPAQUE theme surface: nothing else covers the
    // window, so without it the chrome sits on the compositor's black under
    // text coloured for a light surface (the R1360.2 class — two gallery
    // demos shipped that way).
    Scene::Container(
        ContainerNode::new(vec![title, slot, status, repaint])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(10, 10, 10, 10))
                    .with_gap(8)
                    .with_size(
                        Size::auto()
                            .with_width(SizeValue::Percent(100))
                            .with_height(SizeValue::Percent(100)),
                    ),
            ),
    )
}

/// `WidgetView` binding. The §5.38 Toggle is reused as the RPC-drivable
/// repaint trigger (see the module doc); `#[widget]` derives the mechanical
/// [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`] trio.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "repaint",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-frame-profiler (R1361 §5.16 live frame-time chart)",
    renderer = HelloFrameProfilerRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ToggleExternal::new,
    role = Switch,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
        checked = bool_field(1),
    ),
    access_value = bool_field(1),
    apply_key,
    keybinding,
    initial_size_strategy,
)]
struct ProfilerView;

impl ProfilerView {
    /// Resizable on purpose: the chart's slot is layout-derived, so the
    /// resize is part of what this binding demonstrates. The pre-R1361 cut
    /// declared only `initial_size`, which the derive lowers to
    /// `SizeStrategy::Fixed` — the window could not be resized at all.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::OpenResizable {
            size: (WIN_W, WIN_H),
            min: Some((320, 240)),
        }
    }

    /// Tuple-state introspect: SCXML state name + the repaint bit.
    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    ToggleState::from_name_or_default(&name)
                } else {
                    ToggleState::Idle
                };
                let value = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
                return (state, value);
            }
        }
        (ToggleState::Idle, false)
    }

    /// R641 §5.16 inherent view shim — unpacks the tuple and forwards.
    fn view(state: (ToggleState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    fn event_name(event: ToggleEvent) -> &'static str {
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn keybinding(key: &str) -> Option<ToggleEvent> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }

    /// ARIA toggle-button keyboard activation (Space / Enter flips the
    /// repaint bit in parity with a pointer click).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

fn main() {
    pinion_shell::run::<ProfilerView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_runtime::{FRAME_TIMINGS_KEY, FrameTimingStats, FrameTimingsHolder, compute_layout};

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        match scene {
            Scene::Container(c) => c.children.iter().find_map(|ch| find(ch, tag)),
            Scene::Scroll(s) => find(&s.content, tag),
            _ => None,
        }
    }

    /// Seed the seam the way the shell does: build a `FrameTimingsView`
    /// from ONE `FrameTimingStats` (samples + snapshot together) and
    /// publish it into the owner's holder.
    fn seed(owner: &Owner, totals: &[u64], budget_us: Option<u64>) {
        let mut stats = FrameTimingStats::new();
        for &total in totals {
            // A plausible partition: total >= build + encode + acquire + render.
            let (b, e, r) = (total / 2, total / 4, total / 8);
            stats.record(FrameTiming::new(b, e, 0, r, total));
        }
        let view = FrameTimingsView {
            samples: stats.samples().copied().collect(),
            snapshot: stats.snapshot(budget_us),
        };
        owner
            .run(|| owner.cache(FRAME_TIMINGS_KEY, FrameTimingsHolder::default))
            .publish(view);
    }

    /// One paint cycle driving the REAL measured-rect seam, exactly as
    /// `hello-chart-fill` does: view → layout → publish → (dirty ⇒ re-view
    /// + re-layout). Nothing seeds a rect by hand.
    fn paint_cycle(core: &pinion_runtime::CoreShell<ProfilerView>, w: u32, h: u32) -> Scene {
        let mut cache = pinion_text::LayoutCache::new();
        let run_view = || {
            core.root_owner()
                .run(|| view(ToggleState::Idle, false, &Frame::new()))
        };
        let mut scene = run_view();
        compute_layout(&mut scene, &mut cache, w, h);
        if core.publish_pane_viewports(&scene) {
            scene = run_view();
            compute_layout(&mut scene, &mut cache, w, h);
        }
        scene
    }

    #[test]
    fn r1361_view_charts_the_seam_not_a_synthetic_series() {
        // The reviewers' point, as a test: the plotted geometry must come
        // from the profiler seam. Seed two different histories through the
        // seam and the painted series must differ — a view drawing a
        // canned series would paint identically for both.
        let core: pinion_runtime::CoreShell<ProfilerView> = pinion_runtime::CoreShell::new();
        seed(core.root_owner(), &[2_000, 3_000, 2_500], None);
        let calm = paint_cycle(&core, 760, 520);
        let calm_hash = find(&calm, "chart.series.0")
            .expect("total series")
            .paint_hash();

        seed(core.root_owner(), &[2_000, 25_000, 2_500], None);
        let spiky = paint_cycle(&core, 760, 520);
        let spiky_hash = find(&spiky, "chart.series.0")
            .expect("total series")
            .paint_hash();

        assert_ne!(
            calm_hash, spiky_hash,
            "the plotted series must be derived from the published frame \
             timings — identical geometry for different histories means the \
             chart is fed by something other than the seam",
        );
    }

    #[test]
    fn r1361_unmeasured_seam_paints_no_series_but_still_measures_its_slot() {
        // The bootstrap: before any frame is recorded there is nothing to
        // plot, but the chart root must still take its slot — that is what
        // lets the measured-rect seam converge on the very first paint.
        let core: pinion_runtime::CoreShell<ProfilerView> = pinion_runtime::CoreShell::new();
        let scene = paint_cycle(&core, 760, 520);
        assert!(
            find(&scene, "chart.series.0").is_none(),
            "no measured frames ⇒ no series"
        );
        let root = find(&scene, CHART_TAG).expect("chart root present").rect();
        assert!(
            root.w > 0 && root.h > 0,
            "the unmeasured chart root still fills its flex slot, got {root:?}",
        );
    }

    #[test]
    fn r1361_budget_line_appears_only_when_the_window_has_a_deadline() {
        // The honesty invariant. An unpaced window has no deadline, so a
        // budget line would assert one that does not exist; a paced window
        // must draw the deadline it is actually paced to.
        let core: pinion_runtime::CoreShell<ProfilerView> = pinion_runtime::CoreShell::new();

        // Derived, not hardcoded: the budget is appended after the phases,
        // so its index moves whenever PHASES does. (It was pinned to `4`
        // first, and adding the R1361.1 acquire+work series silently made
        // that assertion point at a phase line instead.)
        let budget_series = format!("chart.series.{}", PHASES.len());

        seed(core.root_owner(), &[2_000, 3_000], None);
        let unpaced = paint_cycle(&core, 760, 520);
        assert!(
            find(&unpaced, &budget_series).is_none(),
            "an unpaced window must draw NO budget line — 'missed budget' is \
             meaningless without a declared frame target",
        );

        seed(core.root_owner(), &[2_000, 3_000], Some(16_666));
        let paced = paint_cycle(&core, 760, 520);
        assert!(
            find(&paced, &budget_series).is_some(),
            "a paced window draws its budget as the series after the phases",
        );
    }

    #[test]
    fn r1361_budget_series_is_flat_at_the_declared_budget() {
        // The line must sit AT the budget, not merely exist. Built from the
        // published budget_us, so it tracks whatever target was declared.
        let view = FrameTimingsView {
            samples: vec![FrameTiming::new(1, 1, 0, 1, 4_000)],
            snapshot: FrameTimingStats::default().snapshot(None),
        };
        assert_eq!(
            timing_series(&view, Color::rgb(0, 0, 0)).len(),
            PHASES.len(),
            "no snapshot ⇒ no budget series",
        );

        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(1_000, 500, 0, 250, 4_000));
        stats.record(FrameTiming::new(1_000, 500, 0, 250, 4_000));
        let paced = FrameTimingsView {
            samples: stats.samples().copied().collect(),
            snapshot: stats.snapshot(Some(16_666)),
        };
        let series = timing_series(&paced, Color::rgb(0, 0, 0));
        assert_eq!(series.len(), PHASES.len() + 1, "budget series appended");
        let budget = series.last().unwrap();
        assert!(
            budget.points.iter().all(|p| (p.y - 16.666).abs() < 1e-3),
            "the budget line is flat at 1e6/60 µs = 16.666ms, got {:?}",
            budget.points,
        );
        assert!(
            budget.name.contains("16.7ms"),
            "the legend names the budget's value, got {:?}",
            budget.name,
        );
    }

    #[test]
    fn r1361_chart_tracks_its_flex_slot_not_a_window_constant() {
        // R1360's declared gap: `build_fill` was proven under a window
        // resize but never under a FLEX slot sized by its siblings. The
        // chart's height here is "whatever the title/status/chip left".
        let core: pinion_runtime::CoreShell<ProfilerView> = pinion_runtime::CoreShell::new();
        seed(core.root_owner(), &[2_000, 3_000, 2_500], Some(16_666));

        let big = paint_cycle(&core, 760, 520);
        let small = paint_cycle(&core, 480, 320);
        let (b, s) = (
            find(&big, CHART_TAG).unwrap().rect(),
            find(&small, CHART_TAG).unwrap().rect(),
        );
        assert!(
            b.w > s.w && b.h > s.h,
            "the chart root tracks its slot: 760x520 -> {b:?} must exceed 480x320 -> {s:?}",
        );
        // …and it is strictly SHORTER than the window, because the chrome
        // takes its share first. That is the flex-slot property a
        // window-sized chart would not have.
        assert!(
            b.h < 520,
            "the chart yields height to the title/status/chip, got {b:?}",
        );
        assert!(
            find(&small, "chart.series.0").is_some(),
            "the resized chart still plots its series",
        );
    }

    #[test]
    fn r1361_status_reports_the_published_snapshot_not_a_recomputed_one() {
        // The HUD's numbers must be the snapshot's — the same fold
        // `scene/frame_timings` returns — so RPC and pixels cannot drift.
        let mut stats = FrameTimingStats::new();
        stats.record(FrameTiming::new(1_000, 500, 0, 250, 4_000));
        let view = FrameTimingsView {
            samples: stats.samples().copied().collect(),
            snapshot: stats.snapshot(Some(16_666)),
        };
        let line = status_line(&view);
        assert!(line.contains("frame 1"), "frame_count echoed: {line}");
        assert!(line.contains("4.00ms"), "last total echoed: {line}");
        assert!(line.contains("budget 16.7ms"), "budget echoed: {line}");
        assert!(line.contains("0 of 1 frames over"), "jank echoed: {line}");

        let unpaced = FrameTimingsView {
            samples: stats.samples().copied().collect(),
            snapshot: stats.snapshot(None),
        };
        assert!(
            status_line(&unpaced).contains("unpaced"),
            "an unpaced window says so instead of implying a deadline",
        );
        assert!(
            status_line(&FrameTimingsView::default()).contains("no frames measured yet"),
            "the bootstrap state is named, not rendered as zeros",
        );
    }

    #[test]
    fn repaint_toggle_relabels_per_flip() {
        // The off/on bit must change the painted tree so each click is a
        // distinct paint the profiler records (re-keys the paint hash).
        let owner = Owner::new();
        let off = owner.run(|| view(ToggleState::Idle, false, &Frame::new()));
        let on = owner.run(|| view(ToggleState::Idle, true, &Frame::new()));
        assert_ne!(off.paint_hash(), on.paint_hash());
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ProfilerView>(
            (ToggleState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<ProfilerView>(
            (ToggleState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn repaint_chip_reports_switch_role() {
        let nodes = <ProfilerView as WidgetA11y>::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, REPAINT_TAG);
    }
}
