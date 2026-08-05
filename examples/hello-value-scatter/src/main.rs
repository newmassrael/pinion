//! `hello-value-scatter` — R1438 §5.35: a scatter chart whose mark colour
//! encodes a THIRD data channel, and a legend that is therefore a colour bar.
//!
//! ## What this demonstrates
//!
//! Every earlier chart in this crate colours by *series identity*: a
//! categorical palette answers "which one is this". Here the colour answers
//! "how much is this" — each [`pinion_chart::DataPoint`] carries a
//! `value` (deviation from a target) that
//! [`ScatterChart::color_by`](pinion_chart::ScatterChart::color_by) maps
//! through a [`ColorScale`], so x, y and colour are three independent data
//! channels on one plane.
//!
//! Two consequences the binding exists to make visible:
//!
//! * **The legend changes kind.** A swatch row asserts "this colour names that
//!   series", which stops being true the moment colour means magnitude. So the
//!   chart emits a **colour bar** instead — a gradient strip plus domain ticks,
//!   tagged `chart.colorbar.strip` / `chart.colorbar.tick.{k}`.
//! * **A diverging encoding is not an evenly-split bar.** The deviation domain
//!   here is deliberately asymmetric (-6 .. +18). Under
//!   [`color_by_diverging`](pinion_chart::ScatterChart::color_by_diverging) the
//!   neutral (0.0, the target) must land on the ramp's centre colour, which
//!   puts it a QUARTER of the way along the bar — not the middle. The bar is
//!   built from the encoding's own stop placement, so it reports that.
//!
//! ## Falsifiable over the wire, not asserted in prose
//!
//! The oracle publishes `color_at "<value>"` — the encoding's colour — AND
//! `linear_color_at "<value>"`, the same value through a plain linear map.
//! A client reads both and sees them disagree at the neutral, which is the
//! whole reason the diverging map exists. `mark_color_at "i,j"` returns the
//! colour a specific painted mark received, so "the bar agrees with the marks"
//! is checkable rather than claimed. `intervene` on `encoding` swaps
//! sequential <-> diverging live, and the paint follows.
//!
//! ## Qt reference
//!
//! Qt reaches this through `QXYSeries::setPointConfiguration` (per-point
//! colour, index-keyed) and `Q3DTheme::ColorStyleRangeGradient` (colour by
//! value), with a colour-scale axis widget in the `QCustomPlot` ecosystem. Two
//! things here go past that: the magnitude rides ON the point (no parallel
//! array to fall out of step), and the bar is data — its gradient stops and
//! tick values are in the scene, so an AI verifies the encoding without
//! sampling a pixel (§2 #7). See `tools/demos/r1438_value_scatter.py`.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, ColorScale, DataPoint, ScatterChart, Series};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloValueScatterRenderer, HelloValueScatterRendererError);

const THEME_TAG: &str = "app";
/// The oracle's registration tag — addressed over RPC as `/external/<field>`.
const CHART_TAG: &str = "valuescatter";
/// Reactive-cache key for the shared encoding state.
const ENCODING_KEY: &str = "value-scatter-encoding";

/// The deviation domain, deliberately **asymmetric**: the shortfall wing is a
/// third the width of the overshoot wing, which is exactly when a linear map
/// misplaces the target and a diverging map does not.
const DOMAIN_LOW: f64 = -6.0;
const DOMAIN_HIGH: f64 = 18.0;
/// The target every sample is measured against.
const NEUTRAL: f64 = 0.0;

const WIN_W: u32 = 620;
const WIN_H: u32 = 420;
const CHART_X: u32 = 16;
const CHART_Y: u32 = 52;
const CHART_W: u32 = WIN_W - CHART_X * 2;
const CHART_H: u32 = 300;
const TITLE_FONT_PX: u32 = 16;
const STATUS_FONT_PX: u32 = 12;

// --- The data ---------------------------------------------------------------

/// One authored sample: latency, throughput, and deviation from target.
type Sample = (f64, f64, f64);

