//! `hello-category-axis` — R1545 §5.38 a **category** is an axis kind.
//!
//! The forcing consumer for [`pinion_chart::Categories`] / [`pinion_chart::CategoryScale`], the crate's fourth axis kind (the
//! toolkit's bar category axis, d3's `scaleBand`).
//!
//! ## What was missing, given that bar charts already had categories
//!
//! [`pinion_chart::BarChart`] has drawn a categorical x since R1374 — but as
//! a private slot metric, `left + i * slot`, written out three times inside
//! `bar.rs` for the bar box, its label and its click surface. It was not an
//! axis: no other chart could take it, no consumer could ask where a category
//! was drawn, and none of the machinery the other three kinds share (the tick
//! set, the label format, the domain pinning a zoom drives) reached it.
//!
//! This window draws the same twelve monthly buckets **twice from one axis** —
//! a bar chart of the month's revenue, and a
//! [`LineChart::x_category`](pinion_chart::LineChart::x_category) of its
//! target attainment over the same slots. The line chart is the proof that the
//! axis is now a *kind*: it is a numeric-x chart, and it takes the categorical
//! axis exactly as it takes the log and time ones.
//!
//! ## The window is set by name, and the name is resolved
//!
//! The toolkit windows a category axis with `setRange(string, string)`, which returns `void`. A name
//! that is not a category leaves the axis silently unwindowed, and a name
//! carried by two categories resolves to the first with nothing said. Here
//! [`Categories::window`](pinion_chart::Categories::window) answers a `Result`, so the failure has
//! to be handled before it can reach a chart — the caption under the plots is
//! that report, and preset <kbd>3</kbd> asks for a renamed month on purpose,
//! the shape a saved dashboard view takes after a category is renamed
//! upstream.
//!
//! ## Driving it
//!
//! The toolbar is focusable. <kbd>1</kbd> / <kbd>2</kbd> / <kbd>3</kbd> pick a
//! window preset, <kbd>Left</kbd> / <kbd>Right</kbd> pan it by a whole
//! category, <kbd>0</kbd> clears it. Every one of those is also an RPC action
//! on the window external (`range` / `pan` / `reset`), which is the primary
//! path (§2 #2).
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the axis as tagged data: `bars.bar.{i}` /
//! `bars.xlabel.{i}` carry the **category index**, so which categories are in
//! view — and how wide their bands are — is read off the tags, not sampled
//! from pixels (§2 #1 / §2 #7). `scene/query` on the window external answers
//! the resolved `lo` / `hi` / `visible` and any lookup `error`. See
//! `tools/demos/r1545_category_axis.py`.

use pinion_a11y::chart::chart_table_nodes;
use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_chart::{
    Bar, BarChart, Categories, CategoryWindow, ChartStyle, DataPoint, LineChart, Series,
};
use pinion_core::Modifiers;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCategoryAxisRenderer, HelloCategoryAxisRendererError);

const WIN_W: u32 = 720;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";

/// The window toolbar's tag: the focusable Tab stop, and the node the window
/// [`External`] is attached to. One place the category range lives, so the bar
/// chart, the line chart and the caption cannot window differently.
const WINDOW_TAG: &str = "category_window";

const TITLE_FONT_PX: u32 = 17;
const CAPTION_FONT_PX: u32 = 12;
const CHIP_FONT_PX: u32 = 13;

/// The axis's categories — the twelve monthly buckets both charts plot over.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// R1633 — the **dense** axis: thirty service endpoints, the shape an
/// analyzer-class dashboard's category axis actually has.
///
/// Twelve three-letter months fit any window this example opens, so until this
/// list existed nothing in the tree forced a category axis to have more labels
/// than room — which is why `pinion_chart` drew every one of them on top of the
/// next from R1374 until R1633. A forcing consumer that only exercises the easy
/// arity is not forcing the axis.
const ENDPOINTS: [&str; 30] = [
    "/health",
    "/login",
    "/logout",
    "/session",
    "/refresh",
    "/profile",
    "/avatar",
    "/settings",
    "/search",
    "/index",
    "/upload",
    "/download",
    "/thumbnail",
    "/transcode",
    "/notify",
    "/subscribe",
    "/publish",
    "/metrics",
    "/trace",
    "/audit",
    "/billing",
    "/invoice",
    "/refund",
    "/webhook",
    "/callback",
    "/export",
    "/import",
    "/purge",
    "/status",
    "/version",
];

