//! `hello-dock-chart` — R1396 consumer that PROVES the docked-chart seam the
//! `pinion-chart` crate documented as UNPROVEN.
//!
//! ## The claim under test
//!
//! `pinion-chart`'s layout-native `build_fill` (R1360) needs its slot's measured
//! size, which it reads through `use_pane_viewport_size(TAG)`. `hello-chart-fill`
//! proved that works when the chart fills a **window**. The crate's own
//! `debt-dataviz-dashboard-substrate` note then flagged the case the target
//! consumer actually needs — a chart inside a resizable **dock pane** — as
//! untested: "the dock↔pane-registry↔chart-tag interaction is untested. Do not
//! assert it." This binding is that missing consumer.
//!
//! A [`view_dock_surface`] dock hosts a `LineChart::build_fill` in its left pane
//! (tag [`CHART_TAG`], nested under the pane's `{panel}#content` wrapper). The
//! shell's post-layout `publish_pane_viewports` walks the laid-out scene for that
//! tag — through the dock's splitter + panel containers — and writes its measured
//! rect to the tag's signal, exactly as for a window-level chart. So:
//!
//! * **Resize the window** (`scene/resize`): both panes re-lay-out, the chart
//!   pane's measured rect changes, and the chart re-scales — the same live paint
//!   path `hello-chart-fill` uses, now with the tag two containers deep in a dock.
//! * **Drag the splitter**: the pane resizes *independently of the window*, and
//!   the chart re-scales to the pane. This is the dock-native resize the window
//!   path cannot exercise.
//!
//! ## Also the forcing consumer for the R1396 narrow-pane clamp
//!
//! A window is wide enough to absorb a chart's legend + last-tick overhang in its
//! own padding; a **dock pane is not** — a chart narrower than its legend paints
//! over its neighbour. R1396 made the chart contain its own chrome (the legend
//! shrinks and then collapses to a `+N` marker; the last x-tick label clamps
//! inside the chart), and this binding is where that is exercised: drag the chart
//! pane narrow and its legend collapses instead of bleeding into the readout pane.
//!
//! ## Verification
//!
//! `tools/demos/r1396_dock_chart.py` drives real `scene/resize` + a real splitter
//! drag over RPC and reads the chart back with `scene/snapshot from=paint`, so the
//! measured-rect publish (a live-paint-only seam) is exercised end to end. The
//! tests below pin the static scene shape (the dock hosts a tagged chart; the
//! readout pane names the seam) through `compute_layout`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, SizeValue, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{External, Frame, Owner, Scene, Signal, WidgetCore, use_pane_viewport_size};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::dock::{DockNode, DockSplitState, DockTopology, view_dock_surface};
use pinion_widget_paint::splitter::{SplitterExternal, SplitterOrientation};
use std::rc::Rc;

// pinion-forge codegen output — defines `HelloDockChartRenderer` +
// `HelloDockChartRendererError` (the Vello wrapper).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloDockChartRenderer, HelloDockChartRendererError);

/// Opening size only — the chart's geometry is derived from the MEASURED pane,
/// never from these. Resize the window or drag the splitter to any size.
const WIN_W: u32 = 760;
const WIN_H: u32 = 480;

/// Shared `ThemeProvider` cache key (the `"app"` gallery convention).
const THEME_TAG: &str = "app";

/// The horizontal split between the chart pane (left) and the readout pane
/// (right). It is BOTH the `DockNode::Split` id (⇒ the walker's splitter tag)
/// and the `SplitterExternal` registration tag + ratio-signal cache key, so the
/// drag handle, its external, and its live ratio are one identity.
const CHART_SPLIT: &str = "chart_split";

/// The chart pane's leaf id (⇒ its dock-panel root tag).
const CHART_PANEL: &str = "chart-pane";

/// The readout pane's leaf id.
const READOUT_PANEL: &str = "readout-pane";

/// The chart root's `tag_prefix`. It is BOTH the §2 #7 introspection prefix
/// (`chart.series.0` …) AND the measured-rect seam key: the shell publishes the
/// rect of the node carrying this tag — reached by descending the dock's
/// splitter + panel containers — and the pane content reads it back through
/// `use_pane_viewport_size(CHART_TAG)`. One tag, so the thing measured is by
/// construction the thing painted, even two containers deep in a dock.
const CHART_TAG: &str = "chart";