/// Three probes over a latency (x) / throughput (y) plane, each sample also
/// carrying its deviation from target as the third channel. The values are
/// fixed and span the domain ends and the neutral, so every part of the ramp
/// is exercised — including exact zeros, the marks whose colour must BE the
/// centre stop.
fn probes() -> Vec<Series> {
    let rows: [(&str, [Sample; 5]); 3] = [
        (
            "probe-a",
            [
                (1.0, 20.0, -6.0),
                (2.0, 34.0, -3.0),
                (3.0, 48.0, 0.0),
                (4.0, 61.0, 6.0),
                (5.0, 70.0, 18.0),
            ],
        ),
        (
            "probe-b",
            [
                (1.5, 26.0, -4.5),
                (2.5, 40.0, 0.0),
                (3.5, 52.0, 3.0),
                (4.5, 58.0, 9.0),
                (5.5, 66.0, 15.0),
            ],
        ),
        (
            "probe-c",
            [
                (1.2, 31.0, -1.5),
                (2.2, 45.0, 0.0),
                (3.2, 55.0, 4.5),
                (4.2, 63.0, 12.0),
                (5.2, 74.0, 16.5),
            ],
        ),
    ];
    rows.into_iter()
        .map(|(name, samples)| {
            Series::new(
                name,
                samples
                    .into_iter()
                    .map(|(x, y, v)| DataPoint::new(x, y).with_value(v))
                    .collect(),
            )
        })
        .collect()
}

/// A colour as the `#rrggbb` wire form the oracle publishes — the introspect
/// surface has no colour type, and hex is what a client compares exactly.
fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

// --- Shared encoding state ---------------------------------------------------

/// Which map the chart runs. Mutated by the oracle's `intervene`, read by the
/// view: one reactive holder shared by `Rc`, never two derived copies.
struct EncodingState {
    diverging: Signal<bool>,
}

/// `Owner::cache`-keyed accessor for the shared [`EncodingState`].
fn use_encoding_state() -> Rc<EncodingState> {
    Owner::current()
        .expect("use_encoding_state requires an active Owner scope")
        .cache(ENCODING_KEY, || EncodingState {
            diverging: Signal::new(true),
        })
}

/// The scale both encodings ride: a colour-blind-safe blue -> neutral ->
/// orange ramp built from the crate's Okabe-Ito constants.
fn scale() -> ColorScale {
    ColorScale::blue_orange()
}

/// The chart for the current encoding — the single place the two arms are
/// chosen, so the view, the a11y node and the oracle cannot drift apart.
fn chart(diverging: bool) -> ScatterChart {
    let base = ScatterChart::new(probes()).with_color_domain(DOMAIN_LOW, DOMAIN_HIGH);
    if diverging {
        base.color_by_diverging(scale(), NEUTRAL)
    } else {
        base.color_by(scale())
    }
}

/// The colour a value receives under the ACTIVE encoding.
fn encoded_color(value: f64, diverging: bool) -> Color {
    if diverging {
        scale().map_diverging(value, DOMAIN_LOW, NEUTRAL, DOMAIN_HIGH)
    } else {
        scale().map(value, DOMAIN_LOW, DOMAIN_HIGH)
    }
}

/// The colour a value would receive under a PLAIN LINEAR map — the refutation
/// channel. At the neutral this disagrees with the diverging encoding, and
/// that disagreement is the contract's evidence.
fn linear_color(value: f64) -> Color {
    scale().map(value, DOMAIN_LOW, DOMAIN_HIGH)
}

/// Where the bar seats the neutral: the neutral's fraction of the domain.
/// A quarter here, not a half — the number an evenly-split bar would get wrong.
fn neutral_offset() -> f64 {
    (NEUTRAL - DOMAIN_LOW) / (DOMAIN_HIGH - DOMAIN_LOW)
}

// --- The oracle (primary External) -------------------------------------------

/// Publishes the encoding, its domain, and both maps' answers, so a client
/// verifies the value->colour claim without a pixel.
///
struct ValueScatterOracle {
    diverging: Option<Rc<EncodingState>>,
}

/// `External` requires `Debug`, and the holder is a shared `Signal` rather than
/// a printable value — so print what a reader actually wants: which encoding is
/// live, and whether the shared state is attached at all.
impl core::fmt::Debug for ValueScatterOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ValueScatterOracle")
            .field("attached", &self.diverging.is_some())
            .field("diverging", &self.is_diverging())
            .finish()
    }
}

impl ValueScatterOracle {
    fn new() -> Self {
        Self { diverging: None }
    }