/// Revenue per month (thousands) — the bar chart's values.
const REVENUE: [f64; 12] = [
    182.0, 164.0, 219.0, 248.0, 231.0, 276.0, 198.0, 205.0, 262.0, 288.0, 301.0, 344.0,
];

/// Target attainment per month (percent) — the line chart's values, over the
/// SAME twelve slots.
const ATTAINMENT: [f64; 12] = [
    91.0, 82.0, 104.0, 118.0, 110.0, 131.0, 94.0, 97.0, 124.0, 137.0, 143.0, 164.0,
];

/// One toolbar preset: the digit that selects it, its chip label, and the
/// category NAME pair it asks for (`None` = clear the window).
type Preset = (
    &'static str,
    &'static str,
    Option<(&'static str, &'static str)>,
);

/// The three window presets.
///
/// The third names a month the axis does not carry. It is the round's the
/// toolkit comparison made reachable: with `setRange(string, string)` this request is a no-op the
/// caller cannot detect.
const PRESETS: [Preset; 3] = [
    ("1", "all months", None),
    ("2", "Apr-Jun", Some(("Apr", "Jun"))),
    ("3", "stale view", Some(("Smarch", "Jun"))),
];

/// Window-absolute plot regions. A chart must be handed its final geometry
/// before layout runs (see the `pinion-chart` coordinate contract).
const BAR_RECT: Rect = Rect::new(16, 56, WIN_W - 32, 226);
const LINE_RECT: Rect = Rect::new(16, 290, WIN_W - 32, 176);

/// The axis's category names, dense or not (R1633).
///
/// One function, so the charts, the window resolution and the caption cannot
/// disagree about which axis is on screen — the same reason `months` was one
/// place before it took an argument.
fn axis_names(dense: bool) -> &'static [&'static str] {
    if dense { &ENDPOINTS } else { &MONTHS }
}

/// The one place the category list is built, so both charts and the window
/// resolution read the same axis.
fn months(dense: bool) -> Categories {
    Categories::new(axis_names(dense).iter().copied())
}

/// The categorical x-axis window, requested by category NAME (the toolkit's
/// `setRange`) and resolved before it reaches a chart.
///
/// Holding the *request* rather than the resolved indices is deliberate: it is
/// what the toolkit's API takes, and it is the form in which a saved view or a
/// URL carries a window. The resolution then happens once, here, so the charts
/// and the caption cannot disagree about whether it succeeded.
#[derive(Debug, Clone, Default)]
struct CategoryWindowExternal {
    /// The requested `(from, to)` names. `None` = every category in view.
    request: Option<(String, String)>,
    /// R1633 — whether the axis is the **dense** one. A window request is by
    /// NAME, and the two lists share none, so switching clears the request
    /// rather than leaving one that cannot resolve.
    dense: bool,
}

impl CategoryWindowExternal {
    /// Resolve the current request against the axis: `Ok(None)` for the
    /// un-windowed axis, `Ok(Some(w))` for a resolved window, `Err` naming the
    /// endpoint that does not resolve.
    fn resolve(&self) -> Result<Option<CategoryWindow>, String> {
        match &self.request {
            None => Ok(None),
            Some((from, to)) => months(self.dense)
                .window(from, to)
                .map(Some)
                .map_err(|e| e.to_string()),
        }
    }

    /// Apply a request by name, answering the lookup failure as text (empty
    /// when it resolved).
    fn set_range(&mut self, from: &str, to: &str) -> String {
        self.request = Some((from.to_string(), to.to_string()));
        self.resolve().err().unwrap_or_default()
    }

