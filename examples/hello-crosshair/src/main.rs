// R1406 §5.35 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-crosshair` — R1406 §5.35 — a [`pinion_chart::LineChart`] inspector
//! **crosshair that follows the bare hover** (no button held), the **2nd
//! consumer** of the R1405 [`External::wants_hover_move`] seam.
//!
//! ## What this demonstrates
//!
//! A plain hover (no press) forwards only `Enter` / `Leave` to a widget — not
//! the intra-widget position. Before R1405 a chart could only get a
//! *continuous* pointer position under **pointer capture** (a button-held
//! drag), so the scrub inspector in `hello-chart` is a capture drag that fakes
//! the crosshair with a `SliderExternal`. R1405 added
//! [`External::wants_hover_move`]: a widget that must know *where inside it* the
//! pointer is on a **free** hover returns `true`, and the router then forwards
//! each hover move as `pointer_move(x_rel, y_rel)`.
//!
//! [`CrosshairExternal`] is the chart-side consumer of that seam. It opts into
//! hover-move but **not** capture ([`External::wants_pointer_capture`] stays
//! `false`): the crosshair tracks the cursor with no press at all — the
//! canonical dataviz "hover to read a value" affordance. It stores only the x
//! fraction; the view feeds it to [`LineChart::inspect`], which maps the
//! fraction through its own margins + domain to the nearest data point and
//! draws the crosshair, per-series marker dots, and a value tooltip. Hovering
//! off the plot fires `Leave`, which clears the fraction so the crosshair
//! disappears.
//!
//! The R1405 hover-move seam is the *only* framework machinery this needs — the
//! [`HyperlinkOracle`](../hello_hyperlink) (a `TextGrid` cell highlighter) and
//! this chart crosshair are structurally unrelated consumers, so a second
//! consumer with **zero** framework change is the proof the seam generalises
//! ([[abstraction-needs-second-consumer]]).
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! The crosshair overlay is retained scene data —
//! `chart.inspect.crosshair` / `.marker.{i}` / `.header` / `.value.{i}` — so
//! `scene/snapshot` reports the readout with no pixel. The external exposes the
//! interaction state a snapshot cannot phrase as one field: `x_frac`
//! (`0.0..=1.0`, or Null off the plot) and the derived `has_crosshair`, driven
//! no-pixel via `scene/intervene /external/x_frac`. See
//! `tools/demos/r1406_crosshair.py`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, ReadRefusal, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::input::PointerReading;
use pinion_core::scene::{ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCrosshairRenderer, HelloCrosshairRendererError);

const WIN_W: u32 = 720;
const WIN_H: u32 = 420;
const THEME_TAG: &str = "app";

/// The plot's paint tag **and** the primary [`CrosshairExternal`]'s
/// registration tag — addressed over RPC as `/external/<field>`. A
/// transparent, pointer-opaque surface over the plot carries it, so a hover
/// anywhere on the plot forwards its position to the external.
const PLOT_TAG: &str = "plot";

/// The human-readable inspector line at the window's foot.
const READOUT_TAG: &str = "crosshair.readout";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 13;

/// Window-absolute plot region. The chart must be handed its final geometry
/// before layout resolves against it, so the constant stays (the `pinion-chart`
/// coordinate contract). The crosshair surface covers exactly this rect, so a
/// hover's `x_rel` fraction `0.0..=1.0` is the inspect fraction across it.
const CHART_RECT: Rect = Rect::new(16, 48, WIN_W - 32, WIN_H - 104);

/// Deterministic sample data — two throughput series over 12 buckets.
#[allow(
    clippy::cast_precision_loss,
    reason = "bucket index (0..12) -> f64 x-coordinate is exact"
)]
fn sample_series() -> Vec<Series> {
    let requests = [
        820.0, 910.0, 1150.0, 1400.0, 1320.0, 1600.0, 2100.0, 2400.0, 2200.0, 1900.0, 2600.0,
        3100.0,
    ];
    let errors = [
        12.0, 8.0, 20.0, 40.0, 15.0, 60.0, 90.0, 30.0, 25.0, 70.0, 45.0, 20.0,
    ];
    let mk = |name: &str, ys: &[f64]| {
        Series::new(
            name,
            ys.iter()
                .enumerate()
                .map(|(i, &y)| DataPoint::new(i as f64, y))
                .collect(),
        )
    };
    vec![mk("requests", &requests), mk("errors", &errors)]
}