    /// Attach the SHARED reactive holder the view reads (never a second copy).
    fn attach_state(&mut self, state: Rc<EncodingState>) {
        self.diverging = Some(state);
    }

    fn is_diverging(&self) -> bool {
        self.diverging.as_ref().is_some_and(|s| s.diverging.get())
    }

    /// R1564 §5.15 (PINION-PR82) — the sentence for a point that exists but
    /// carries no reading. Two arms answer it; a missing sample is a state of
    /// the DATA, not a bad address, and the two used to be one value.
    const MISSING_SAMPLE: &str = "that point exists but holds no sample value";

    /// Parse a float argument. A non-string argument is a
    /// [`TypeMismatch`](InvokeError::TypeMismatch) (the same shape cannot
    /// succeed on retry); an unparseable string is
    /// [`Rejected`](InvokeError::Rejected) (another argument would work).
    fn parse_value(arg: &IntrospectValue) -> Result<f64, InvokeError> {
        match arg {
            IntrospectValue::Text(s) => s
                .trim()
                .parse::<f64>()
                .map_err(|_| InvokeError::rejected(format!("{s:?} is not a number"))),
            IntrospectValue::Float(f) => Ok(*f),
            _ => Err(InvokeError::TypeMismatch),
        }
    }

    /// Parse an `"i,j"` series/point coordinate into an existing sample.
    fn parse_point(arg: &IntrospectValue) -> Result<DataPoint, InvokeError> {
        let IntrospectValue::Text(s) = arg else {
            return Err(InvokeError::TypeMismatch);
        };
        let parsed = s.split_once(',').and_then(|(i, j)| {
            let i: usize = i.trim().parse().ok()?;
            let j: usize = j.trim().parse().ok()?;
            probes().get(i)?.points.get(j).copied()
        });
        parsed.ok_or_else(|| {
            InvokeError::rejected(format!(
                "{s:?} does not address a point (expected \"<probe>,<point>\" \
                 with both in range)"
            ))
        })
    }
}