    /// Shift the window by `delta` categories, clamped inside the list, and
    /// write the moved endpoints back as NAMES. A request that does not
    /// resolve cannot be panned — there is no window to move — and neither can
    /// the un-windowed axis, which already shows everything.
    fn pan(&mut self, delta: i64) -> bool {
        let Ok(Some(w)) = self.resolve() else {
            return false;
        };
        let names = axis_names(self.dense);
        let last = index_i64(names.len() - 1);
        let span = index_i64(w.hi()) - index_i64(w.lo());
        let lo = (index_i64(w.lo()) + delta).clamp(0, last - span);
        if lo == index_i64(w.lo()) {
            return false;
        }
        self.request = Some((
            names[usize_of(lo)].to_string(),
            names[usize_of(lo + span)].to_string(),
        ));
        true
    }

    /// The resolved endpoints as wire integers, `-1` when there is no window
    /// (unset, or a request that did not resolve). `-1` rather than a silent
    /// `0`: a reader must be able to tell "no window" from "windowed onto the
    /// first category".
    fn bounds(&self) -> (i64, i64) {
        match self.resolve() {
            Ok(Some(w)) => (index_i64(w.lo()), index_i64(w.hi())),
            _ => (-1, -1),
        }
    }

    /// The requested endpoint names, empty when nothing is requested.
    fn requested(&self) -> (&str, &str) {
        self.request
            .as_ref()
            .map_or(("", ""), |(f, t)| (f.as_str(), t.as_str()))
    }
}

/// A category index as a wire integer.
#[allow(
    clippy::cast_possible_wrap,
    reason = "a month index is a display cardinality far below i64::MAX"
)]
fn index_i64(index: usize) -> i64 {
    index as i64
}

/// A clamped, non-negative wire integer back to a category index.
fn usize_of(v: i64) -> usize {
    usize::try_from(v.max(0)).unwrap_or(0)
}

impl External for CategoryWindowExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    /// The window changes only through `range` / `reset` / `pan`, and the
    /// framework repaints after each. Never self-dirty.
    fn is_dirty(&self) -> bool {
        false
    }
}