/// Resolve the theme into a [`ChartStyle`].
fn chart_style(theme: &pinion_core::Theme) -> ChartStyle {
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

/// The inspector readout string for the current hover fraction, or the idle
/// prompt when nothing is hovered — the SSOT both the status line and the a11y
/// value read.
fn readout_text(x_frac: Option<f32>, style: &ChartStyle) -> String {
    LineChart::new(sample_series())
        .inspect(x_frac)
        .inspect_readout(CHART_RECT, style)
        .unwrap_or_else(|| "hover the plot to inspect a value".to_owned())
}

/// view-fn (§6.3): pure sync mapping. `x_frac` is the bare-hover fraction
/// across [`CHART_RECT`] (the primary [`CrosshairExternal`]'s state), or `None`
/// when the pointer is off the plot.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference; the signature mirrors it"
)]
fn view(x_frac: Option<f32>, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);
    let style = chart_style(&theme);

    let chart = LineChart::new(sample_series())
        .filled(true)
        .inspect(x_frac)
        .build(CHART_RECT, &style);

    let title = Scene::Text(
        TextNode::styled(
            "Throughput (pkt/s) — hover the plot to inspect (no click)",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 16)),
    );

    // Transparent capture surface over the plot — the `plot` primary tag. On
    // top so a hover anywhere over the plot resolves to it; transparent so the
    // chart shows through, pointer-opaque so the hover hit-test lands here. Not
    // focusable: the external opts into hover-move, not focus, so the position
    // arrives on a bare hover. R1417 capture_surface lift.
    let plot_surface = capture_surface(PLOT_TAG, CHART_RECT, false);

    let status = Scene::Text(
        TextNode::styled(
            readout_text(x_frac, &style),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_tag(READOUT_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(18, WIN_H - 26)),
    );

    Scene::Container(
        ContainerNode::new(vec![chart, plot_surface, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// Read the bare-hover fraction from the primary [`CrosshairExternal`] in the
/// state scene; `None` (no crosshair) when absent or off the plot. `f32` is
/// `Copy`, so the widget state stays `Copy`.
fn read_crosshair(scene: &Scene) -> Option<f32> {
    let intro = scene
        .find_external_with_tag(PLOT_TAG)
        .and_then(|n| n.handle.introspect())?;
    match intro.query("x_frac") {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "an inspect fraction 0.0..=1.0 loses no meaningful precision as f32"
        )]
        Ok(IntrospectValue::Float(f)) => Some(f as f32),
        _ => None,
    }
}

// --- The crosshair external (primary) --------------------------------------

/// The bare-hover interaction state: the pointer's x fraction across the plot,
/// or `None` off the plot. The 2nd consumer of [`External::wants_hover_move`]:
/// it opts into hover-move (position on a free hover) but not capture (no
/// press), so the crosshair tracks the cursor without a button.
#[derive(Debug, Clone, Default)]
struct CrosshairExternal {
    /// The hover x fraction `0.0..=1.0` across the plot rect, or `None` when
    /// the pointer is off the plot (no crosshair).
    x_frac: Option<f32>,
}

impl CrosshairExternal {
    fn new() -> Self {
        Self::default()
    }
}

impl External for CrosshairExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Opt into hover-move so the pointer's position is forwarded on a plain
    /// hover, not only under a press (R1405) — the crosshair tracks the cursor
    /// with no button. This is the whole point of the demo.
    fn wants_hover_move(&self) -> bool {
        true
    }

    /// Each hover move delivers a `[0, 1]` fraction across the plot rect: store
    /// it as the inspect x. `wants_pointer_capture` is left at its default
    /// `false` — a crosshair reads on hover, it does not capture a drag.
    fn pointer_move(&mut self, at: PointerReading) {
        self.x_frac = Some(at.u().clamp(0.0, 1.0));
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for CrosshairExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The hover x fraction 0.0..=1.0 across the plot, or Null off
                    // the plot. Writable (the AI-first no-pixel drive).
                    SchemaField::new("x_frac", "float"),
                    // Whether a crosshair is currently drawn (x_frac.is_some()).
                    SchemaField::new("has_crosshair", "bool"),
                    // The router's pointer boundary events (Leave / Cancel clear
                    // the crosshair when the pointer leaves the plot).
                    SchemaField::action("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "x_frac" => Ok(self
                .x_frac
                .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into()))),
            "has_crosshair" => Ok(IntrospectValue::Bool(self.x_frac.is_some())),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // AI-first, no-pixel hover: set the crosshair fraction (or Null to
            // clear). Rejects a fraction outside 0.0..=1.0.
            "x_frac" => match value {
                IntrospectValue::Null => {
                    self.x_frac = None;
                    Ok(())
                }
                IntrospectValue::Float(f) => {
                    if (0.0..=1.0).contains(&f) {
                        #[allow(
                            clippy::cast_possible_truncation,
                            reason = "a fraction 0.0..=1.0 loses no meaningful precision as f32"
                        )]
                        {
                            self.x_frac = Some(f as f32);
                        }
                        Ok(())
                    } else {
                        Err(InterveneError::out_of_range(format!(
                            "{f} is not a fraction of the plot width (0.0..=1.0)"
                        )))
                    }
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "has_crosshair" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, pinion_core::external::InvokeError> {
        match path {
            // The router pointer boundary. Leaving the plot (or a cancel)
            // clears the crosshair — it is a hover affordance, so it lives
            // only while the pointer is over the plot.
            "send" => {
                if let IntrospectValue::Text(ref name) = args {
                    if matches!(name.as_str(), "PointerLeave" | "PointerCancel") {
                        self.x_frac = None;
                    }
                }
                Ok(IntrospectValue::Null)
            }
            _ => Err(pinion_core::external::InvokeError::UnknownPath),
        }
    }
}