impl ExternalIntrospect for ValueScatterOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // Which map is live; the one writable path.
                    SchemaField::new("encoding", "string"),
                    // The (asymmetric) colour domain and its neutral anchor.
                    SchemaField::new("domain_low", "float"),
                    SchemaField::new("domain_high", "float"),
                    SchemaField::new("neutral", "float"),
                    // Where the bar seats the neutral: a quarter, not a half.
                    SchemaField::new("neutral_offset", "float"),
                    // The ramp's centre stop, `#rrggbb` — what an on-target mark is.
                    SchemaField::new("neutral_hex", "string"),
                    // Per-value oracles, arg = the value.
                    SchemaField::new("color_at", "string"),
                    SchemaField::new("linear_color_at", "string"),
                    // Per-mark oracles, arg `"i,j"`.
                    SchemaField::new("value_at", "string"),
                    SchemaField::new("mark_color_at", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let diverging = self.is_diverging();
        match path {
            "encoding" => Some(IntrospectValue::Text(
                if diverging { "diverging" } else { "sequential" }.to_string(),
            )),
            "domain_low" => Some(IntrospectValue::Float(DOMAIN_LOW)),
            "domain_high" => Some(IntrospectValue::Float(DOMAIN_HIGH)),
            "neutral" => Some(IntrospectValue::Float(NEUTRAL)),
            "neutral_offset" => Some(IntrospectValue::Float(neutral_offset())),
            "neutral_hex" => Some(IntrospectValue::Text(hex(scale().sample(0.5)))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "encoding" => {
                let IntrospectValue::Text(mode) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let diverging = match mode.as_str() {
                    "diverging" => true,
                    "sequential" => false,
                    _ => {
                        return Err(InterveneError::out_of_range(format!(
                            "{mode:?} is not an encoding (expected \"diverging\" or \"sequential\")"
                        )));
                    }
                };
                if let Some(state) = self.diverging.as_ref() {
                    state.diverging.set(diverging);
                }
                Ok(())
            }
            "domain_low" | "domain_high" | "neutral" | "neutral_offset" | "neutral_hex" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let diverging = self.is_diverging();
        match path {
            "color_at" => Ok(IntrospectValue::Text(hex(encoded_color(
                Self::parse_value(&args)?,
                diverging,
            )))),
            "linear_color_at" => Ok(IntrospectValue::Text(hex(linear_color(Self::parse_value(
                &args,
            )?)))),
            "value_at" => {
                let point = Self::parse_point(&args)?;
                point
                    .value
                    .map(IntrospectValue::Float)
                    .ok_or_else(|| InvokeError::rejected(Self::MISSING_SAMPLE))
            }
            "mark_color_at" => {
                let point = Self::parse_point(&args)?;
                let value = point
                    .value
                    .ok_or_else(|| InvokeError::rejected(Self::MISSING_SAMPLE))?;
                Ok(IntrospectValue::Text(hex(encoded_color(value, diverging))))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl External for ValueScatterOracle {
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
}

// --- The view ----------------------------------------------------------------

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors the WidgetCore::view(&Frame) trait signature"
)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let diverging = use_encoding_state().diverging.get();

    let title = Scene::Text(TextNode::styled(
        "Deviation from target, encoded as mark colour",
        Rect::new(CHART_X, 16, CHART_W, TITLE_FONT_PX + 4),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let style = ChartStyle {
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        axis: theme.resolve(ColorRole::Outline),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        ..ChartStyle::default()
    };
    let plot = chart(diverging).build(Rect::new(CHART_X, CHART_Y, CHART_W, CHART_H), &style);

    let status = Scene::Text(TextNode::styled(
        if diverging {
            format!(
                "diverging about {NEUTRAL:.0} — the target sits {:.0}% along the bar",
                neutral_offset() * 100.0
            )
        } else {
            "sequential — colour ranks magnitude across the whole domain".to_string()
        },
        Rect::new(CHART_X, CHART_Y + CHART_H + 24, CHART_W, STATUS_FONT_PX + 4),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, plot, status])
            .with_tag(CHART_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct ValueScatterView;

impl WidgetCore for ValueScatterView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = ValueScatterOracle::new();
        oracle.attach_state(use_encoding_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        CHART_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-value-scatter (R1438 §5.35)"
    }
}

impl WidgetA11y for ValueScatterView {
    /// The chart is a WAI-ARIA `img` whose value text states the encoding and
    /// its domain — an AT user hears what the colours mean, which is the whole
    /// content of a value-encoded plot.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let diverging = use_encoding_state().diverging.get();
        let description = if diverging {
            format!(
                "Scatter of three probes; mark colour encodes deviation from \
                 target {NEUTRAL:.0}, diverging over {DOMAIN_LOW:.0} to {DOMAIN_HIGH:.0}"
            )
        } else {
            format!(
                "Scatter of three probes; mark colour encodes deviation \
                 magnitude over {DOMAIN_LOW:.0} to {DOMAIN_HIGH:.0}"
            )
        };
        vec![
            AccessNode::new(CHART_TAG, AriaRole::Group)
                .with_name("Deviation scatter")
                .with_value(AccessValue::Text(description)),
        ]
    }
}

impl WidgetView for ValueScatterView {
    type Renderer = HelloValueScatterRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ValueScatterView>();
}

#[cfg(test)]
mod tests {
    use super::{
        DOMAIN_HIGH, DOMAIN_LOW, NEUTRAL, chart, encoded_color, hex, linear_color, neutral_offset,
        probes, scale,
    };
    use pinion_chart::ChartStyle;
    use pinion_core::Scene;
    use pinion_core::scene::Rect;

    fn build(diverging: bool) -> Scene {
        chart(diverging).build(Rect::new(0, 0, 560, 300), &ChartStyle::default())
    }

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        match scene {
            Scene::Container(c) => c.children.iter().find_map(|ch| find(ch, tag)),
            _ => None,
        }
    }

    /// Every sample carries the third channel and the set spans both wings and
    /// the neutral — the precondition the rest of the assertions rest on.
    #[test]
    fn r1438_every_sample_carries_a_value_spanning_the_domain() {
        let series = probes();
        assert_eq!(series.len(), 3);
        let values: Vec<f64> = series
            .iter()
            .flat_map(|s| s.points.iter())
            .map(|p| p.value.expect("every sample carries the third channel"))
            .collect();
        assert_eq!(values.len(), 15);
        assert!(values.iter().any(|&v| v < NEUTRAL), "a shortfall sample");
        assert!(values.iter().any(|&v| v > NEUTRAL), "an overshoot sample");
        assert!(
            values.iter().filter(|&&v| v == NEUTRAL).count() >= 3,
            "on-target samples exist — the marks whose colour must be neutral"
        );
        let (lo, hi) = (
            values.iter().copied().fold(f64::INFINITY, f64::min),
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        assert!((lo - DOMAIN_LOW).abs() < 1e-9, "data reaches the low end");
        assert!((hi - DOMAIN_HIGH).abs() < 1e-9, "data reaches the high end");
    }

    /// The domain is asymmetric, so the neutral is NOT at the bar's midpoint —
    /// the fact that makes the diverging map observable rather than cosmetic.
    #[test]
    fn r1438_the_domain_is_asymmetric_by_construction() {
        let below = NEUTRAL - DOMAIN_LOW;
        let above = DOMAIN_HIGH - NEUTRAL;
        assert!(
            (above - below * 3.0).abs() < 1e-9,
            "the overshoot wing is three times the shortfall wing"
        );
        assert!(
            (neutral_offset() - 0.25).abs() < 1e-9,
            "the target sits a quarter along, not half"
        );
    }

    /// The two maps DISAGREE at the neutral — the refutation channel. If they
    /// agreed, the oracle's `linear_color_at` would prove nothing.
    #[test]
    fn r1438_the_linear_map_misplaces_the_target() {
        let encoded = encoded_color(NEUTRAL, true);
        assert_eq!(
            hex(encoded),
            hex(scale().sample(0.5)),
            "the diverging map puts the target on the ramp's centre stop"
        );
        assert_ne!(
            hex(encoded),
            hex(linear_color(NEUTRAL)),
            "a linear map does NOT — this disagreement is the evidence"
        );
    }

    /// The colour bar replaces the swatch row, and its neutral stop sits where
    /// the encoding puts it.
    #[test]
    fn r1438_the_bar_reports_the_active_encoding() {
        let diverging = build(true);
        assert!(find(&diverging, "chart.colorbar.strip").is_some());
        assert!(
            find(&diverging, "chart.legend.0.swatch").is_none(),
            "no categorical swatches while colour means magnitude"
        );
        let Some(Scene::Box(bar)) = find(&diverging, "chart.colorbar.strip") else {
            panic!("the strip is a box")
        };
        let stops = &bar.style.gradient.as_ref().expect("gradient").stops;
        assert!(
            (f64::from(stops[1].offset) - neutral_offset()).abs() < 1e-3,
            "the bar's neutral stop is the domain fraction, got {}",
            stops[1].offset
        );

        let sequential = build(false);
        let Some(Scene::Box(seq)) = find(&sequential, "chart.colorbar.strip") else {
            panic!("the strip is a box")
        };
        let seq_stops = &seq.style.gradient.as_ref().expect("gradient").stops;
        assert!(
            (seq_stops[1].offset - 0.5).abs() < 1e-3,
            "the sequential ramp is evenly spaced, got {}",
            seq_stops[1].offset
        );
    }

    /// Switching the encoding actually repaints different marks — the toggle
    /// is not decorative.
    #[test]
    fn r1438_switching_the_encoding_changes_the_marks() {
        let mark = |scene: &Scene, tag: &str| match find(scene, tag) {
            Some(Scene::Path(p)) => p.style.fill.expect("filled mark"),
            _ => panic!("mark {tag} missing"),
        };
        let diverging = build(true);
        let sequential = build(false);
        // The on-target mark is the one that must move: neutral under the
        // diverging map, a quarter up the ramp under the linear one.
        assert_ne!(
            hex(mark(&diverging, "chart.point.0.2")),
            hex(mark(&sequential, "chart.point.0.2")),
            "the on-target mark is coloured differently by the two encodings"
        );
        // The domain ends are shared by both maps, so they must NOT move —
        // otherwise the assertion above could pass for the wrong reason.
        assert_eq!(
            hex(mark(&diverging, "chart.point.0.0")),
            hex(mark(&sequential, "chart.point.0.0")),
            "the low end is the ramp start under both encodings"
        );
        assert_eq!(
            hex(mark(&diverging, "chart.point.0.4")),
            hex(mark(&sequential, "chart.point.0.4")),
            "the high end is the ramp end under both encodings"
        );
    }
}