impl ExternalIntrospect for CategoryWindowExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("from", "string"),
                    SchemaField::new("to", "string"),
                    SchemaField::new("lo", "int"),
                    SchemaField::new("hi", "int"),
                    SchemaField::new("visible", "int"),
                    SchemaField::new("error", "string"),
                    SchemaField::action("range", "string"),
                    SchemaField::action("reset", "bool"),
                    SchemaField::action("pan", "bool"),
                    SchemaField::new("dense", "bool"),
                    // R1637 — the VERB has its own address. `dense` was
                    // declared readable and dispatched as an action, so
                    // one name carried two channels and the declaration
                    // could only ever state one of them. The reference
                    // splits the same way (a property and a `setDense()`
                    // live in different meta-object namespaces), and so
                    // does the rest of this tree (`set_sort`,
                    // `set_background`, `set_voice_gain`).
                    SchemaField::action("set_dense", "int"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let (from, to) = self.requested();
        let (lo, hi) = self.bounds();
        match path {
            "from" => Some(IntrospectValue::Text(from.to_string())),
            "to" => Some(IntrospectValue::Text(to.to_string())),
            "lo" => Some(IntrospectValue::Int(lo)),
            "hi" => Some(IntrospectValue::Int(hi)),
            // The count actually in view. The toolkit's `count()` answers every
            // category whatever the range is, so this number has no the
            // toolkit peer.
            "visible" => Some(IntrospectValue::Int(match self.resolve() {
                Ok(Some(w)) => index_i64(w.len()),
                _ => index_i64(axis_names(self.dense).len()),
            })),
            "error" => Some(IntrospectValue::Text(
                self.resolve().err().unwrap_or_default(),
            )),
            // R1633 — which axis is on. A bool and not a count, because the
            // count is derivable and this is the request.
            "dense" => Some(IntrospectValue::Bool(self.dense)),
            _ => None,
        }
    }

    /// Read-only: the window moves through the action channel, whose answers
    /// carry the resolution. A silent state write would be the very thing this
    /// round exists to remove.
    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::ReadOnly)
    }

    /// `range` takes `{"from": name, "to": name}` and answers the lookup
    /// failure as text (empty when it resolved), so the write channel reports
    /// what the toolkit's `void setRange` cannot.
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // R1633 — swap the axis for the dense one. The request is by NAME
            // and the two lists share none, so the window is CLEARED rather
            // than left holding one that cannot resolve — the same refusal
            // discipline the range verb has, applied to the state change that
            // would invalidate it.
            "set_dense" => {
                let IntrospectValue::Bool(on) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                self.dense = on;
                self.request = None;
                Ok(IntrospectValue::Int(index_i64(axis_names(on).len())))
            }
            "range" => {
                let IntrospectValue::Json(v) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let (Some(from), Some(to)) = (
                    v.get("from").and_then(serde_json::Value::as_str),
                    v.get("to").and_then(serde_json::Value::as_str),
                ) else {
                    return Err(InvokeError::TypeMismatch);
                };
                Ok(IntrospectValue::Text(self.set_range(from, to)))
            }
            "reset" => Ok(IntrospectValue::Bool(self.request.take().is_some())),
            "pan" => {
                let delta = match args {
                    IntrospectValue::Int(d) => d,
                    IntrospectValue::Json(serde_json::Value::Number(n)) => {
                        n.as_i64().ok_or(InvokeError::TypeMismatch)?
                    }
                    _ => return Err(InvokeError::TypeMismatch),
                };
                Ok(IntrospectValue::Bool(self.pan(delta)))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// The window state the view paints from — `Copy`, so the failing NAME is not
/// carried here. It does not need to be: a preset's names are static, so the
/// caption re-resolves them for the exact message, and an arbitrary
/// RPC-supplied name reaches a reader through `scene/query error`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct WindowState {
    /// The resolved window — `None` when unset OR when the request did not
    /// resolve, which is the same thing on screen: every category is shown.
    window: Option<CategoryWindow>,
    /// Whether the current request failed to resolve.
    unresolved: bool,
    /// Which [`PRESETS`] entry the current request matches.
    active_preset: Option<usize>,
    /// R1633 — whether the dense axis is on.
    dense: bool,
}

/// Read the window off the scene's external. An absent external shows every
/// category, so the charts are never blank because a lookup missed.
fn read_window(scene: &Scene) -> WindowState {
    let Some(intro) = scene
        .find_external_with_tag(WINDOW_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return WindowState {
            window: None,
            unresolved: false,
            active_preset: Some(0),
            dense: false,
        };
    };
    let text = |field: &str| match intro.query(field) {
        Some(IntrospectValue::Text(t)) => t,
        _ => String::new(),
    };
    let int = |field: &str| match intro.query(field) {
        Some(IntrospectValue::Int(i)) => i,
        _ => -1,
    };
    let (from, to) = (text("from"), text("to"));
    let (lo, hi) = (int("lo"), int("hi"));
    WindowState {
        window: (lo >= 0 && hi >= 0).then(|| CategoryWindow::new(usize_of(lo), usize_of(hi))),
        unresolved: !text("error").is_empty(),
        dense: matches!(intro.query("dense"), Some(IntrospectValue::Bool(true))),
        active_preset: PRESETS.iter().position(|(_, _, r)| match r {
            None => from.is_empty() && to.is_empty(),
            Some((f, t)) => from == *f && to == *t,
        }),
    }
}

/// The themed chart style.
fn chart_style(theme: &Theme, legend: bool) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        legend,
        label_size_px: 12,
        y_ticks: 4,
        ..ChartStyle::default()
    }
}

/// The bar chart for a resolved window — the ONE place `x_window` is applied.
fn bar_chart(window: Option<CategoryWindow>, dense: bool) -> BarChart {
    // The dense axis reuses the revenue figures cyclically: what it is here to
    // force is the AXIS's arity, and inventing thirty more plausible numbers
    // would say the data mattered when it does not.
    let bars = axis_names(dense)
        .iter()
        .enumerate()
        .map(|(i, m)| Bar::new(*m, REVENUE[i % REVENUE.len()]))
        .collect();
    let chart = BarChart::new(bars).with_tag_prefix("bars");
    match window {
        Some(w) => chart.x_window(w),
        None => chart,
    }
}

