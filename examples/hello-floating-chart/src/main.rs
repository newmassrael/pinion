//! `hello-floating-chart` — R1410 consumer that PROVES a `pinion-chart` chart
//! re-measures inside a real **torn-off (floating) OS window**, the LAST
//! docked-chart placement R1396 (`Leaf`) and R1409 (`Tabs` well) left explicitly
//! UNEXERCISED.
//!
//! ## The two seams this crosses
//!
//! `hello-dock-chart` (R1396) proved a `LineChart::build_fill` re-scales inside a
//! dock **pane of one window**: the shell's post-layout `publish_pane_viewports`
//! walks that window's laid-out scene for `CHART_TAG` and writes its measured rect
//! to `use_pane_viewport_size(CHART_TAG)`. Separately, `pane_viewport_seam.rs`
//! (R1021) proved a **plain pane** torn off into its own OS window reflows to THAT
//! window's size — the publish runs per painted window, and a tag absent from a
//! window's scene is skipped (never clobbered). **No test or example crossed the
//! two**: a chart's `build_fill` measured inside a floating window. The
//! `pinion-chart` `debt-dataviz-dashboard-substrate` note flagged exactly this as
//! the remaining placement. This binding is that missing consumer.
//!
//! ## Why a floating window is not "just a resize"
//!
//! A window resize (R1396) re-measures a chart that stays in ONE window's paint
//! pass. A tear-off moves the chart into a **second winit window** with its own
//! paint pass, its own `compute_paint_scene_for_window` → `publish_pane_viewports`
//! call, feeding the ONE tag-keyed pane registry that lives on the shell's shared
//! `root_owner` (every window's view fn resolves `use_pane_viewport_size` there,
//! R680). The genuinely new behaviour, and the R1021.1 precondition this binding
//! must honour: `CHART_TAG` is drawn in **exactly one window per frame**. When the
//! chart floats, the main dock drops it to a placeholder (`CHART_TAG` absent from
//! main) and the floating window draws it — so the floating window's publish is
//! the one that measures it, and the chart reflows to the floating window's size,
//! which differs from the docked pane's.
//!
//! ## What it proves
//!
//! A horizontal split hosts a **chart panel** (a [`view_dock_panel`] wrapping a
//! `LineChart::build_fill`, tag [`CHART_TAG`], leaf id [`CHART_PANEL`]) beside a
//! **readout panel** that mirrors the seam's live state as scene data (§2 #7).
//! The chart panel carries a [`DockPanelExternal`] tear-off drag source.
//!
//! * **Tear off** (the header escapes the dock, or the AI `invoke("tear_off")`):
//!   the reducer pushes a `torn-chartpane` [`WindowSpec`] onto the reactive
//!   `windows_signal`; the shell's R683 reconcile Effect spawns the second window;
//!   its paint pass measures `CHART_TAG` and the chart reflows to the floating
//!   window's size. The main dock slot becomes a placeholder — the chart tag has
//!   LEFT the main scene, so the two windows never fight for the one tag.
//! * **Resize the floating window** (a native OS resize → winit `Resized` →
//!   that window's re-paint + `publish_pane_viewports`): the floating window's own
//!   publish re-measures the chart at ITS new size — the R1396 seam, in a window
//!   that is not the primary. This multi-size re-measure is pinned by the tests
//!   through `compute_paint_scene_for_window(id, w, h)` at 560 then 360, NOT by the
//!   demo: `scene/resize` is primary-window-only and a spawned window ignores a
//!   `scene/snapshot` viewport override, so no RPC drives a secondary window's size.
//! * **Dock back** (`invoke("tear_off")` again, or a header drag back over the
//!   panel's own slot): the reducer removes the `WindowSpec`; the reconcile Effect
//!   drops the window and the main dock re-installs the chart, which re-measures to
//!   the docked pane size.
//!
//! §2 #7 discovery is via scene-as-data, exactly as the `hello-dock-chart` leaf
//! precedent: `scene/windows` enumerates the floating window and a
//! `{window: "torn-chartpane"}`-scoped `scene/snapshot from=paint` reads the chart
//! (its `chart.*` tags) back inside it — no bespoke a11y structure is missing,
//! because a window is already first-class introspectable data (unlike a `Tabs`
//! well's roles, which R1409 had to announce explicitly).
//!
//! ## Verification
//!
//! `tools/demos/r1410_floating_chart.py` drives a real `invoke("tear_off")` over
//! RPC, gates on the floating window becoming addressable (the reconcile Effect is
//! async), and reads the chart back with a `{window}`-scoped
//! `scene/snapshot from=paint`, so the per-window measured-rect publish is
//! exercised end to end. The tests below pin the same behaviour through the real
//! `ShellCore` per-window paint pipeline (`compute_paint_scene` +
//! `compute_paint_scene_for_window`, the R1021 `pane_viewport_seam` precedent).

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{FlexDirection, LayoutStyle, Size, SizeValue, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{External, Frame, Owner, Scene, Signal, WidgetCore, use_pane_viewport_size};
use pinion_shell::{
    SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl, window_exists,
    window_topology_remove, window_topology_toggle,
};
use pinion_widget_paint::dock::{
    DEFAULT_FLOATING_WINDOW_PREFIX, DockPanelExternal, DockPanelStyle, FloatingPlaceholderStyle,
    floating_window_id as dock_floating_window_id, view_dock_panel, view_floating_placeholder,
};
use pinion_widget_paint::splitter::{
    SplitterExternal, SplitterOrientation, SplitterStyle, view_splitter,
};
use std::borrow::Cow;
use std::rc::Rc;

// pinion-forge codegen output — defines `HelloFloatingChartRenderer` +
// `HelloFloatingChartRendererError` (the Vello wrapper).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloFloatingChartRenderer, HelloFloatingChartRendererError);

/// Main window opening size only — the docked chart's geometry is derived from
/// the MEASURED pane, never from these.
const WIN_W: u32 = 760;
const WIN_H: u32 = 480;

/// Floating (torn-off) window size. Deliberately a DIFFERENT extent + aspect from
/// the docked chart pane (~half of `WIN_W`), so the re-measure after a tear-off is
/// witnessed by a size that could not be the stale docked one.
const FLOAT_W: u32 = 560;
const FLOAT_H: u32 = 360;

/// The floating window's declared outer position (logical px). Exercises the
/// R1087 [`WindowSpec::with_position`] seam and keeps the demo's window placement
/// deterministic instead of WM-default.
const FLOAT_X: i32 = 140;
const FLOAT_Y: i32 = 100;

/// Shared `ThemeProvider` cache key (the `"app"` gallery convention).
const THEME_TAG: &str = "app";

/// The primary window id (the docked home of the chart).
const MAIN_WINDOW_ID: &str = "main";

/// The horizontal split between the chart panel (left) and the readout panel
/// (right). It is BOTH the `SplitterExternal` registration tag and its ratio
/// signal cache key.
const CHART_SPLIT: &str = "chart_split";

/// The chart panel's leaf id — the [`DockPanelExternal`] registration tag (its
/// drained intents are prefixed with it, `intent-tag-dotted-wire-form`), the
/// [`view_dock_panel`] style tag, and the `{panel}` half of the `torn-{panel}`
/// floating-window id. MUST equal the first literal of
/// [`CHART_TEAR_OFF_INTENT_TAG`] et al. (pinned by the
/// `the_tear_off_intent_tag_matches_the_panel_id` test).
const CHART_PANEL: &str = "chartpane";

/// The readout panel's leaf id (right of the split) + its [`view_dock_panel`]
/// style tag.
const READOUT_PANEL: &str = "readoutpane";

/// The chart root's `tag_prefix` — BOTH the §2 #7 introspection prefix
/// (`chart.series.0` …) AND the measured-rect seam key. DISTINCT from
/// [`CHART_PANEL`] (the panel/external tag): the shell publishes the rect of the
/// node carrying THIS tag, reached by descending whichever window (dock pane or
/// floating window) currently draws it, and the pane content reads it back
/// through `use_pane_viewport_size(CHART_TAG)`.
const CHART_TAG: &str = "chart";

/// The readout body tag — a focus stop + the place the seam's live state is
/// mirrored as scene data (§2 #7): whether the chart is docked or floating, its
/// last-measured size, and the split ratio.
const READOUT_BODY_TAG: &str = "readout_body";

/// The initial split fraction (the chart panel's share of the width).
const BOOT_RATIO: f32 = 0.5;

/// The chart panel's `tear_off` intent (the R742 [`DockPanelExternal`] header
/// escape + the AI `invoke("tear_off")` toggle). The reducer TOGGLES the floating
/// window on it — create when docked, remove (dock-back) when floating.
const CHART_TEAR_OFF_INTENT_TAG: &str = intent_tag!("chartpane", "tear_off");

/// The chart panel's `tear_off_follow` intent — a LIVE header drag that escaped
/// every dock zone with a forwarded cursor (the modern
/// [`DockPanelExternal::drag_release_at`] float path). The reducer ENSURES the
/// window exists (create-only; the redock arm removes it), so a real user drag —
/// not just the AI toggle — floats the chart.
const CHART_TEAR_OFF_FOLLOW_INTENT_TAG: &str = intent_tag!("chartpane", "tear_off_follow");

/// The chart panel's `tear_off_redock` intent — a live drag that ended back over
/// the panel's own slot while floating (`DropResolution::SnapBack`). The reducer
/// REMOVES the window (dock-back).
const CHART_TEAR_OFF_REDOCK_INTENT_TAG: &str = intent_tag!("chartpane", "tear_off_redock");

/// Cache-or-create the shared split-ratio signal (the `hello-dock-chart`
/// `Owner::cache` idiom). The same key in `create_extra_externals` (which
/// `attach_ratio`s it onto the `SplitterExternal`) and in `view_main_dock`
/// (which reads it) returns the SAME `Rc<Signal<f32>>`.
fn use_split_ratio() -> Rc<Signal<f32>> {
    Owner::current()
        .expect("hello-floating-chart: runs inside an owner scope")
        .cache(CHART_SPLIT, || Signal::new(BOOT_RATIO))
}

/// The reactive window topology — the SSOT for "is the chart floating". Seeded
/// with the single main window; the tear-off reducer appends / removes the
/// `torn-chartpane` floating spec. The shell's `reconcile_windows` Effect
/// subscribes this at boot (via `windows_signal`) and drives real winit window
/// add / drop on each `Signal::set`. Cached so `windows_signal`, the reducer, and
/// the view fns all resolve the SAME signal.
fn use_windows_topology() -> Rc<Signal<Vec<WindowSpec>>> {
    Owner::current()
        .expect("hello-floating-chart: windows_signal runs inside the substrate root owner scope")
        .cache("floating_chart_windows", || {
            Signal::new(vec![WindowSpec::new(
                Cow::Borrowed(MAIN_WINDOW_ID),
                "hello-floating-chart — Main (R1410)",
                SizeStrategy::OpenResizable {
                    size: (WIN_W, WIN_H),
                    min: Some((320, 240)),
                },
            )])
        })
}

/// The canonical floating-window id for the chart panel (`torn-chartpane`) — a
/// thin wrapper over the lifted [`dock_floating_window_id`] +
/// [`DEFAULT_FLOATING_WINDOW_PREFIX`] SSOT, so the reducer, `view_for_window`, and
/// the floating `WindowSpec` all name the one window identically.
fn floating_window_id() -> String {
    dock_floating_window_id(DEFAULT_FLOATING_WINDOW_PREFIX, CHART_PANEL)
}

/// The floating `WindowSpec` the torn-off chart opens into — a decorated (OS
/// title-bar) window at a fixed declared position. Decorated, not borderless: the
/// `hello-dock-panels` flat-consumer convention (the borderless / custom-chrome
/// floater is `hello-dock-panels-editor`), so the `DockPanelExternal` needs no
/// `with_floating_window` and a header drag in this window is a dock-back tear-off
/// rather than a window move.
fn floating_window_spec() -> WindowSpec {
    WindowSpec::new(
        Cow::Owned(floating_window_id()),
        "hello-floating-chart — Chart (floating)",
        SizeStrategy::Fixed {
            width: FLOAT_W,
            height: FLOAT_H,
        },
    )
    .with_position(FLOAT_X, FLOAT_Y)
}

/// `true` iff the chart's floating window currently exists in `panels` — the
/// lifted [`window_exists`] predicate keyed on the `torn-chartpane` id.
fn is_chart_floating(panels: &[WindowSpec]) -> bool {
    window_exists(panels, &floating_window_id())
}

/// Ensure the chart's floating window exists (create-only, idempotent). Drives the
/// `tear_off_follow` arm — a live header drag that escaped the dock. `Signal::set`'s
/// equality-skip collapses a repeated per-move follow to no repaint once created.
fn ensure_chart_floating() {
    let signal = use_windows_topology();
    let mut current = signal.get();
    if !window_exists(&current, &floating_window_id()) {
        current.push(floating_window_spec());
        signal.set(current);
    }
}

/// Remove the chart's floating window if present (dock-back). Idempotent no-op when
/// already docked. Drives the `tear_off_redock` arm. The mutation shell is the
/// lifted [`window_topology_remove`] (R1410 rule-of-three, shared with the two dock
/// consumers); this binding's only job is to name its one window.
fn redock_chart_floating() {
    window_topology_remove(&use_windows_topology(), &floating_window_id());
}

/// Toggle the chart's floating window — create when docked, remove when floating.
/// Drives the `tear_off` arm (the AI `invoke("tear_off")` toggle + the cursor-less
/// drag fallback) and is the deterministic hook the tests drive directly. The
/// mutation shell is the lifted [`window_topology_toggle`]; `floating_window_spec`
/// is this binding's per-window float policy, invoked only on the create arm.
fn toggle_chart_floating() {
    window_topology_toggle(
        &use_windows_topology(),
        &floating_window_id(),
        floating_window_spec,
    );
}

/// Four deterministic series — enough that a narrow floating window's legend must
/// shrink and then collapse to a `+N` marker (the R1396 clamp). Data is
/// programmatic; the tear-off + re-measure behaviour is what this binding shows.
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

/// The chart panel's content: a `LineChart` that FILLS its pane. `build_fill`
/// reads the measured size from `use_pane_viewport_size(CHART_TAG)` — `(0, 0)`
/// until the painting window's post-layout publish lands, then the same-frame
/// re-pass rebuilds at the real size. Byte-for-byte the `hello-dock-chart`
/// pattern; the only difference is the pane may live in the main dock OR the
/// floating window, and this same fn authors both.
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

/// The readout panel's content: names the seam's live state as scene data — where
/// the chart is (docked vs floating), its last-measured size, and the split ratio.
/// It lets the demo (and an AI) confirm the tear-off + re-measure by reading text,
/// and it is the neighbour a pre-R1396 narrow chart would have painted over. The
/// size mirror reads the SAME shared registry the floating window's publish writes,
/// so once the chart floats and re-measures, this readout in the MAIN window
/// reflects the floating size (a cross-window §2 #7 witness).
/// What the readout says, as one derivation.
///
/// R1581 — the paint and the accessible node read this, so the sentence a
/// screen reader is given and the sentence on screen cannot be two sentences.
fn readout_body_text(floating: bool, cw: u32, ch: u32, ratio: f32) -> String {
    let where_part = if floating {
        format!("chart is FLOATING in window {}", floating_window_id())
    } else {
        "chart is DOCKED in the left pane".to_string()
    };
    let size_part = if cw == 0 || ch == 0 {
        "unmeasured — it paints on the next pass".to_string()
    } else {
        format!("last measured {cw} x {ch} px")
    };
    format!("{where_part}; {size_part}; split ratio {ratio:.2}")
}

fn readout_pane_content(theme: &Theme, floating: bool) -> Scene {
    let (cw, ch) = use_pane_viewport_size(CHART_TAG);
    let ratio = use_split_ratio().get();
    let heading = Scene::Text(
        TextNode::styled(
            "floating-chart readout".to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(
            LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );
    let body = Scene::Text(
        TextNode::styled(
            readout_body_text(floating, cw, ch, ratio),
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

/// The chart panel — a [`view_dock_panel`] wrapping the chart, OR a placeholder
/// when the chart is floating. When floating the placeholder keeps the layout slot
/// present (so dock-back re-installs cleanly) AND, crucially, omits `CHART_TAG`
/// from the main scene: the R1021.1 precondition that the chart tag is drawn in
/// exactly one window per frame. The `DockPanelExternal` at [`CHART_PANEL`]
/// services both the docked header and the placeholder header (dock-back drag).
fn chart_panel(theme: &Theme, floating: bool) -> Scene {
    if floating {
        view_floating_placeholder(
            CHART_PANEL,
            "Chart",
            theme,
            &FloatingPlaceholderStyle::m3_default(),
        )
    } else {
        view_dock_panel(
            "Chart",
            chart_pane_content(theme),
            theme,
            &DockPanelStyle::m3_default(CHART_PANEL),
            None,
        )
    }
}

/// The main window's dock layout — a horizontal split of the chart panel (left,
/// swapped to a placeholder when floating) and the readout panel (right). Reads
/// the live window topology so a tear-off's `Signal::set` re-renders both panes.
fn view_main_dock() -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let floating = is_chart_floating(&use_windows_topology().get());
    let chart_pane = chart_panel(&theme, floating);
    let readout = view_dock_panel(
        "Readout",
        readout_pane_content(&theme, floating),
        &theme,
        &DockPanelStyle::m3_default(READOUT_PANEL),
        None,
    );
    view_splitter(
        chart_pane,
        readout,
        &use_split_ratio(),
        &theme,
        &SplitterStyle::m3_default(SplitterOrientation::Horizontal, CHART_SPLIT),
        false,
    )
}

/// The floating window's paint — the chart wrapped in its own [`view_dock_panel`]
/// so the torn-off window carries a draggable header (a header drag back re-emits
/// the `tear_off` chain → dock-back). The SAME `chart_pane_content` as the docked
/// path, so `CHART_TAG` is authored identically; only the window it lands in — and
/// therefore the pane size the publish measures — differs.
fn view_floating_chart() -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    view_dock_panel(
        "Chart",
        chart_pane_content(&theme),
        &theme,
        &DockPanelStyle::m3_default(CHART_PANEL),
        None,
    )
}

/// The binding. Hand-written (not `#[widget]`-derived), PR-51 display-only like
/// `hello-dock-chart`: the interactive elements are the splitter + the dock-panel
/// tear-off (extra externals), and the chart is display-only, so there is no
/// primary surface.
struct FloatingChartView;

impl WidgetCore for FloatingChartView {
    type State = ();
    type Event = ();

    /// (PR-51) No primary surface: the interactive elements are the splitter and
    /// the dock-panel tear-off drag source (both extra externals); the chart is
    /// display-only.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-floating-chart has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-floating-chart has no primary surface — see primary_surface()")
    }

    /// Two extra externals: the `SplitterExternal` (chart↔readout resize, sharing
    /// the ratio signal the view reads) and the `DockPanelExternal` (the R742
    /// tear-off drag source at [`CHART_PANEL`] — tear-off-only, no reorganizer;
    /// decorated floater, so no `with_floating_window`). Its drained intents drive
    /// the reducer's float / follow / redock arms.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let splitter =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(use_split_ratio());
        let panel = DockPanelExternal::new(CHART_PANEL);
        vec![
            ExtraExternal::new(CHART_SPLIT, Box::new(splitter)),
            ExtraExternal::new(CHART_PANEL, Box::new(panel)),
        ]
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view(state: Self::State, frame: &Frame) -> Scene {
        // Single-window fallback (the RPC `scene/snapshot` without `{window}`):
        // the live loop always calls `view_for_window`.
        let _ = (state, frame);
        view_main_dock()
    }

    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }

    /// R1410 §5.51 §5.16 — the tear-off reducer. One panel, three arms mirroring
    /// the `DockPanelExternal` gesture surface so BOTH the AI `invoke("tear_off")`
    /// AND a live header drag float / dock the chart:
    ///
    /// * `tear_off` — TOGGLE (AI invoke + cursor-less drag fallback).
    /// * `tear_off_follow` — ENSURE the window (a live drag that escaped the dock
    ///   with a forwarded cursor); create-only, the redock arm removes it.
    /// * `tear_off_redock` — REMOVE the window (a live drag back over the slot).
    fn update(_state: Self::State, intent: &Intent) -> Vec<pinion_core::command::Command> {
        // The payload carries the panel id the `DockPanelExternal` was built with;
        // this binding has a single floatable panel, so the tag match already
        // scopes it and the payload is not re-inspected.
        match intent.tag_str() {
            tag if tag == CHART_TEAR_OFF_INTENT_TAG => toggle_chart_floating(),
            tag if tag == CHART_TEAR_OFF_FOLLOW_INTENT_TAG => ensure_chart_floating(),
            tag if tag == CHART_TEAR_OFF_REDOCK_INTENT_TAG => redock_chart_floating(),
            _ => {}
        }
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-floating-chart (R1410: a chart re-measures in a torn-off window)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (the chart's only input is its measured pane, docked or floating)".to_string()
    }
}

// Default a11y surface — the chart's own AT description is `pinion-chart`'s
// (R1359 `describedby_region`); §2 #7 discovery of the floating chart is via
// scene-as-data (`scene/windows` + a `{window}`-scoped `scene/snapshot`), the
// `hello-dock-chart` leaf precedent. Unlike R1409's `Tabs` well (whose roles are
// not in the scene), a window is already first-class introspectable, so this
// binding adds no bespoke AccessNodes.
impl WidgetA11y for FloatingChartView {
    /// R1581 §5.40 — the readout body is a keyboard FOCUS STOP
    /// (`with_focusable`), and a focus stop with no node in the AT tree is one
    /// `AccessTreeBuilder` folds onto the window root, so tabbing to it
    /// announces the window instead of the readout. Same sentence the pane
    /// paints, from one derivation.
    ///
    /// `access_node` runs in the shell's owner scope, so the topology,
    /// pane-viewport and split-ratio hooks resolve here as they do in the view.
    fn access_node(_state: &Self::State, _focused: Option<&str>) -> Vec<AccessNode> {
        let floating = is_chart_floating(&use_windows_topology().get());
        let (cw, ch) = use_pane_viewport_size(CHART_TAG);
        let ratio = use_split_ratio().get();
        vec![
            AccessNode::new(READOUT_BODY_TAG, AriaRole::Status)
                .with_name("floating-chart readout")
                .with_value(AccessValue::Text(readout_body_text(
                    floating, cw, ch, ratio,
                ))),
        ]
    }
}

impl WidgetView for FloatingChartView {
    type Renderer = HelloFloatingChartRenderer;

    /// Resizable on purpose: a window resize AND a splitter drag both re-scale the
    /// docked chart, and the floating window re-measures the torn-off chart — the
    /// three re-measure paths the round proves.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::OpenResizable {
            size: (WIN_W, WIN_H),
            min: Some((320, 240)),
        }
    }

    /// R683 §5.16 §5.41 — opt-in runtime window topology: the shell's
    /// `reconcile_windows` Effect subscribes this at boot and spawns / drops the
    /// floating window on each tear-off `Signal::set`.
    fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
        Some(use_windows_topology())
    }

    /// R670.B + R683 §5.16 — per-window paint dispatch. The main window paints the
    /// dock; the `torn-chartpane` window paints the chart via [`view_floating_chart`].
    fn view_for_window(window_id: &str, _state: Self::State, _frame: &Frame) -> Scene {
        if window_id.strip_prefix(DEFAULT_FLOATING_WINDOW_PREFIX) == Some(CHART_PANEL) {
            view_floating_chart()
        } else {
            view_main_dock()
        }
    }
}