/// The readout pane body tag — a focus stop + the place the seam's live state is
/// mirrored as scene data (§2 #7): the measured chart size and the split ratio,
/// so a reader (or the demo) can confirm the pane resized without pixels.
const READOUT_BODY_TAG: &str = "readout_body";

/// The initial split fraction (the chart pane's share of the width).
const BOOT_RATIO: f32 = 0.5;

/// Cache-or-create the shared split-ratio signal (the `hello-chart-fill`
/// `Owner::cache` idiom). The same key in `create_extra_externals` (which
/// `attach_ratio`s it onto the `SplitterExternal`) and in the view's
/// `split_state` callback returns the SAME `Rc<Signal<f32>>`, so a drag's
/// `Signal::set` and the view's `get()` are one value.
fn use_split_ratio() -> Rc<Signal<f32>> {
    Owner::current()
        .expect("hello-dock-chart: runs inside an owner scope")
        .cache(CHART_SPLIT, || Signal::new(BOOT_RATIO))
}

/// Four deterministic series — enough that a narrow pane's legend must shrink and
/// then collapse to a `+N` marker (the R1396 clamp the dock forces). Data is
/// programmatic; the pane-resize behaviour is what this binding exists to show.
fn sample_series() -> Vec<Series> {
    (0u8..4)
        .map(|s| {
            let phase = f64::from(s) * 0.8;
            let points: Vec<DataPoint> = (0..24)
                .map(|i| {
                    let x = f64::from(i);
                    DataPoint::new(x, 400.0 + 260.0 * (x / 3.2 + phase).sin() + 24.0 * x)
                })
                .collect();
            Series::new(format!("worker-{}", (b'a' + s) as char), points)
        })
        .collect()
}

/// The chart style, resolved from the live theme so the chart tracks light/dark
/// (series colours come from the crate's Okabe-Ito palette, theme-independent).
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        ..ChartStyle::default()
    }
}

/// The chart pane's content: a `LineChart` that FILLS the pane. `build_fill`
/// reads the pane's measured size from `use_pane_viewport_size(CHART_TAG)` —
/// `(0, 0)` until the shell's post-layout publish lands, then the same-frame
/// re-pass rebuilds at the real size. This is byte-for-byte the `hello-chart-fill`
/// pattern; the only difference is that the slot is a dock pane, not the window.
fn chart_pane_content(theme: &Theme) -> Scene {
    let (cw, ch) = use_pane_viewport_size(CHART_TAG);
    let chart = LineChart::new(sample_series())
        .filled(false)
        .build_fill((cw, ch), &chart_style(theme));
    // The chart fills the pane's content wrapper (which is already flex-main:
    // basis 0 / grow 1 / min-height 0), so the chart root's measured rect IS the
    // pane's content rect — the thing the seam publishes under CHART_TAG.
    Scene::Container(
        ContainerNode::new(vec![chart]).with_layout(
            LayoutStyle::new().with_flex_grow(1.0).with_size(
                Size::auto()
                    .with_width(SizeValue::Percent(100))
                    .with_height(SizeValue::Percent(100)),
            ),
        ),
    )
}

/// What the readout says, as one derivation.
///
/// R1581 — the paint and the accessible node read this, so the sentence a
/// screen reader is given and the sentence on screen cannot be two sentences.
fn readout_body_text(cw: u32, ch: u32, ratio: f32) -> String {
    if cw == 0 || ch == 0 {
        "chart pane unmeasured — it paints on the next pass".to_string()
    } else {
        format!("chart pane measured {cw} x {ch} px (split ratio {ratio:.2})")
    }
}

