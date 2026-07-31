//! `hello-tabbed-chart` — R1409 consumer that PROVES a `pinion-chart` chart
//! lives in a dock **TAB well** (`DockNode::Tabs`), the case R1396's docked-chart
//! note left explicitly UNEXERCISED.
//!
//! ## Why a tab well is not "just a Leaf"
//!
//! R1396 (`hello-dock-chart`) proved a `LineChart::build_fill` re-scales inside a
//! dock **Leaf** pane: the shell's post-layout `publish_pane_viewports` walks the
//! laid-out scene for the chart tag — through the dock's splitter + panel
//! containers — and writes its measured rect to `use_pane_viewport_size(CHART_TAG)`.
//! The chart crate's own `debt-dataviz-dashboard-substrate` note then flagged the
//! remaining case a real dashboard needs — a chart as one **tab** of a well — as
//! UNEXERCISED, with the caveat "the walker wraps a tab-well leaf identically, so
//! it is a demo gap, not a suspected defect."
//!
//! That caveat is a *hypothesis*: the `Tabs` arm is **not** byte-identical to the
//! `Leaf` arm. A well renders ONLY its active panel, header-suppressed, under a
//! fixed tab strip. So a chart in a well is measured for the space **below the
//! strip**, and — the genuinely new behaviour — an *inactive* chart tab is absent
//! from the scene entirely, so nothing measures it until it is re-activated. This
//! binding is the forcing consumer that turns the hypothesis into a proof.
//!
//! ## What it proves
//!
//! A [`view_dock_surface`] hosts a horizontal split: the LEFT pane is a `Tabs`
//! well stacking a **chart tab** (a `LineChart::build_fill`, tag `CHART_TAG`) and
//! a **notes tab**; the RIGHT pane is a readout that mirrors the seam's live state
//! (the measured chart size, the active tab, the split ratio) as scene data (§2
//! #7), so the demo — and an AI — can confirm the behaviour by reading text.
//!
//! * **Resize the window / drag the splitter** while the chart tab is active: the
//!   well's cell changes, the chart pane's measured rect (below the strip)
//!   republishes, and the chart re-scales — the R1396 seam, now one wrapper (the
//!   tab well) deeper. A narrow well still collapses the legend to a `+N` marker
//!   (the R1396 clamp) instead of bleeding over the readout.
//! * **Switch tabs** ([`TabWellExternal`], the click wire): activating the notes
//!   tab removes the chart tag from the scene (active-only render); re-activating
//!   the chart tab makes it reappear and re-measure. If the well was resized while
//!   the chart tab was hidden, the reappeared chart fits the NOW size, not the
//!   stale one — caught by the same publish -> dirty -> re-pass the shell runs on
//!   any resize, but off a trigger a Leaf never has: a Leaf is in the scene every
//!   frame, so it is never *absent* to reappear. The witness is the reappeared
//!   chart's own INTERNAL geometry (its x-tick labels stay inside its narrow rect,
//!   the R1396 clamp) — a chart rebuilt from the stale wide size would overflow.
//! * **Announce itself** (§2 #7): the tab well emits a WAI-ARIA `tablist` /
//!   `tab` / `tabpanel` tree (`scene/access`), so an AI can DISCOVER the well and
//!   its tabs, and `aria-selected` tracks the switch — a chart in a tab an AI
//!   cannot enumerate would violate the "AI-introspection 1st-class" invariant.
//!
//! ## Verification
//!
//! `tools/demos/r1409_tabbed_chart.py` drives real `scene/resize`, a real splitter
//! `scene/drag`, and real tab-switch clicks (`scene/invoke` `send`) over RPC, and
//! reads the chart back with `scene/snapshot from=paint`, so the measured-rect
//! publish (a live-paint-only seam) is exercised end to end through a tab well. The
//! tests below pin the same behaviour through the real `CoreShell` +
//! `compute_layout` + `publish_pane_viewports` pipeline.

use pinion_a11y::{AccessFocus, AccessNode, WidgetA11y};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{External, Frame, Owner, Scene, Signal, WidgetCore, use_pane_viewport_size};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::dock::{
    DockNode, DockReorganizer, DockSplitState, DockTopology, TabWellExternal,
    dock_tablist_access_nodes, dock_tablist_focus_target, view_dock_surface,
};
use pinion_widget_paint::splitter::{SplitterExternal, SplitterOrientation};
use std::borrow::Cow;
use std::rc::Rc;

