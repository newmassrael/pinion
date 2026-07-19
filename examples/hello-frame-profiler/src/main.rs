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

use pinion_a11y::described::describedby_region;
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{Bar, BarChart, ChartStyle, DataPoint, LineChart, Series};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue,
    TextStyle,
};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::slider::SliderExternal;
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

/// The distribution histogram's tag — its own introspection prefix
/// (`histogram.bar.0` …) and measured-rect seam key, the bar-chart twin of
/// [`CHART_TAG`]. The timeline (line) answers "how did each frame do over
/// time"; the histogram answers "how OFTEN do I land in each duration band",
/// the distribution the time-series cannot show.
const HIST_TAG: &str = "histogram";

/// The transparent capture surface over the histogram panel — the press-drag
/// scrub's tag (R1375). It hosts a sibling [`SliderExternal`] (a captured 1-D
/// fraction, RPC-drivable) whose value the histogram's
/// [`BarChart::inspect`](pinion_chart::BarChart::inspect) turns into a
/// highlight ring + a per-bin value tooltip. pinion only forwards a continuous
/// pointer position under capture (free hover reports only WHICH tag), so the
/// inspector is a drag over this surface — the same design `hello-chart` uses
/// for the line chart's scrub.
const HIST_SCRUB_TAG: &str = "hist_scrub";

/// The distribution histogram's bucket count. 12 keeps each bar wide enough to
/// read + label across the panel while still resolving a bimodal frame-time
/// distribution (a calm cluster + a jank tail) — the Chrome-DevTools
/// frame-histogram granularity, not a tuned value.
const HIST_BINS: usize = 12;

/// Fixed height (px) of the histogram panel below the flex-grow timeline.
const HIST_H: u32 = 150;

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