/// The readout pane: names the seam's live state as scene data — the measured
/// chart size and the split ratio. Not chrome for its own sake: it lets the demo
/// (and an AI) confirm the pane resized by reading text, and it is the neighbour
/// a pre-R1396 narrow chart would have painted over.
fn readout_pane_content(theme: &Theme) -> Scene {
    let (cw, ch) = use_pane_viewport_size(CHART_TAG);
    let ratio = use_split_ratio().get();
    let heading = Scene::Text(
        TextNode::styled(
            "chart pane readout".to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(
            LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );
    let body_text = readout_body_text(cw, ch, ratio);
    let body = Scene::Text(
        TextNode::styled(
            body_text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(READOUT_BODY_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::auto().with_width(SizeValue::Percent(100)))
                .with_focusable(true),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![heading, body]).with_layout(
            LayoutStyle::new()
                .flex(pinion_core::style::FlexDirection::Column)
                .with_gap(6)
                .with_size(
                    Size::auto()
                        .with_width(SizeValue::Percent(100))
                        .with_height(SizeValue::Percent(100)),
                ),
        ),
    )
}

/// The static two-leaf topology: a horizontal split with the chart pane left and
/// the readout pane right. Built each paint (cheap, pure data); the live ratio
/// lives in the signal the walker threads through `DockSplitState`.
fn topology() -> DockTopology {
    DockTopology::new(DockNode::split_horizontal(
        CHART_SPLIT,
        BOOT_RATIO,
        DockNode::leaf(CHART_PANEL),
        DockNode::leaf(READOUT_PANEL),
    ))
}

/// view-fn (§6.3): pure sync `() -> Scene`. The dock surface is the whole window;
/// the walker lowers the topology into splitter + panel containers and calls back
/// for each pane's content.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let workspace = view_dock_surface(
        &topology(),
        |panel_id| match panel_id {
            CHART_PANEL => chart_pane_content(&theme),
            READOUT_PANEL => readout_pane_content(&theme),
            other => Scene::Text(TextNode::styled(
                format!("(unknown panel: {other})"),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(12)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )),
        },
        // The split's live state — the shared ratio signal (the drag writes it,
        // this reads it). `dragging` is the cosmetic handle-tint mirror; a static
        // surface leaves it false (the handle still drags).
        |_split_id, _initial| DockSplitState {
            ratio_signal: use_split_ratio(),
            dragging: false,
        },
        // No in-flight reorganize drag: no panel shows a drop-zone overlay.
        |_panel_id| None,
        &theme,
    );
    // The dock surface fills the window on the theme Surface (so the panel gaps
    // and any padding sit on the theme colour, not the black render clear).
    Scene::Container(
        ContainerNode::new(vec![workspace])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(pinion_core::style::FlexDirection::Column)
                    .with_size(
                        Size::auto()
                            .with_width(SizeValue::Percent(100))
                            .with_height(SizeValue::Percent(100)),
                    ),
            ),
    )
}

/// The binding. Hand-written (not `#[widget]`-derived), PR-51 display-only like
/// `hello-chart-fill`: the chart's only "input" is its measured pane, and the
/// splitter is an extra external, so there is no primary surface.
struct DockChartView;

impl WidgetCore for DockChartView {
    type State = ();
    type Event = ();

    /// (PR-51) No primary surface: the interactive element is the splitter (an
    /// extra external), and the chart is display-only. The resize path is the
    /// shell's.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-dock-chart has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-dock-chart has no primary surface — see primary_surface()")
    }

    /// One `SplitterExternal` for the chart↔readout split, keyed on the split id
    /// (which IS the walker's splitter tag). `attach_ratio` shares the ratio
    /// signal the view reads, so a drag on the handle re-scales both panes — and
    /// the chart pane's chart with them.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let external =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(use_split_ratio());
        vec![ExtraExternal::new(CHART_SPLIT, Box::new(external))]
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-dock-chart (R1396: a chart lives in a resizable dock pane)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (the chart's only input is its measured dock pane)".to_string()
    }
}

impl WidgetA11y for DockChartView {
    /// R1581 §5.40 — the readout body is a keyboard FOCUS STOP
    /// (`with_focusable`), and a focus stop with no node in the AT tree is one
    /// `AccessTreeBuilder` folds onto the window root: a screen-reader user who
    /// tabs to it is told they are on the window, which is the R1329/PR-53
    /// failure shape. The chart's own description stays `pinion-chart`'s
    /// (R1359 `describedby_region`) — this adds only the node the focus ring
    /// needs, with the same sentence the pane paints.
    ///
    /// `access_node` runs in the shell's owner scope, so the pane-viewport and
    /// split-ratio hooks resolve here exactly as they do in the view.
    fn access_node(_state: &Self::State, _focused: Option<&str>) -> Vec<AccessNode> {
        let (cw, ch) = use_pane_viewport_size(CHART_TAG);
        let ratio = use_split_ratio().get();
        vec![
            AccessNode::new(READOUT_BODY_TAG, AriaRole::Status)
                .with_name("chart pane readout")
                .with_value(AccessValue::Text(readout_body_text(cw, ch, ratio))),
        ]
    }
}