// pinion-forge codegen output — defines `HelloTabbedChartRenderer` +
// `HelloTabbedChartRendererError` (the Vello wrapper).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTabbedChartRenderer, HelloTabbedChartRendererError);

/// Opening size only — the chart's geometry is derived from the MEASURED pane,
/// never from these. Resize the window or drag the splitter to any size.
const WIN_W: u32 = 820;
const WIN_H: u32 = 500;

/// Shared `ThemeProvider` cache key (the `"app"` gallery convention).
const THEME_TAG: &str = "app";

/// The horizontal split between the tab well (left) and the readout pane (right).
/// It is BOTH the `DockNode::Split` id (⇒ the walker's splitter tag) and the
/// `SplitterExternal` registration tag + ratio-signal cache key.
const CHART_SPLIT: &str = "chart_split";

/// The tab well's stable id ([`DockNode::Tabs::id`]) — the painted `view_tabs`
/// strip tag, the [`TabWellExternal`] registration tag, and the R51.42 primary
/// half of every `{WELL}#{i}` tab tag the walker paints.
const WELL: &str = "left_well";

/// The chart tab's panel id (tab 0). Its content is a chart that fills the well
/// cell below the strip.
const CHART_PANEL: &str = "chart-tab";

/// The notes tab's panel id (tab 1). A static sibling — activating it removes the
/// chart from the scene, which is exactly the state the round exists to exercise.
const NOTES_PANEL: &str = "notes-tab";

/// The readout pane's leaf id (right of the split).
const READOUT_PANEL: &str = "readout-pane";

/// The chart root's `tag_prefix` — BOTH the §2 #7 introspection prefix
/// (`chart.series.0` …) AND the measured-rect seam key: the shell publishes the
/// rect of the node carrying this tag (reached by descending the split + well +
/// panel containers), and the chart tab reads it back through
/// `use_pane_viewport_size(CHART_TAG)`.
const CHART_TAG: &str = "chart";

/// The readout body tag — a focus stop + the place the seam's live state is
/// mirrored as scene data (§2 #7): the measured chart size, the active tab, and
/// the split ratio, so a reader (or the demo) can confirm the behaviour without
/// pixels.
const READOUT_BODY_TAG: &str = "readout_body";

/// The notes tab's body tag — present only while the notes tab is active, so a
/// reader can confirm the tab switch by which body is in the scene.
const NOTES_BODY_TAG: &str = "notes_body";

/// The initial split fraction (the tab well's share of the width).
const BOOT_RATIO: f32 = 0.58;

/// Cache-or-create the shared split-ratio signal (the `hello-dock-chart`
/// `Owner::cache` idiom). The same key in `create_extra_externals` (which
/// `attach_ratio`s it onto the `SplitterExternal`) and in the view's
/// `split_state` callback returns the SAME `Rc<Signal<f32>>`.
fn use_split_ratio() -> Rc<Signal<f32>> {
    Owner::current()
        .expect("hello-tabbed-chart: runs inside an owner scope")
        .cache(CHART_SPLIT, || Signal::new(BOOT_RATIO))
}

/// The static topology shape: a horizontal split with the tab well (chart tab +
/// notes tab, chart active) on the left and the readout leaf on the right.
fn build_topology() -> DockTopology {
    DockTopology::new(DockNode::split_horizontal(
        CHART_SPLIT,
        BOOT_RATIO,
        DockNode::tabs(
            WELL,
            [Cow::Borrowed(CHART_PANEL), Cow::Borrowed(NOTES_PANEL)],
            0,
        ),
        DockNode::leaf(READOUT_PANEL),
    ))
}

/// The live dock topology, owned as a reactive `Signal<Option<DockTopology>>`
/// (the R1084 universal Option surface the reorganize coordinator is total over)
/// so a tab switch mutates it and the view fn's `get()` subscription re-renders.
/// Cached so it is the SAME signal the reorganizer wraps.
fn use_topology_signal() -> Rc<Signal<Option<DockTopology>>> {
    Owner::current()
        .expect("hello-tabbed-chart: runs inside an owner scope")
        .cache("tabbed_topology", || Signal::new(Some(build_topology())))
}

/// The ONE reorganize coordinator — the sole writer of the topology (the active
/// tab lives ONLY there, per the dock SSOT). The [`TabWellExternal`] routes a tab
/// click through its `activate_tab`. Cached so the external and the view share one
/// instance. The topology dep is resolved BEFORE the cache factory (the
/// `Owner::cache` factory must not nest another `cache` resolution).
fn use_reorganizer() -> Rc<DockReorganizer> {
    let topology = use_topology_signal();
    Owner::current()
        .expect("hello-tabbed-chart: runs inside an owner scope")
        .cache("tabbed_reorganizer", move || DockReorganizer::new(topology))
}