/// Bin the rolling window's total frame times into a distribution: `HIST_BINS`
/// equal-width ms buckets from 0 to the window max, each bar's height the count
/// of frames in that band (the labels are the buckets' lower ms edges). A
/// bucket whose LOWER edge is at or past the frame budget is painted in the
/// `over` (jank) colour — the R925 over-budget classification made visual, so a
/// cluster of bars past the budget reads as how OFTEN (and how far) this window
/// janks, the distribution the timeline cannot show. No samples yet ⇒ no bars
/// (the chart still draws its empty axes).
///
/// The budget line the timeline draws is a horizontal threshold; here the same
/// deadline is a colour boundary along the x-axis — the two encodings of the
/// one `budget_us` the shell publishes, never diverging.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a bin index is 0..HIST_BINS and a frame count is a small window \
              length; both are far below f64/usize precision limits and the \
              index is clamped into range"
)]
fn histogram_bars(view: &FrameTimingsView, under: Color, over: Color) -> Vec<Bar> {
    if view.samples.is_empty() {
        return Vec::new();
    }
    let max_ms = view
        .samples
        .iter()
        .map(|s| ms(s.total_us))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let width = max_ms / HIST_BINS as f64;
    let budget_ms = view.snapshot.and_then(|s| s.budget_us).map(ms);
    let mut counts = [0u32; HIST_BINS];
    for s in &view.samples {
        let idx = ((ms(s.total_us) / width).floor() as usize).min(HIST_BINS - 1);
        counts[idx] += 1;
    }
    (0..HIST_BINS)
        .map(|k| {
            let lo = k as f64 * width;
            let color = match budget_ms {
                Some(b) if lo >= b => over,
                _ => under,
            };
            Bar::new(format!("{lo:.0}"), f64::from(counts[k])).with_color(color)
        })
        .collect()
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

/// The distribution [`BarChart`], built identically for the painted panel and
/// the a11y readout (R1375) so the two cannot disagree — the R1355 parity, now
/// for the bar chart. `scrub` is the inspect fraction across the panel.
fn histogram_chart(view: &FrameTimingsView, theme: &pinion_core::Theme, scrub: f32) -> BarChart {
    BarChart::new(histogram_bars(
        view,
        theme.resolve(ColorRole::Accent),
        theme.resolve(ColorRole::Error),
    ))
    .with_tag_prefix(HIST_TAG)
    .inspect(Some(scrub))
}

/// R1249 — the histogram scrub as an `External`, dispatched by tag. A
/// [`SliderExternal`] (a captured 1-D fraction) seeded to the panel centre so
/// the boot frame shows a centred inspector, exactly as `hello-chart`'s scrub.
fn hist_scrub_extras() -> Vec<ExtraExternal> {
    let mut slider = SliderExternal::new();
    slider.set_value(0.5);
    vec![ExtraExternal::new(HIST_SCRUB_TAG, Box::new(slider))]
}

/// Read the histogram scrub fraction `0.0..=1.0` from the sibling external. A
/// missing external falls back to the panel centre.
fn read_scrub(scene: &Scene) -> f32 {
    scene
        .find_external_with_tag(HIST_SCRUB_TAG)
        .and_then(|node| node.handle.introspect())
        .and_then(|intro| intro.query("value"))
        .and_then(|v| v.as_f32())
        .unwrap_or(0.5)
}

/// R1374 — the distribution histogram panel below the timeline: the SAME
/// window's total frame times, binned. Under-budget bars in the accent,
/// over-budget (jank) bars in the error colour — the timeline's horizontal
/// budget line re-encoded as a colour boundary along the x-axis. A fixed-height
/// slot (the timeline flex-grows above it), on its own `HIST_TAG` measured-rect
/// seam (`size` = the slot's measured px).
///
/// R1375 — a transparent `HIST_SCRUB_TAG` capture surface fills the panel (the
/// scrub slider's basis), and `scrub` drives the bar chart's inspect overlay.
/// The surface sits ON TOP so a press anywhere on the panel captures the scrub;
/// transparent so the histogram shows through, and geometric hit-test is
/// alpha-independent so it still captures.
fn histogram_panel(
    view: &FrameTimingsView,
    theme: &pinion_core::Theme,
    size: (u32, u32),
    scrub: f32,
) -> Scene {
    let (hw, hh) = size;
    let histogram = histogram_chart(view, theme, scrub).build_fill(size, &chart_style(theme));
    // The scrub's a11y name lives on its `AccessNode` (the manual `WidgetA11y`
    // impl), not the scene node — a `BoxNode` has no `aria_label`.
    let scrub_surface = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
            .with_tag(HIST_SCRUB_TAG)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, 0)
                    .with_size(Size::px(hw.max(1), hh.max(1))),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![histogram, scrub_surface]).with_layout(
            LayoutStyle::new().with_size(
                Size::auto()
                    .with_width(SizeValue::Percent(100))
                    .with_height(SizeValue::Px(HIST_H)),
            ),
        ),
    )
}