/// The line chart over the SAME categories — a numeric-x chart taking the
/// categorical axis, which is the swap this round makes possible.
///
/// The window reaches it as a pinned x-domain
/// ([`CategoryWindow::domain`]), the one path every axis kind is windowed
/// through, so the two plots cannot show different months.
fn line_chart(window: Option<CategoryWindow>, dense: bool) -> LineChart {
    // `Categories::positions` gives the x each slot sits at, so the binding
    // never casts an index into an axis coordinate itself.
    let points = months(dense)
        .positions()
        .zip(ATTAINMENT)
        .map(|(x, y)| DataPoint::new(x, y))
        .collect();
    let chart = LineChart::new(vec![
        Series::new("attainment %", points).with_color(Color::rgb(0xf9, 0xab, 0x00)),
    ])
    .x_category(MONTHS)
    .with_tag_prefix("trend");
    match window {
        Some(w) => {
            let (lo, hi) = w.domain();
            chart.with_x_domain(lo, hi)
        }
        None => chart,
    }
}

/// The caption: which categories are in view, or why the requested window was
/// not honoured.
///
/// The in-view branch asks the chart's own
/// [`BarChart::visible_categories`](pinion_chart::BarChart::visible_categories)
/// rather than restating the window, so a caption that disagreed with the plot
/// would be a bug in the crate and not in this string.
fn caption(state: &WindowState) -> String {
    let names = axis_names(state.dense);
    if state.unresolved {
        let detail = state
            .active_preset
            .and_then(|i| PRESETS[i].2)
            .and_then(|(f, t)| months(state.dense).window(f, t).err())
            .map_or_else(
                || "the requested range names no category".to_string(),
                |e| e.to_string(),
            );
        return format!(
            "{detail} — the range was NOT applied, so all {} months are shown. \
             the toolkit's setRange(string, string) returns void: this request would \
             have been ignored with nothing said.",
            names.len()
        );
    }
    let chart = bar_chart(state.window, state.dense);
    match chart.visible_categories(BAR_RECT, &ChartStyle::default()) {
        Some(v) => format!(
            "showing {}-{} — {} of {} categories on one axis, both charts",
            names[v.lo()],
            names[v.hi()],
            v.len(),
            names.len(),
        ),
        None => "no category is in view".to_string(),
    }
}

/// The window toolbar: the focusable Tab stop the window external hangs off,
/// with one chip per preset showing which is active.
fn toolbar(state: &WindowState, theme: &Theme) -> Scene {
    let mut chips: Vec<Scene> = Vec::with_capacity(PRESETS.len());
    for (i, (key, label, _)) in PRESETS.iter().enumerate() {
        let active = state.active_preset == Some(i);
        let swatch = Scene::Box(
            pinion_core::scene::BoxNode::new(
                Rect::default(),
                BoxStyle::filled(if active {
                    theme.resolve(ColorRole::Accent)
                } else {
                    theme.resolve(ColorRole::Outline)
                })
                .with_corner_radius(3),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(CHIP_FONT_PX, CHIP_FONT_PX))),
        );
        chips.push(Scene::Container(
            ContainerNode::new(vec![
                swatch,
                Scene::Text(TextNode::styled(
                    format!("{key}  {label}"),
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(CHIP_FONT_PX)
                        .with_fg(theme.resolve(ColorRole::OnSurface)),
                )),
            ])
            .with_tag(format!("preset.{i}"))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(6)
                    .with_size(Size::px(132, CHIP_FONT_PX + 8)),
            ),
        ));
    }
    Scene::Container(
        ContainerNode::new(chips)
            .with_tag(WINDOW_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8)
                    .with_focusable(true)
                    .with_absolute_position(WIN_W - 428, 18)
                    .with_size(Size::px(412, CHIP_FONT_PX + 12)),
            ),
    )
}