/// Four deterministic series — enough that a narrow well's legend must shrink and
/// then collapse to a `+N` marker (the R1396 clamp the dock forces). Data is
/// programmatic; the pane-resize + tab-switch behaviour is what this binding shows.
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

/// The chart tab's content: a `LineChart` that FILLS the well cell (below the
/// strip). `build_fill` reads the measured size from `use_pane_viewport_size(CHART_TAG)`
/// — `(0, 0)` until the shell's post-layout publish lands, then the same-frame
/// re-pass rebuilds at the real size. Byte-for-byte the `hello-dock-chart` pattern;
/// the only difference is the slot is a tab of a well, not a bare Leaf.
fn chart_pane_content(theme: &Theme) -> Scene {
    let (cw, ch) = use_pane_viewport_size(CHART_TAG);
    let chart = LineChart::new(sample_series())
        .filled(false)
        .build_fill((cw, ch), &chart_style(theme));
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

/// The notes tab's content: a static explainer. Its only job is to be a sibling
/// the chart tab can hide behind — while it is active, `chart_pane_content` is
/// never called, so the chart tag is absent from the scene.
fn notes_pane_content(theme: &Theme) -> Scene {
    let body = Scene::Text(
        TextNode::styled(
            "notes tab — the chart tab is HIDDEN. A tab well renders only its \
             active panel, so the chart tag is absent from the scene right now. \
             Re-activate the chart tab (or press its tab) and it re-measures the \
             well and re-scales."
                .to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(NOTES_BODY_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::auto().with_width(SizeValue::Percent(100)))
                .with_focusable(true),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![body]).with_layout(
            LayoutStyle::new().flex(FlexDirection::Column).with_size(
                Size::auto()
                    .with_width(SizeValue::Percent(100))
                    .with_height(SizeValue::Percent(100)),
            ),
        ),
    )
}

/// The readout pane: names the seam's live state as scene data — the active tab,
/// the last-measured chart size (retained even while the chart tab is hidden), and
/// the split ratio. It lets the demo (and an AI) confirm the tab switch + resize
/// by reading text, and it is the neighbour a pre-R1396 narrow chart would paint
/// over.
fn readout_pane_content(theme: &Theme, active: usize) -> Scene {
    let (cw, ch) = use_pane_viewport_size(CHART_TAG);
    let ratio = use_split_ratio().get();
    let heading = Scene::Text(
        TextNode::styled(
            "tab well readout".to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(
            LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );
    let tab_name = if active == 0 { "chart" } else { "notes" };
    let size_part = if cw == 0 || ch == 0 {
        "chart pane unmeasured — it paints on the next pass".to_string()
    } else if active == 0 {
        format!("chart tab visible, measured {cw} x {ch} px")
    } else {
        format!("chart tab hidden, last measured {cw} x {ch} px")
    };
    let body = Scene::Text(
        TextNode::styled(
            format!("active tab: {tab_name} (index {active}); {size_part}; split ratio {ratio:.2}"),
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
                .flex(FlexDirection::Column)
                .with_gap(6)
                .with_size(
                    Size::auto()
                        .with_width(SizeValue::Percent(100))
                        .with_height(SizeValue::Percent(100)),
                ),
        ),
    )
}

/// view-fn (§6.3): pure sync `() -> Scene`. Reads the live topology from its
/// reactive signal (a tab switch's `Signal::set` re-renders), then lowers it into
/// the dock surface, calling back for each pane's content. In a `Tabs` well the
/// walker calls `panel_content` ONLY for the active panel — so when the notes tab
/// is active, `chart_pane_content` is never invoked and the chart tag is absent.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // Read the live topology (subscribes the view). The seed is `Some` and never
    // empties; the fallback keeps the view total over the universal Option state.
    let topology = use_topology_signal().get().unwrap_or_else(build_topology);
    let active = topology.tab_well_active(WELL).unwrap_or(0);
    let workspace = view_dock_surface(
        &topology,
        |panel_id| match panel_id {
            CHART_PANEL => chart_pane_content(&theme),
            NOTES_PANEL => notes_pane_content(&theme),
            READOUT_PANEL => readout_pane_content(&theme, active),
            other => Scene::Text(TextNode::styled(
                format!("(unknown panel: {other})"),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(12)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )),
        },
        // The split's live state — the shared ratio signal (the drag writes it,
        // this reads it). `dragging` is the cosmetic handle-tint mirror.
        |_split_id, _initial| DockSplitState {
            ratio_signal: use_split_ratio(),
            dragging: false,
        },
        // No in-flight reorganize drag: no panel shows a drop-zone overlay.
        |_panel_id| None,
        &theme,
    );
    // The dock surface fills the window on the theme Surface (so panel gaps sit on
    // the theme colour, not the black render clear).
    Scene::Container(
        ContainerNode::new(vec![workspace])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new().flex(FlexDirection::Column).with_size(
                    Size::auto()
                        .with_width(SizeValue::Percent(100))
                        .with_height(SizeValue::Percent(100)),
                ),
            ),
    )
}

/// The binding. Hand-written (not `#[widget]`-derived), PR-51 display-only like
/// `hello-dock-chart`: the interactive elements are the splitter + the tab well
/// (extra externals), and the chart is display-only, so there is no primary
/// surface.
struct TabbedChartView;

impl WidgetCore for TabbedChartView {
    type State = ();
    type Event = ();

    /// (PR-51) No primary surface: the interactive elements are the splitter and
    /// the tab well (extra externals); the chart is display-only.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-tabbed-chart has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-tabbed-chart has no primary surface — see primary_surface()")
    }

    /// Two extra externals: the `SplitterExternal` (well↔readout resize, sharing
    /// the ratio signal the view reads) and the `TabWellExternal` (click-to-switch
    /// the chart↔notes tabs, routing through the shared reorganizer — the sole
    /// writer of the topology's active tab).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let splitter =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(use_split_ratio());
        let well = TabWellExternal::new(WELL, use_reorganizer());
        vec![
            ExtraExternal::new(CHART_SPLIT, Box::new(splitter)),
            ExtraExternal::new(WELL, Box::new(well)),
        ]
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-tabbed-chart (R1409: a chart lives in a dock TAB well)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (the chart's only input is its measured tab-well cell)".to_string()
    }
}