/// view-fn (§6.3): pure sync `(ToggleState, bool, f32) -> Scene` (R1375 added
/// the histogram scrub fraction).
///
/// Pure *of published state*, which is what the `dry_run` guarantee needs:
/// the wall-clock non-determinism enters at the shell's publish, exactly as
/// an animation's `dt` enters at `tick_animations_for_window` rather than
/// here.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, scrub: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let chip_fill = theme.resolve(ColorRole::SurfaceContainerHighest);

    // The two seams: what to draw, and how big to draw it. One measured-rect
    // seam per chart, keyed on its own tag.
    let timings = use_frame_timings();
    let (cw, ch) = use_pane_viewport_size(CHART_TAG);
    let (hw, hh) = use_pane_viewport_size(HIST_TAG);

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

    // R1374 — the distribution histogram below the timeline (its own seam);
    // R1375 — `scrub` drives its per-bin inspect overlay.
    let hist_slot = histogram_panel(&timings, &theme, (hw, hh), scrub);

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
        ContainerNode::new(vec![title, slot, hist_slot, status, repaint])
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
/// [`WidgetCore`] + [`WidgetView`]. R1375's `a11y_manual` opts OUT of the
/// derived [`WidgetA11y`] so the histogram scrub node can join the repaint
/// Switch (the hand-written impl below).
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "repaint",
    state = (ToggleState, bool, f32),
    event = ToggleEvent,
    title = "pinion hello-frame-profiler (R1361 §5.16 live frame-time chart)",
    renderer = HelloFrameProfilerRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ToggleExternal::new,
    extra_externals = hist_scrub_extras,
    a11y_manual,
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
    ///
    /// R1374 — the min HEIGHT (`400`, up from `240`) reserves room for BOTH
    /// panels: the chrome (~112px) + the fixed `HIST_H` histogram would, at a
    /// 240px min, squeeze the flex-grow timeline to zero (`build_fill((w, 0))`
    /// paints nothing) and the primary panel would vanish. `400` keeps the
    /// timeline above its empty sentinel at the smallest window.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::OpenResizable {
            size: (WIN_W, WIN_H),
            min: Some((360, 400)),
        }
    }

    /// Tuple-state introspect: the repaint Toggle (SCXML state name + the
    /// repaint bit) read BY TAG plus the histogram scrub fraction. R1375 — the
    /// `extra_externals` slot wraps the primary Toggle in a
    /// `Container([primary, ...extras])`, so the pre-R1375 `Scene::External`
    /// root read no longer holds; both externals are found by tag now (the
    /// Toggle read reuses `toggle_group::read_toggle`, the crate's SSOT for that
    /// introspect walk, rather than re-deriving it here).
    fn read_state(scene: &Scene) -> (ToggleState, bool, f32) {
        let (state, on) = pinion_core::widgets::toggle_group::read_toggle(scene, REPAINT_TAG);
        (state, on, read_scrub(scene))
    }

    /// R641 §5.16 inherent view shim — unpacks the tuple and forwards.
    fn view(state: (ToggleState, bool, f32), frame: Frame) -> Scene {
        view(state.0, state.1, state.2, &frame)
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

/// R1375 §5.16 §5.40 — manual a11y (the `a11y_manual` opt-out): the repaint
/// Switch (reproducing verbatim the pre-R1375 derived form) plus the histogram
/// scrub Slider `describedby` the histogram's inspect tooltip region — so when a
/// bin is focused the tree is three nodes (Switch + scrub + region), and two
/// (Switch + scrub, no `describedby`) when there is nothing to read. That region
/// carries the per-bin readout a sighted user sees in the tooltip, so it reaches
/// a screen reader too (the R1355 parity, now for the bar chart). It is computed
/// against the
/// LIVE measured panel size — `access_node` runs inside `root_owner.run`, so
/// `use_pane_viewport_size(HIST_TAG)` resolves here to the SAME size the view
/// built the painted overlay at, and the two cannot disagree even though the
/// histogram is a `build_fill` (layout-derived-geometry) chart.
impl WidgetA11y for ProfilerView {
    fn access_node(state: &(ToggleState, bool, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, on, scrub) = (state.0, state.1, state.2);
        // Reproduces `emit_a11y_impl`'s Switch form exactly (disabled/hovered/
        // pressed by variant, checked = the repaint bit).
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, ToggleState::Disabled),
            hovered: matches!(interaction, ToggleState::Hover),
            pressed: matches!(interaction, ToggleState::Pressed),
            checked: Some(on),
            ..AccessState::default()
        };
        let switch = AccessNode::new(REPAINT_TAG, AriaRole::Switch)
            .with_value(AccessValue::Bool(on))
            .with_state(access_state);

        // The scrub readout, over the SAME measured size the view paints at.
        // `(0, 0)` = unmeasured: mirror `build_fill`'s empty sentinel (no
        // overlay painted ⇒ no readout) so a11y and paint stay in lockstep.
        let timings = use_frame_timings();
        let theme = use_theme(THEME_TAG).theme_animated();
        let (hw, hh) = use_pane_viewport_size(HIST_TAG);
        let readout = if hw != 0 && hh != 0 {
            histogram_chart(&timings, &theme, scrub)
                .inspect_readout(Rect::new(0, 0, hw, hh), &chart_style(&theme))
        } else {
            None
        };
        let has_readout = readout.is_some();

        let scrub_node = AccessNode::new(HIST_SCRUB_TAG, AriaRole::Slider)
            .with_name("Frame-time histogram scrub".to_string())
            .with_value(AccessValue::Float {
                value: scrub,
                min: 0.0,
                max: 1.0,
            });
        let mut nodes = vec![switch];
        nodes.extend(describedby_region(
            scrub_node,
            format!("{HIST_TAG}.inspect.tooltip"),
            AriaRole::Tooltip,
            readout,
            has_readout,
        ));
        nodes
    }
}