/// view-fn (§6.3): pure sync `WindowState -> Scene`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: WindowState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Revenue and attainment by month",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 20)),
    );

    let bars = bar_chart(state.window, state.dense).build(BAR_RECT, &chart_style(&theme, false));
    let trend = line_chart(state.window, state.dense).build(LINE_RECT, &chart_style(&theme, true));

    let caption = Scene::Text(
        TextNode::styled(
            caption(&state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CAPTION_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("caption".to_string())
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(18, WIN_H - 46)
                .with_size(Size::px(WIN_W - 36, 40)),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![bars, trend, title, toolbar(&state, &theme), caption])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

struct CategoryAxisView;

impl WidgetCore for CategoryAxisView {
    type State = WindowState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(CategoryWindowExternal::default())
    }

    fn tag() -> &'static str {
        WINDOW_TAG
    }

    fn read_state(scene: &Scene) -> WindowState {
        read_window(scene)
    }

    fn view(state: WindowState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-category-axis (R1545 §5.38 categorical axis)"
    }

    /// The keyboard leg of the same three actions RPC drives, so a human and
    /// an agent move one window rather than two.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: Modifiers,
    ) -> bool {
        if focused != Some(WINDOW_TAG) {
            return false;
        }
        let Some(intro) = scene
            .find_external_with_tag_mut(WINDOW_TAG)
            .and_then(|n| n.handle.introspect_mut())
        else {
            return false;
        };
        match key {
            "ArrowLeft" => intro.invoke("pan", IntrospectValue::Int(-1)).is_ok(),
            "ArrowRight" => intro.invoke("pan", IntrospectValue::Int(1)).is_ok(),
            "0" => intro.invoke("reset", IntrospectValue::Null).is_ok(),
            k => PRESETS
                .iter()
                .find(|(digit, _, _)| *digit == k)
                .is_some_and(|(_, _, request)| match request {
                    None => intro.invoke("reset", IntrospectValue::Null).is_ok(),
                    Some((from, to)) => intro
                        .invoke(
                            "range",
                            IntrospectValue::Json(serde_json::json!({"from": from, "to": to})),
                        )
                        .is_ok(),
                }),
        }
    }

    fn fmt_state_log(state: &WindowState) -> String {
        match (state.unresolved, state.window) {
            (true, _) => "unresolved".to_string(),
            (false, None) => "all categories".to_string(),
            (false, Some(w)) => format!("{}..{}", MONTHS[w.lo()], MONTHS[w.hi()]),
        }
    }
}

impl WidgetA11y for CategoryAxisView {
    /// The toolbar is one group naming the window it controls, so a screen
    /// reader is told which categories are in view — a thing the toolkit's
    /// category axis cannot report to anyone, sighted or not.
    fn access_node(state: &WindowState, _focused: Option<&str>) -> Vec<AccessNode> {
        let names = axis_names(state.dense);
        let name = match (state.unresolved, state.window) {
            (true, _) => format!(
                "category window: not applied, all {} categories shown",
                names.len()
            ),
            (false, None) => format!("category window: all {} categories", names.len()),
            (false, Some(w)) => format!(
                "category window: {} to {}, {} of {} categories",
                names[w.lo()],
                names[w.hi()],
                w.len(),
                names.len()
            ),
        };
        // The `focused` flag is stamped by the assembler (R1518), so this
        // binding does not compute one.
        let mut nodes = vec![AccessNode::new(WINDOW_TAG, AriaRole::Group).with_name(name)];
        // R1634 — and the CHART itself, one node per datum. The binding states
        // the two names and the crate builds the topology: what was a single
        // string to a screen reader is now a table a reader navigates, with a
        // row for every category the picture may have had no room to label.
        nodes.extend(chart_table_nodes(
            &bar_chart(state.window, state.dense).access_table(
                "Revenue by category",
                "Category",
                BAR_RECT,
                &ChartStyle::default(),
            ),
        ));
        nodes
    }
}