impl WidgetView for DockChartView {
    type Renderer = HelloDockChartRenderer;

    /// Resizable on purpose: a window resize AND a splitter drag both re-scale
    /// the docked chart — the two resize paths the round proves.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::OpenResizable {
            size: (WIN_W, WIN_H),
            min: Some((280, 200)),
        }
    }
}

fn main() {
    pinion_shell::run::<DockChartView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_runtime::{CoreShell, compute_layout};

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

    /// One paint cycle through the REAL seam, exactly as `hello-chart-fill`'s
    /// test harness does: run the view under the shell's `root_owner`, lay it
    /// out, publish the post-layout pane rects, and — when that publish reports
    /// dirty — re-run view + layout once (the same-frame re-pass). Returns the
    /// painted scene plus whether the re-pass fired. Nothing seeds a signal by
    /// hand: the measured size the second view reads is what layout produced —
    /// here for a tag two containers deep in a dock.
    fn paint_cycle(core: &CoreShell<DockChartView>, w: u32, h: u32) -> (Scene, bool) {
        let mut cache = pinion_text::LayoutCache::new();
        let run_view = || core.root_owner().run(|| view((), &Frame::new()));
        let mut scene = run_view();
        compute_layout(&mut scene, &mut cache, w, h);
        let dirty = core.publish_pane_viewports(&scene);
        if dirty {
            scene = run_view();
            compute_layout(&mut scene, &mut cache, w, h);
        }
        (scene, dirty)
    }

    #[test]
    fn the_dock_hosts_a_tagged_chart_in_its_left_pane() {
        let core: CoreShell<DockChartView> = CoreShell::new();
        let (scene, _) = paint_cycle(&core, WIN_W, WIN_H);

        // The chart root is present, nested under the chart pane's content
        // wrapper — proving the tag is reachable through the dock containers.
        assert!(
            find(&scene, CHART_TAG).is_some(),
            "the chart root is in the scene"
        );
        assert!(
            find(&scene, CHART_PANEL).is_some(),
            "the chart pane root carries its leaf id"
        );
        assert!(
            find(&scene, READOUT_PANEL).is_some(),
            "the readout pane root carries its leaf id"
        );
    }

    #[test]
    fn the_chart_pane_measures_and_the_readout_names_it() {
        let core: CoreShell<DockChartView> = CoreShell::new();
        // The publish writes CHART_TAG's measured rect (reached through the dock
        // splitter + panel containers) — the seam the round proves.
        let (scene, dirty) = paint_cycle(&core, WIN_W, WIN_H);
        assert!(
            dirty,
            "publishing the dock-nested chart tag reports a change"
        );

        let chart = find(&scene, CHART_TAG).expect("chart present");
        let rect = chart.rect();
        assert!(
            rect.w > 0 && rect.h > 0,
            "the chart measured a real rect: {rect:?}"
        );
        // The readout mirrors the measured size (not the "unmeasured" sentinel).
        let readout = find(&scene, READOUT_BODY_TAG).expect("readout body present");
        let text = match readout {
            Scene::Text(t) => t.content.clone(),
            _ => String::new(),
        };
        assert!(
            text.contains("measured"),
            "the readout names the measured pane, got {text:?}"
        );
    }

    #[test]
    fn the_chart_rescales_when_the_window_widens() {
        let core: CoreShell<DockChartView> = CoreShell::new();
        let (narrow, _) = paint_cycle(&core, 480, 360);
        let narrow_w = find(&narrow, CHART_TAG).expect("chart").rect().w;
        let (wide, _) = paint_cycle(&core, 1100, 360);
        let wide_w = find(&wide, CHART_TAG).expect("chart").rect().w;
        assert!(
            wide_w > narrow_w,
            "a wider window widens the docked chart: {narrow_w} -> {wide_w}"
        );
    }
}