fn main() {
    pinion_shell::run::<FloatingChartView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;
    use pinion_shell::ShellCore;

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

    /// Drive the tear-off toggle through the SAME `use_windows_topology` signal the
    /// view reads (the deterministic hook the AI `invoke("tear_off")` and the
    /// cursor-less drag also reach), inside the shell's owner scope so the cached
    /// signal is the one the next paint sees.
    fn toggle(core: &ShellCore<FloatingChartView>) {
        core.root_owner().run(toggle_chart_floating);
    }

    /// The docked chart is measured in the MAIN window's pane, reached through the
    /// splitter + dock-panel containers — the R1396 seam.
    #[test]
    fn the_docked_chart_measures_in_the_main_window() {
        let mut core: ShellCore<FloatingChartView> = ShellCore::new();
        let scene = core.compute_paint_scene(WIN_W, WIN_H);

        let chart = find(&scene, CHART_TAG).expect("docked chart present in the main window");
        let rect = chart.rect();
        assert!(
            rect.w > 0 && rect.h > 0,
            "the docked chart measured a real rect: {rect:?}"
        );
        // The readout names the docked state + the measured size.
        let readout = find(&scene, READOUT_BODY_TAG).expect("readout body present");
        let text = match readout {
            Scene::Text(t) => t.content.clone(),
            _ => String::new(),
        };
        assert!(
            text.contains("DOCKED") && text.contains("measured"),
            "the readout names the docked, measured chart, got {text:?}"
        );
    }

    /// The HEADLINE: after a tear-off the chart LEAVES the main scene (a
    /// placeholder takes its slot — the R1021.1 one-window precondition) and is
    /// measured in the FLOATING window's own paint pass, reflowing to that window's
    /// size, which differs from the docked pane size.
    #[test]
    fn the_torn_off_chart_re_measures_in_the_floating_window() {
        let mut core: ShellCore<FloatingChartView> = ShellCore::new();

        // Boot docked: the chart is in the main window, measured to its pane.
        let docked = core.compute_paint_scene(WIN_W, WIN_H);
        let docked_rect = find(&docked, CHART_TAG)
            .expect("docked chart present")
            .rect();
        assert!(docked_rect.w > 0 && docked_rect.h > 0);

        // Tear off: the main dock drops the chart to a placeholder.
        toggle(&core);
        let main_after = core.compute_paint_scene(WIN_W, WIN_H);
        assert!(
            find(&main_after, CHART_TAG).is_none(),
            "the floating chart's tag has LEFT the main scene (a placeholder takes its slot)"
        );
        // The readout (still in main) now names the floating state.
        let readout = find(&main_after, READOUT_BODY_TAG).expect("readout still in main");
        if let Scene::Text(t) = readout {
            assert!(
                t.content.contains("FLOATING"),
                "the readout names the floating chart, got {:?}",
                t.content
            );
        }

        // Paint the floating window: the chart re-appears there, measured to the
        // floating window's size (the per-window publish, R1021).
        let float_scene =
            core.compute_paint_scene_for_window(&floating_window_id(), FLOAT_W, FLOAT_H);
        let float_rect = find(&float_scene, CHART_TAG)
            .expect("the chart is in the floating window's scene")
            .rect();
        assert!(
            float_rect.w > 0 && float_rect.h > 0,
            "the torn-off chart measured a real rect in the floating window: {float_rect:?}"
        );

        // DISCRIMINATING (not a fill-parent tautology). `float_rect.h < docked_rect.h`
        // would pass even if the publish were disabled — the chart root is
        // `fill_parent`, so it always lays out to its pane regardless of the `(cw, ch)`
        // handed to `build_fill`. The witness is the `(cw, ch)`-DRIVEN internal
        // geometry: the floating window (FLOAT_H 360) is SHORTER than the docked pane
        // (~WIN_H 480), and the x-tick labels sit along the bottom of the authored
        // height. Had the floating window's publish not run, `build_fill` would read
        // the STALE, TALLER docked height and place the x-tick labels BELOW the shorter
        // floating chart's bottom edge (the paint adapter does not clip, R1356), or read
        // `(0, 0)` and paint NO labels. Assert every x-tick label is contained inside
        // the floating chart's bottom edge, and that some exist — both FAIL iff the
        // internal build used a stale/zero size instead of the floating measurement.
        let chart_bottom = u64::from(float_rect.y) + u64::from(float_rect.h);
        let mut k = 0;
        while let Some(label) = find(&float_scene, &format!("{CHART_TAG}.label.x.{k}")) {
            let lr = label.rect();
            let label_bottom = u64::from(lr.y) + u64::from(lr.h);
            assert!(
                label_bottom <= chart_bottom + 1,
                "x-tick label {k} ends at y {label_bottom}, below the shorter floating chart's bottom {chart_bottom} — the internal geometry was authored from the stale TALLER docked height, not the floating measurement"
            );
            k += 1;
        }
        assert!(
            k > 0,
            "the floating chart paints x-tick labels (a (0,0) unmeasured read would paint none)"
        );
    }

    /// DISCRIMINATING: the floating chart's `(cw, ch)`-driven INTERNAL geometry is
    /// authored across the FLOATING window's width — its x-tick labels stay
    /// contained in the floating chart's rect. Had the floating window's publish
    /// not run (the R1021 per-window seam broken) the chart would read `(0, 0)` and
    /// paint no labels, or read the stale wide docked size and overflow. This
    /// asserts the internal build used the floating measurement.
    #[test]
    fn the_floating_chart_internal_geometry_fits_the_floating_window() {
        let mut core: ShellCore<FloatingChartView> = ShellCore::new();
        let _ = core.compute_paint_scene(WIN_W, WIN_H);
        toggle(&core);
        let _ = core.compute_paint_scene(WIN_W, WIN_H);

        // A deliberately NARROW floating window so the containment is a real
        // constraint (a wide window would trivially contain the labels).
        let narrow_w = 360u32;
        let float_scene =
            core.compute_paint_scene_for_window(&floating_window_id(), narrow_w, FLOAT_H);
        let chart = find(&float_scene, CHART_TAG).expect("chart in the floating window");
        let cr = chart.rect();
        let chart_right = u64::from(cr.x) + u64::from(cr.w);

        let mut k = 0;
        while let Some(label) = find(&float_scene, &format!("{CHART_TAG}.label.x.{k}")) {
            let lr = label.rect();
            let label_right = u64::from(lr.x) + u64::from(lr.w);
            assert!(
                label_right <= chart_right + 1,
                "x-tick label {k} ends at {label_right}, past the narrow floating chart's right edge {chart_right} — the internal geometry was not authored from the floating measurement"
            );
            k += 1;
        }
        assert!(
            k > 0,
            "the floating chart paints x-tick labels to check for containment"
        );
    }

    /// Dock-back: after a second toggle the floating window's spec is gone and the
    /// chart re-installs in the main dock, re-measured to the docked pane size.
    #[test]
    fn docking_back_restores_the_chart_to_the_main_window() {
        let mut core: ShellCore<FloatingChartView> = ShellCore::new();
        let _ = core.compute_paint_scene(WIN_W, WIN_H);

        toggle(&core); // float
        let floated = core
            .root_owner()
            .run(|| is_chart_floating(&use_windows_topology().get()));
        assert!(floated, "the first toggle floats the chart");

        toggle(&core); // dock back
        let docked = core
            .root_owner()
            .run(|| is_chart_floating(&use_windows_topology().get()));
        assert!(!docked, "the second toggle docks the chart back");

        let scene = core.compute_paint_scene(WIN_W, WIN_H);
        let chart = find(&scene, CHART_TAG).expect("the chart re-installed in the main dock");
        assert!(
            chart.rect().w > 0 && chart.rect().h > 0,
            "the re-docked chart re-measured a real rect: {:?}",
            chart.rect()
        );
    }

    /// The reducer routes the real `DockPanelExternal` intents. Feeding the dotted
    /// wire-form tags through `update` toggles / ensures / removes the floating
    /// window exactly as the live gesture does — proving the arms match the tags
    /// the drain actually produces (not a bare event name).
    #[test]
    fn the_reducer_arms_match_the_drained_intent_tags() {
        let core: ShellCore<FloatingChartView> = ShellCore::new();
        core.root_owner().run(|| {
            let payload = || IntrospectValue::Text(CHART_PANEL.to_string());

            // tear_off toggles ON.
            let _ = FloatingChartView::update(
                (),
                &Intent {
                    tag: Cow::Borrowed(CHART_TEAR_OFF_INTENT_TAG),
                    payload: payload(),
                },
            );
            assert!(
                is_chart_floating(&use_windows_topology().get()),
                "tear_off floats"
            );

            // tear_off_redock docks back.
            let _ = FloatingChartView::update(
                (),
                &Intent {
                    tag: Cow::Borrowed(CHART_TEAR_OFF_REDOCK_INTENT_TAG),
                    payload: payload(),
                },
            );
            assert!(
                !is_chart_floating(&use_windows_topology().get()),
                "tear_off_redock docks back"
            );

            // tear_off_follow ensures (creates) it.
            let _ = FloatingChartView::update(
                (),
                &Intent {
                    tag: Cow::Borrowed(CHART_TEAR_OFF_FOLLOW_INTENT_TAG),
                    payload: payload(),
                },
            );
            assert!(
                is_chart_floating(&use_windows_topology().get()),
                "tear_off_follow floats"
            );
        });
    }

    /// Pin the `intent_tag!` / panel-id coupling the macro doc mandates: the
    /// reducer's compile-time tags equal the runtime dotted form the drain builds
    /// from `CHART_PANEL`.
    #[test]
    fn the_tear_off_intent_tag_matches_the_panel_id() {
        assert_eq!(CHART_TEAR_OFF_INTENT_TAG, format!("{CHART_PANEL}.tear_off"));
        assert_eq!(
            CHART_TEAR_OFF_FOLLOW_INTENT_TAG,
            format!("{CHART_PANEL}.tear_off_follow")
        );
        assert_eq!(
            CHART_TEAR_OFF_REDOCK_INTENT_TAG,
            format!("{CHART_PANEL}.tear_off_redock")
        );
    }
}