impl WidgetView for CategoryAxisView {
    type Renderer = HelloCategoryAxisRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<CategoryAxisView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn state(window: Option<CategoryWindow>, unresolved: bool) -> WindowState {
        WindowState {
            window,
            unresolved,
            active_preset: if unresolved { Some(2) } else { None },
            dense: false,
        }
    }

    fn render(window: Option<CategoryWindow>, unresolved: bool) -> Scene {
        Owner::new().run(|| view(state(window, unresolved), &Frame::new()))
    }

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        if let Scene::Container(c) = scene {
            return c.children.iter().find_map(|ch| find(ch, tag));
        }
        None
    }

    fn text_of(scene: &Scene, tag: &str) -> String {
        match find(scene, tag) {
            Some(Scene::Text(t)) => t.content.clone(),
            _ => panic!("no text node tagged {tag}"),
        }
    }

    /// ★ The whole round on screen: the two charts are drawn from one axis, so
    /// a window narrows BOTH. The bar chart drops the out-of-window bars; the
    /// line chart — a numeric-x chart taking the categorical axis — labels the
    /// same months and no others.
    #[test]
    fn r1545_one_window_narrows_both_charts() {
        let all = render(None, false);
        for i in 0..MONTHS.len() {
            assert!(find(&all, &format!("bars.bar.{i}")).is_some(), "bar {i}");
        }

        let windowed = render(Some(CategoryWindow::new(3, 5)), false);
        for i in 3..=5 {
            assert!(find(&windowed, &format!("bars.bar.{i}")).is_some());
        }
        for i in [0, 2, 6, 11] {
            assert!(
                find(&windowed, &format!("bars.bar.{i}")).is_none(),
                "bar {i} is outside the window"
            );
        }
        // The line chart's x labels come from the same category axis, and
        // there are exactly three of them.
        let labels: Vec<String> = (0..3)
            .map(|k| text_of(&windowed, &format!("trend.label.x.{k}")))
            .collect();
        assert_eq!(labels, ["Apr", "May", "Jun"]);
        assert!(find(&windowed, "trend.label.x.3").is_none());
    }

    /// ★ Past the toolkit: an unresolvable name is REPORTED and the charts
    /// stay whole, where `setRange` would have returned `void` and left the axis
    /// silently unwindowed — indistinguishable from a range that happened to
    /// be full.
    #[test]
    fn r1545_an_unresolvable_name_is_reported_not_swallowed() {
        let scene = render(None, true);
        let caption = text_of(&scene, "caption");
        assert!(caption.contains("Smarch"), "names the month: {caption}");
        assert!(
            caption.contains("NOT applied"),
            "and what it did: {caption}"
        );
        // The charts are unwindowed, not blank.
        assert!(find(&scene, "bars.bar.0").is_some());
        assert!(find(&scene, "bars.bar.11").is_some());

        // The counterfactual: a window that DOES resolve reports its extent
        // instead, so the caption is not simply always an apology.
        let good = text_of(&render(Some(CategoryWindow::new(3, 5)), false), "caption");
        assert!(good.contains("Apr-Jun"), "got {good}");
        assert!(good.contains("3 of 12"), "got {good}");
    }

    /// ★ The window external resolves the toolkit-shaped by-name call and
    /// hands back the failure the toolkit call cannot.
    #[test]
    fn r1545_the_window_external_answers_the_resolution() {
        let mut ext = CategoryWindowExternal::default();
        assert_eq!(ext.bounds(), (-1, -1), "unset is not 'windowed onto 0'");

        let ok = ext
            .invoke(
                "range",
                IntrospectValue::Json(serde_json::json!({"from": "Apr", "to": "Jun"})),
            )
            .expect("range is a known path");
        assert_eq!(ok, IntrospectValue::Text(String::new()), "resolved");
        assert_eq!(ext.bounds(), (3, 5));
        assert_eq!(ext.query("visible"), Some(IntrospectValue::Int(3)));

        let bad = ext
            .invoke(
                "range",
                IntrospectValue::Json(serde_json::json!({"from": "Smarch", "to": "Jun"})),
            )
            .expect("range is a known path");
        assert_eq!(
            bad,
            IntrospectValue::Text("no category named \"Smarch\"".to_string())
        );
        assert_eq!(ext.bounds(), (-1, -1), "an unresolved request is no window");
        assert_eq!(
            ext.query("visible"),
            Some(IntrospectValue::Int(12)),
            "so every category stays in view"
        );

        assert_eq!(
            ext.invoke("nope", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
        assert_eq!(
            ext.invoke("range", IntrospectValue::Int(3)),
            Err(InvokeError::TypeMismatch)
        );
    }

    /// ★ Panning is a window operation, so it moves the resolved indices and
    /// writes the NAMES back — the round-trip that proves the two forms agree.
    #[test]
    fn r1545_the_window_pans_by_whole_categories() {
        let mut ext = CategoryWindowExternal::default();
        ext.set_range("Apr", "Jun");
        assert_eq!(
            ext.invoke("pan", IntrospectValue::Int(2)),
            Ok(IntrospectValue::Bool(true))
        );
        assert_eq!(ext.bounds(), (5, 7));
        assert_eq!(
            ext.query("from"),
            Some(IntrospectValue::Text("Jun".to_string()))
        );
        assert_eq!(
            ext.query("to"),
            Some(IntrospectValue::Text("Aug".to_string()))
        );

        // It stops at the end of the list rather than running off it, and says
        // it did not move.
        assert_eq!(
            ext.invoke("pan", IntrospectValue::Int(99)),
            Ok(IntrospectValue::Bool(true))
        );
        assert_eq!(ext.bounds(), (9, 11));
        assert_eq!(
            ext.invoke("pan", IntrospectValue::Int(99)),
            Ok(IntrospectValue::Bool(false))
        );

        // An unwindowed axis has no window to pan; nor has an unresolved one.
        assert_eq!(
            ext.invoke("reset", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true))
        );
        assert_eq!(
            ext.invoke("pan", IntrospectValue::Int(1)),
            Ok(IntrospectValue::Bool(false))
        );
        ext.set_range("Smarch", "Jun");
        assert_eq!(
            ext.invoke("pan", IntrospectValue::Int(1)),
            Ok(IntrospectValue::Bool(false))
        );
    }

    /// ★ The window reaches assistive technology as a named group. The
    /// toolkit's category axis reports its range to nobody: `the toolkit's charting module` draws into a
    /// canvas scene whose axis labels carry no accessible relationship.
    #[test]
    fn r1545_a11y_names_the_categories_in_view() {
        let nodes =
            CategoryAxisView::access_node(&state(Some(CategoryWindow::new(3, 5)), false), None);
        let name = nodes[0].name.clone().unwrap_or_default();
        assert!(name.contains("Apr to Jun"), "got {name}");
        assert!(name.contains("3 of 12"), "got {name}");

        let all = CategoryAxisView::access_node(&state(None, false), None);
        assert!(
            all[0]
                .name
                .as_deref()
                .unwrap_or_default()
                .contains("all 12"),
            "the unwindowed axis says so too"
        );

        // ★ R1634 — and the CHART is here too, as a table rather than as a
        // sentence. The windowed tree is strictly smaller because the window is
        // the bound, and it still claims twelve rows because `aria-rowcount`
        // states the whole extent the window is a window onto.
        let windowed_rows = nodes.iter().filter(|n| n.role == AriaRole::Row).count();
        let all_rows = all.iter().filter(|n| n.role == AriaRole::Row).count();
        assert_eq!(all_rows, 13, "twelve months and a header row");
        assert_eq!(windowed_rows, 4, "three months and a header row");
        let table = nodes
            .iter()
            .find(|n| n.role == AriaRole::Table)
            .expect("the chart publishes a table");
        assert_eq!(
            table.row_count,
            Some(13),
            "★ the window presents three and DECLARES twelve"
        );
        let cells: Vec<&str> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Cell)
            .filter_map(|n| n.name.as_deref())
            .collect();
        assert_eq!(cells.len(), 3, "one datum per visible month");
        assert!(
            cells[0].starts_with("Revenue by category: "),
            "a cell names its series with its value: {cells:?}"
        );
    }
}