// --- The binding -----------------------------------------------------------

struct CrosshairView;

impl WidgetCore for CrosshairView {
    type State = Option<f32>;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(CrosshairExternal::new())
    }

    fn tag() -> &'static str {
        PLOT_TAG
    }

    fn read_state(scene: &Scene) -> Option<f32> {
        read_crosshair(scene)
    }

    fn view(state: Option<f32>, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-crosshair (R1406 §5.35 bare-hover chart crosshair)"
    }

    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &Option<f32>) -> String {
        state.map_or_else(
            || "no crosshair".to_owned(),
            |f| format!("crosshair at x_frac {f:.2}"),
        )
    }
}

impl WidgetA11y for CrosshairView {
    fn access_node(state: &Option<f32>, _focused: Option<&str>) -> Vec<AccessNode> {
        let readout = readout_text(*state, &ChartStyle::default());
        vec![
            AccessNode::new(PLOT_TAG, AriaRole::Group)
                .with_name("Chart crosshair")
                .with_value(AccessValue::Text(readout)),
        ]
    }
}

impl WidgetView for CrosshairView {
    type Renderer = HelloCrosshairRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<CrosshairView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_core::test_fixtures::assert_out_of_range_saying;

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

    fn rendered(x_frac: Option<f32>) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(x_frac, &Frame::new()))
    }

    #[test]
    fn no_hover_draws_no_crosshair_and_prompts() {
        let scene = rendered(None);
        assert!(
            find(&scene, "chart.inspect.crosshair").is_none(),
            "no crosshair off-hover"
        );
        let Some(Scene::Text(t)) = find(&scene, READOUT_TAG) else {
            panic!("readout line")
        };
        assert!(
            t.content.contains("hover the plot"),
            "idle prompt shown, got {:?}",
            t.content
        );
    }

    #[test]
    fn hovering_draws_a_crosshair_with_a_marker_per_series() {
        let scene = rendered(Some(0.5));
        assert!(
            find(&scene, "chart.inspect.crosshair").is_some(),
            "crosshair drawn on hover"
        );
        assert!(
            find(&scene, "chart.inspect.marker.0").is_some(),
            "series 0 marker"
        );
        assert!(
            find(&scene, "chart.inspect.marker.1").is_some(),
            "series 1 marker"
        );
        assert!(
            find(&scene, "chart.inspect.marker.2").is_none(),
            "only two series"
        );
    }

    #[test]
    fn the_readout_names_the_nearest_x_and_each_series() {
        // Far left snaps to the first bucket (x = 0), far right to the last.
        let left = readout_text(Some(0.0), &ChartStyle::default());
        assert!(left.starts_with("x = 0"), "left snaps to x=0, got {left:?}");
        assert!(left.contains("requests"), "names the requests series");
        assert!(left.contains("errors"), "names the errors series");
        let right = readout_text(Some(1.0), &ChartStyle::default());
        assert!(
            right.starts_with("x = 11"),
            "right snaps to x=11, got {right:?}"
        );
    }

    #[test]
    fn the_external_opts_into_hover_not_capture() {
        let ext = CrosshairExternal::new();
        assert!(
            ext.wants_hover_move(),
            "opts into hover-move (the R1405 seam)"
        );
        assert!(
            !ext.wants_pointer_capture(),
            "does NOT capture — a crosshair reads on bare hover"
        );
    }

    #[test]
    fn a_hover_move_sets_the_fraction_and_a_leave_clears_it() {
        let mut ext = CrosshairExternal::new();
        assert_eq!(ext.query("x_frac"), Ok(IntrospectValue::Null), "boot: none");
        ext.pointer_move(PointerReading::over_unit((0.42, 0.5)));
        assert_eq!(
            ext.query("x_frac"),
            Ok(IntrospectValue::Float(0.42_f32.into())),
            "hover move stored the x fraction"
        );
        assert_eq!(ext.query("has_crosshair"), Ok(IntrospectValue::Bool(true)));
        // A leave (the router's boundary send) clears the crosshair.
        ext.invoke("send", IntrospectValue::Text("PointerLeave".to_owned()))
            .expect("send is infallible");
        assert_eq!(
            ext.query("x_frac"),
            Ok(IntrospectValue::Null),
            "leaving the plot clears the crosshair"
        );
    }

    #[test]
    fn out_of_range_intervene_is_rejected() {
        let mut ext = CrosshairExternal::new();
        assert_out_of_range_saying(
            &ext.intervene("x_frac", IntrospectValue::Float(1.5)),
            "1.5 is not a fraction of the plot width",
        );
        assert_eq!(
            ext.intervene("has_crosshair", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
            "the derived flag is read-only"
        );
        assert!(
            ext.intervene("nope", IntrospectValue::Null).is_err(),
            "unknown path errors"
        );
        // A valid fraction is accepted and rendered.
        ext.intervene("x_frac", IntrospectValue::Float(0.5))
            .expect("in-range fraction accepted");
        assert_eq!(ext.query("has_crosshair"), Ok(IntrospectValue::Bool(true)));
    }
}