// §5.40 §2 #7 — the tab well announces itself. `dock_tablist_access_nodes` walks
// the live topology and emits the WAI-ARIA `tablist` / `tab` / `tabpanel`
// AccessNodes (the same lifted helper `hello-dock-panels-editor` uses, R1095), so
// an AT — or an AI reading `scene/access` — discovers the well, its two tabs,
// which one is aria-selected, and the active tabpanel: the same structure the user
// sees. A chart living in a tab well the AI cannot enumerate would violate the
// §2 #7 "AI-introspection 1st-class" invariant, so the example owns this, not just
// the resize seam. The chart's OWN content description is `pinion-chart`'s (R1359
// `describedby_region`); this binding adds only the tab-well structure around it.
impl WidgetA11y for TabbedChartView {
    fn access_node(_state: &Self::State, focused: Option<&str>) -> Vec<AccessNode> {
        // `access_node` runs in the shell's owner scope (the R1095 editor precedent),
        // so the live topology signal resolves; a tab switch's `Signal::set` moves
        // aria-selected here exactly as it moves the painted strip.
        use_topology_signal()
            .get()
            .as_ref()
            .map(|topology| dock_tablist_access_nodes(topology, focused))
            .unwrap_or_default()
    }

    /// R1518 §5.40 — publish the focus-target half of the same walk, so a strip
    /// that owns focus names its active tab as the `aria-activedescendant`
    /// instead of being reported atomically.
    fn access_focus_target(_state: &Self::State, focused: Option<&str>) -> Option<AccessFocus> {
        use_topology_signal()
            .get()
            .as_ref()
            .and_then(|topology| dock_tablist_focus_target(topology, focused))
    }
}

impl WidgetView for TabbedChartView {
    type Renderer = HelloTabbedChartRenderer;

    /// Resizable on purpose: a window resize, a splitter drag, AND a tab switch
    /// all drive the docked chart — the paths the round proves.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::OpenResizable {
            size: (WIN_W, WIN_H),
            min: Some((320, 220)),
        }
    }
}

