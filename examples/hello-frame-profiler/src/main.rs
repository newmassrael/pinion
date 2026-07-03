//! `hello-frame-profiler` — R907 §5.16 §5.7 consumer of the
//! frame-timing profiler substrate.
//!
//! ## What this demonstrates
//!
//! The §1 northern-star's pro-tool-performance axis is "measure first":
//! a frame-budget tuning round can only be evidence-based if the frame
//! cost is *readable*. R907 instruments `AppShell::render_window` to
//! bracket every painted frame into three phases — **build** (`view`
//! fn + layout), **encode** (structured scene → `vello` fragments),
//! **render** (GPU command-buffer submit) — plus the **total**
//! productive frame span. Each sample feeds a per-window rolling window
//! (`pinion_runtime::FrameTimingStats`), and `scene/frame_timings`
//! projects it: last frame's breakdown + the window's min/mean/max +
//! per-phase means + mean fps.
//!
//! This binding is a *driver* for that substrate. It paints a small
//! text workload (a heading + labelled rows) so each frame has
//! non-trivial build/encode cost, plus a §5.38 [`ToggleExternal`]
//! ("Repaint") whose every flip re-runs the view and produces one more
//! measured frame. An AI client drives repaints by clicking the toggle
//! and watches the profiler's `frame_count` climb and its aggregates
//! settle — all through RPC, never pixels.
//!
//! ## Why a Toggle
//!
//! A profiler readout is intrinsically stateless, but the `AppShell`
//! drives a `WidgetView` with a statechart `External`. Rather than
//! invent a one-off widget ([[abstraction-needs-second-consumer]]), the
//! binding reuses the Toggle purely as the *RPC-drivable repaint
//! trigger* — the hello-elevation pattern (Toggle-as-bit). The frames
//! it triggers are the substrate under test.
//!
//! ## Verification (substrate-first)
//!
//! `tools/demos/r907_frame_profiler.py` reads `scene/frame_timings`
//! over RPC and asserts the **deterministic invariants on
//! non-deterministic data**: `total >= build + encode + render` (the
//! phases are disjoint sub-intervals), `min <= mean <= max`,
//! `mean_fps ≈ 1e6 / mean_total_us`, `window_len = min(frame_count,
//! FRAME_TIMING_WINDOW)`, and `frame_count` strictly increases per
//! driven repaint. Wall-clock magnitudes are never asserted — only
//! their structure ([[ai-first-rpc-introspection-obligation]],
//! [[introspection-from-paint-not-screen]]).

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloFrameProfilerRenderer`
// + error + async `new` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can drive it.
vello_renderer_impl!(HelloFrameProfilerRenderer, HelloFrameProfilerRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 460;

const THEME_TAG: &str = "app";
const REPAINT_TAG: &str = "repaint";

const TITLE_FONT_PX: u32 = 18;
const ROW_FONT_PX: u32 = 14;
const STATUS_FONT_PX: u32 = 12;

/// Workload row count. A modest column of shaped-text rows so the build
/// and encode phases have measurable, non-zero cost each frame — enough
/// for the profiler to report a realistic breakdown, small enough to
/// stay legible in a 360x460 window.
const WORKLOAD_ROWS: usize = 8;

const ROW_W: u32 = 240;
const ROW_H: u32 = 24;

/// Material 3 state-layer overlay weights for the repaint chip chrome.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// One workload row: a tagged rounded chip holding a label. Tagged so
/// the demo can confirm the painted tree (and thus the measured frame)
/// actually carried the workload.
fn workload_row(index: usize, fill: Color, fg: Color) -> Scene {
    let label = Scene::Text(TextNode::styled(
        format!("workload row {index}"),
        Rect::default(),
        TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(row_tag(index))
            .with_style(BoxStyle::filled(fill).with_corner_radius(6))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(ROW_W, ROW_H)),
            ),
    )
}

/// Stable per-row tags. A fixed `&'static str` table (not a per-frame
/// `format!` + leak) — the view fn runs every paint, so allocating
/// tags there would leak unboundedly. Length pinned to
/// [`WORKLOAD_ROWS`] by a compile-time assert.
const ROW_TAGS: [&str; WORKLOAD_ROWS] = [
    "workload_row_0",
    "workload_row_1",
    "workload_row_2",
    "workload_row_3",
    "workload_row_4",
    "workload_row_5",
    "workload_row_6",
    "workload_row_7",
];

fn row_tag(index: usize) -> &'static str {
    ROW_TAGS[index]
}

/// view-fn (§6.3): pure sync `(ToggleState, bool) -> Scene`. The `on`
/// bit only flips the repaint chip's label/fill — its purpose is to
/// make every click a distinct paint the profiler records, not to mean
/// anything on its own.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let row_fill = theme.resolve(ColorRole::SurfaceContainerHighest);

    let title = Scene::Text(TextNode::styled(
        "Frame profiler",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(on_surface),
    ));

    // The text workload — the per-frame build/encode cost the profiler
    // measures.
    let mut children: Vec<Scene> = Vec::with_capacity(WORKLOAD_ROWS + 3);
    children.push(title);
    for i in 0..WORKLOAD_ROWS {
        children.push(workload_row(i, row_fill, on_surface));
    }

    // Repaint trigger — the Toggle hit handle. Each click flips `on`,
    // re-runs the view, and produces one more measured frame.
    let repaint_fill: Color = match state {
        ToggleState::Idle => row_fill,
        ToggleState::Hover => row_fill.lerp(on_surface, HOVER_OVERLAY_T),
        ToggleState::Pressed => row_fill.lerp(on_surface, PRESSED_OVERLAY_T),
        ToggleState::Disabled => row_fill.lerp(surface, DISABLED_OVERLAY_T),
    };
    let repaint_label = Scene::Text(TextNode::styled(
        if on { "Repaint (B)" } else { "Repaint (A)" },
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
                    .with_size(Size::px(ROW_W, ROW_H)),
            ),
    );
    children.push(repaint);

    let status = Scene::Text(TextNode::styled(
        format!("{} | frame {}", state.as_name(), if on { "B" } else { "A" }),
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    children.push(status);

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8),
            ),
    )
}

/// `WidgetView` binding. The §5.38 Toggle is reused as the RPC-drivable
/// repaint trigger (see the module doc); `#[widget]` derives the
/// mechanical [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`] trio.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "repaint",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-frame-profiler (R907 §5.16 frame timing)",
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
)]
struct ProfilerView;

impl ProfilerView {
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

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            _ => None,
        }
    }

    fn rendered(state: ToggleState, on: bool) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, on, &Frame::new()))
    }

    #[test]
    fn view_carries_the_text_workload() {
        let scene = rendered(ToggleState::Idle, false);
        for i in 0..WORKLOAD_ROWS {
            assert!(
                find_container(&scene, row_tag(i)).is_some(),
                "workload row {i} present in the painted tree",
            );
        }
    }

    #[test]
    fn repaint_toggle_relabels_per_flip() {
        // The off/on bit must change the painted tree so each click is a
        // distinct paint the profiler records (re-keys the paint hash).
        let off = rendered(ToggleState::Idle, false);
        let on = rendered(ToggleState::Idle, true);
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
    fn repaint_chip_reports_switch_role() {
        let nodes = <ProfilerView as WidgetA11y>::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, REPAINT_TAG);
    }
}