fn main() {
    pinion_shell::run::<ProfilerView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_runtime::{FRAME_TIMINGS, FrameTimingStats, compute_layout};

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
        FRAME_TIMINGS.resolve(owner).publish(view);
    }

    /// One paint cycle driving the REAL measured-rect seam, exactly as
    /// `hello-chart-fill` does: view → layout → publish → (dirty ⇒ re-view
    /// + re-layout). Nothing seeds a rect by hand.
    fn paint_cycle(core: &pinion_runtime::CoreShell<ProfilerView>, w: u32, h: u32) -> Scene {
        paint_cycle_scrub(core, w, h, 0.5)
    }

    fn paint_cycle_scrub(
        core: &pinion_runtime::CoreShell<ProfilerView>,
        w: u32,
        h: u32,
        scrub: f32,
    ) -> Scene {
        let mut cache = pinion_text::LayoutCache::new();
        let run_view = || {
            core.root_owner()
                .run(|| view(ToggleState::Idle, false, scrub, &Frame::new()))
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
        let off = owner.run(|| view(ToggleState::Idle, false, 0.5, &Frame::new()));
        let on = owner.run(|| view(ToggleState::Idle, true, 0.5, &Frame::new()));
        assert_ne!(off.paint_hash(), on.paint_hash());
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ProfilerView>(
            (ToggleState::Idle, false, 0.5),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<ProfilerView>(
            (ToggleState::Idle, false, 0.5),
            &Frame::new(),
        );
    }

    #[test]
    fn repaint_chip_reports_switch_role() {
        // R1375 — access_node now reads reactive hooks (the histogram readout
        // against the live measured size), so it runs inside an Owner scope
        // exactly as the shell wraps it (`collect_access_emit_inputs`).
        let owner = Owner::new();
        let nodes = owner.run(|| {
            <ProfilerView as WidgetA11y>::access_node(&(ToggleState::Idle, true, 0.5), None)
        });
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, REPAINT_TAG);
    }

    /// R1374 — the histogram bins EVERY frame exactly once, and paints a bucket
    /// whose lower edge is at/past the budget in the over-budget (jank) colour.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        reason = "HIST_BINS / a bin index are small display counts, exact in f64"
    )]
    fn r1374_histogram_bins_the_window_and_colours_over_budget() {
        let mut stats = FrameTimingStats::new();
        // Three ~8ms frames (under the 16.7ms budget), one 24ms jank frame.
        for &t in &[8_000u64, 8_000, 8_000, 24_000] {
            stats.record(FrameTiming::new(t / 2, t / 4, 0, t / 8, t));
        }
        let view = FrameTimingsView {
            samples: stats.samples().copied().collect(),
            snapshot: stats.snapshot(Some(16_666)),
        };
        let under = Color::rgb(0x40, 0x80, 0xF0);
        let over = Color::rgb(0xE0, 0x40, 0x40);
        let bars = histogram_bars(&view, under, over);
        assert_eq!(bars.len(), HIST_BINS, "one bar per bin");
        let binned: f64 = bars.iter().map(|b| b.value).sum();
        assert!(
            (binned - 4.0).abs() < 1e-9,
            "every frame is binned exactly once"
        );
        // max = 24ms ⇒ bin width 2ms; a bin's colour is the budget test on its
        // lower edge (k·2ms vs 16.666ms).
        let width = 24.0 / HIST_BINS as f64;
        for (k, b) in bars.iter().enumerate() {
            let expected = if (k as f64) * width >= 16.666 {
                over
            } else {
                under
            };
            assert_eq!(b.color, Some(expected), "bin {k} over/under-budget colour");
        }
        // The 24ms jank frame lands in the top bin (24/2 = 12, clamped to 11).
        assert!(
            bars[HIST_BINS - 1].value >= 1.0,
            "the jank frame is in the top bin"
        );
        // The three 8ms frames share bin 4 (8/2 = 4).
        assert!(
            (bars[4].value - 3.0).abs() < 1e-9,
            "the calm frames cluster in bin 4"
        );
    }

    /// R1374 — an empty window bins to no bars (the chart still draws its axes).
    #[test]
    fn r1374_no_samples_yet_is_an_empty_distribution() {
        let bars = histogram_bars(
            &FrameTimingsView::default(),
            Color::rgb(0, 0, 0),
            Color::rgb(0, 0, 0),
        );
        assert!(bars.is_empty(), "no frames ⇒ no bars");
    }

    /// R1374 — after real frames flow through the seam, the histogram panel
    /// paints its own tagged bars + axes beside the timeline (§2 #7).
    #[test]
    fn r1374_histogram_panel_paints_after_frames() {
        let core: pinion_runtime::CoreShell<ProfilerView> = pinion_runtime::CoreShell::new();
        seed(
            core.root_owner(),
            &[2_000, 3_000, 20_000, 2_500],
            Some(16_666),
        );
        let scene = paint_cycle(&core, 760, 520);
        assert!(
            find(&scene, "histogram.bar.0").is_some(),
            "the histogram painted bars"
        );
        assert!(
            find(&scene, "histogram.axis.x").is_some(),
            "with its own axes"
        );
        // The two charts are distinct panels, not one tag reused.
        assert!(
            find(&scene, "chart.series.0").is_some(),
            "the timeline still paints too"
        );
    }

    /// R1375 — a press-drag scrub over the histogram emits a per-bin inspect
    /// overlay, AND the readout it paints reaches a11y describedby the tooltip
    /// region. The readout is computed against the SAME live measured size the
    /// paint used (`access_node` runs in the owner scope), so paint and a11y
    /// agree even though the histogram is a `build_fill` chart.
    #[test]
    fn r1375_histogram_scrub_emits_overlay_and_a11y_readout() {
        let core: pinion_runtime::CoreShell<ProfilerView> = pinion_runtime::CoreShell::new();
        seed(
            core.root_owner(),
            &[2_000, 3_000, 20_000, 2_500],
            Some(16_666),
        );
        // A scrub near the right edge focuses a higher bin; the paint_cycle also
        // publishes the HIST_TAG measured size the a11y readout then reads.
        let scene = paint_cycle_scrub(&core, 760, 520, 0.9);
        assert!(
            find(&scene, "histogram.inspect.tooltip").is_some(),
            "the scrub paints a tooltip"
        );
        assert!(
            find(&scene, "histogram.inspect.header").is_some(),
            "with a bin-label header"
        );
        assert!(
            find(&scene, "histogram.inspect.value").is_some(),
            "and a value row"
        );
        // The overlay is the histogram's, NOT the timeline's — the timeline
        // (line chart) has no inspect here, so it emits no inspect nodes.
        assert!(
            find(&scene, "chart.inspect.tooltip").is_none(),
            "only the histogram is scrubbed, not the timeline"
        );

        // a11y: the scrub Slider node, describedby the histogram inspect region
        // that carries the readout.
        let nodes = core.root_owner().run(|| {
            <ProfilerView as WidgetA11y>::access_node(&(ToggleState::Idle, false, 0.9), None)
        });
        let scrub = nodes
            .iter()
            .find(|n| n.tag == HIST_SCRUB_TAG)
            .expect("scrub node in the a11y tree");
        assert_eq!(scrub.role, AriaRole::Slider);
        let region_tag = scrub
            .described_by
            .as_deref()
            .expect("scrub is describedby a region");
        assert_eq!(region_tag, "histogram.inspect.tooltip");
        let region = nodes
            .iter()
            .find(|n| n.tag == region_tag)
            .expect("the described region is IN the tree (no dangling reference)");
        let name = region.name.as_deref().expect("region carries the readout");
        assert!(
            name.contains('='),
            "readout names the focused bin and its value: {name:?}"
        );
    }
}