fn main() {
    pinion_shell::run::<TabbedChartView>();
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

    /// One paint cycle through the REAL seam, exactly as `hello-dock-chart`'s test
    /// harness does: run the view under the shell's `root_owner`, lay it out,
    /// publish the post-layout pane rects, and — when that publish reports dirty —
    /// re-run view + layout once (the same-frame re-pass). Returns the painted
    /// scene plus whether the re-pass fired. Nothing seeds a signal by hand.
    fn paint_cycle(core: &CoreShell<TabbedChartView>, w: u32, h: u32) -> (Scene, bool) {
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

    /// Activate a tab through the SAME reorganizer the view reads (the click wire's
    /// `activate_tab` funnel), inside the shell's owner scope so the cached
    /// reorganizer + topology signal are the ones the next `paint_cycle` sees.
    fn activate_tab(core: &CoreShell<TabbedChartView>, index: usize) {
        core.root_owner().run(|| {
            let _ = use_reorganizer().activate_tab(WELL, index);
        });
    }

    #[test]
    fn the_well_hosts_the_chart_tab_active_at_boot() {
        let core: CoreShell<TabbedChartView> = CoreShell::new();
        let (scene, _) = paint_cycle(&core, WIN_W, WIN_H);

        // The chart root is present (the active tab), reached through the split +
        // well + panel containers; the notes body is NOT (it is the hidden tab).
        assert!(
            find(&scene, CHART_TAG).is_some(),
            "the chart tab is active at boot, so the chart root is in the scene"
        );
        assert!(
            find(&scene, NOTES_BODY_TAG).is_none(),
            "the notes tab is inactive at boot, so its body is absent"
        );
        assert!(
            find(&scene, READOUT_PANEL).is_some(),
            "the readout pane root carries its leaf id"
        );
    }

    #[test]
    fn the_chart_tab_measures_and_the_readout_names_it() {
        let core: CoreShell<TabbedChartView> = CoreShell::new();
        // The publish writes CHART_TAG's measured rect (reached through the split +
        // well + panel containers) — the seam the round proves, one wrapper deeper
        // than R1396's Leaf.
        let (scene, dirty) = paint_cycle(&core, WIN_W, WIN_H);
        assert!(
            dirty,
            "publishing the well-nested chart tag reports a change"
        );

        let chart = find(&scene, CHART_TAG).expect("chart present");
        let rect = chart.rect();
        assert!(
            rect.w > 0 && rect.h > 0,
            "the chart measured a real rect below the strip: {rect:?}"
        );
        let readout = find(&scene, READOUT_BODY_TAG).expect("readout body present");
        let text = match readout {
            Scene::Text(t) => t.content.clone(),
            _ => String::new(),
        };
        assert!(
            text.contains("chart tab visible") && text.contains("index 0"),
            "the readout names the visible chart tab + its measured size, got {text:?}"
        );
    }

    #[test]
    fn the_chart_rescales_when_the_window_widens() {
        let core: CoreShell<TabbedChartView> = CoreShell::new();
        let (narrow, _) = paint_cycle(&core, 520, 380);
        let narrow_w = find(&narrow, CHART_TAG).expect("chart").rect().w;
        let (wide, _) = paint_cycle(&core, 1180, 380);
        let wide_w = find(&wide, CHART_TAG).expect("chart").rect().w;
        assert!(
            wide_w > narrow_w,
            "a wider window widens the well's chart: {narrow_w} -> {wide_w}"
        );
    }

    #[test]
    fn switching_to_notes_hides_the_chart_then_back_restores_it() {
        let core: CoreShell<TabbedChartView> = CoreShell::new();
        let (boot, _) = paint_cycle(&core, WIN_W, WIN_H);
        assert!(
            find(&boot, CHART_TAG).is_some(),
            "chart visible on the active tab at boot"
        );

        // Activate the notes tab: the well renders only its active panel, so the
        // chart tag leaves the scene and the notes body enters it.
        activate_tab(&core, 1);
        let (notes, _) = paint_cycle(&core, WIN_W, WIN_H);
        assert!(
            find(&notes, CHART_TAG).is_none(),
            "a Tabs well renders only the active panel — the chart tag is absent while notes is active"
        );
        assert!(
            find(&notes, NOTES_BODY_TAG).is_some(),
            "the notes tab's body is now in the scene"
        );

        // Re-activate the chart tab: it reappears and re-measures.
        activate_tab(&core, 0);
        let (back, _) = paint_cycle(&core, WIN_W, WIN_H);
        let chart = find(&back, CHART_TAG).expect("chart reappears when its tab is re-activated");
        assert!(
            chart.rect().w > 0 && chart.rect().h > 0,
            "the reappeared chart re-measured a real rect: {:?}",
            chart.rect()
        );
    }

    #[test]
    fn the_tab_well_announces_itself_in_the_access_tree() {
        let core: CoreShell<TabbedChartView> = CoreShell::new();
        // `access_node` reads the live topology signal, so run it in the owner scope.
        let nodes = core
            .root_owner()
            .run(|| TabbedChartView::access_node(&(), None));

        let tablists: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role.aria_name() == "tablist")
            .collect();
        assert_eq!(tablists.len(), 1, "the well emits exactly one tablist");
        assert_eq!(
            tablists[0].tag, WELL,
            "the tablist is tagged with the well id"
        );

        let tabs: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role.aria_name() == "tab")
            .collect();
        assert_eq!(tabs.len(), 2, "the well has one tab per panel");
        // Address the tabs by their R51.42 `{well}#{i}` tags (not iteration order):
        // the chart tab is index 0 (active at boot) and the notes tab is index 1.
        let tab_selected = |i: usize| {
            let tag = format!("{WELL}#{i}");
            tabs.iter()
                .find(|n| n.tag == tag)
                .unwrap_or_else(|| panic!("a tab node tagged {tag}"))
                .selected
        };
        assert_eq!(
            tab_selected(0),
            Some(true),
            "the active chart tab (#0) is aria-selected"
        );
        assert_eq!(
            tab_selected(1),
            Some(false),
            "the inactive notes tab (#1) is not aria-selected"
        );
        assert!(
            nodes.iter().any(|n| n.role.aria_name() == "tabpanel"),
            "the active panel is exposed as a tabpanel"
        );
    }

    /// The headline: a Leaf chart is measured every frame, so a resize always
    /// reaches it. A *hidden* tab chart is absent from the scene, so a resize while
    /// it is hidden CANNOT measure it — its size signal goes stale. Re-activating
    /// the tab must catch that up: the first view reads the stale size, but the
    /// same-frame publish measures the now-resized pane, reports dirty, and the
    /// re-pass rebuilds the chart at the correct size. This is the reappear ->
    /// publish -> dirty -> re-pass chain the walker's active-only render forces.
    #[test]
    fn a_resize_while_the_chart_tab_is_hidden_is_caught_on_reactivation() {
        let core: CoreShell<TabbedChartView> = CoreShell::new();
        // Boot with the chart tab active in a WIDE window: it measures wide, so its
        // internal axes/series/labels are authored across the wide extent.
        let (_boot, _) = paint_cycle(&core, 1160, 460);

        // Hide the chart (activate notes), then resize the window NARROW while the
        // chart tag is absent — nothing measures it, so its signal holds the wide
        // size (the publish skips an absent tag, retaining its last rect).
        activate_tab(&core, 1);
        let (_notes, _) = paint_cycle(&core, 420, 460);

        // Re-activate the chart tab into the now-narrow pane. The first view reads
        // the STALE wide size; the same-frame publish catches the narrow pane.
        activate_tab(&core, 0);
        let (back, dirty) = paint_cycle(&core, 420, 460);
        assert!(
            dirty,
            "re-activating the chart tab into a resized pane makes the publish detect the stale size and fire the re-pass"
        );

        // DISCRIMINATING: the chart ROOT rect is narrow purely because the window is
        // narrow — `build_fill`'s `fill_parent` root is sized by layout, NOT by the
        // `(cw, ch)` it is handed — so a root-width check proves nothing about the
        // catch-up. The witness is the `(cw, ch)`-DRIVEN internal geometry: the x-axis
        // + tick labels are authored across the MEASURED width. Had the re-pass
        // rebuilt from the stale WIDE size, a tick label would sit at a wide x and
        // overflow the narrow chart's right edge (the R1396 clamp defeated). Assert
        // every x-tick label is contained inside the reappeared narrow chart — this
        // FAILS iff the internal build used the stale wide size.
        let chart = find(&back, CHART_TAG).expect("chart reappeared on re-activation");
        let cr = chart.rect();
        let chart_right = u64::from(cr.x) + u64::from(cr.w);
        let mut k = 0;
        while let Some(label) = find(&back, &format!("{CHART_TAG}.label.x.{k}")) {
            let lr = label.rect();
            let label_right = u64::from(lr.x) + u64::from(lr.w);
            assert!(
                label_right <= chart_right + 1,
                "x-tick label {k} ends at {label_right}, past the narrow chart's right edge {chart_right} — the re-pass rebuilt the internal geometry from the STALE wide size instead of the narrow pane"
            );
            k += 1;
        }
        assert!(
            k > 0,
            "the reappeared chart paints x-tick labels to check for containment"
        );
    }
}
