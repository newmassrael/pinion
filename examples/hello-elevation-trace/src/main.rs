//! `hello-elevation-trace` — R1440 §5.35: a trace whose COLOUR encodes a second
//! measure, and the two different geometries that takes.
//!
//! ## What this demonstrates
//!
//! An elevation profile is the canonical two-channel trace: `y` is height, and
//! the colour is **slope** — signed, so the natural ramp is diverging about
//! level ground. Height and slope are related but independent readings: the
//! highest point of a climb is exactly where the slope has already gone to zero,
//! so a colour that tracked `y` would say something different from one that
//! tracks the measure. The data here is authored so that shows: the summit is
//! flat, and the steepest segment is partway up.
//!
//! ## One encoding, two geometries — because the primitives differ
//!
//! This is what makes the line chart's version of the encoding structurally
//! different from the scatter's (R1438) or the treemap's (R1439), where a mark is
//! one shape with one colour:
//!
//! * The **line** is a polyline, and a stroke takes a flat colour — pinion's
//!   `PathStyle` gradient replaces the FILL, not the stroke. So the trace is
//!   emitted as one stroked path PER SEGMENT (`chart.series.0.seg.{k}`), each
//!   coloured at the mean of its two endpoints' slopes.
//! * The **area** is a filled path, so it takes a real horizontal GRADIENT whose
//!   stops sit at the samples' own x positions — genuinely continuous, and exact
//!   rather than approximate, since colour is piecewise-linear between stops just
//!   as the encoding is between samples.
//!
//! Toggling the fill on and off (`intervene` on `filled`) switches which
//! mechanism is on screen, and both are readable as scene data.
//!
//! ## Falsifiable over the wire, not asserted in prose
//!
//! The oracle publishes `slope_at "<i>"` (the measure at sample `i`),
//! `segment_color_at "<k>"` (the colour segment `k` should receive, computed as
//! the endpoint mean) and `endpoint_color_at "<i>"`. A client checks that a
//! segment is the MEAN rather than either endpoint — the two disagree everywhere
//! here, which is what makes the assertion worth making. `x_fraction_at "<i>"`
//! gives each sample's position along the mark, so the area's gradient stops are
//! checkable against the data instead of against even spacing.
//!
//! ## the toolkit reference
//!
//! line series carries one pen; the toolkit Charts has no per-vertex or
//! per-segment line colour and no gradient-along-a-series fill driven by a
//! third channel, so a heat-line there is custom painter work. Here both forms
//! are retained scene nodes whose colours and gradient stops an AI reads out
//! of `scene/snapshot` (§2 #7). See `tools/demos/r1440_elevation_trace.py`.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, ColorScale, DataPoint, LineChart, Series};
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
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(
    HelloElevationTraceRenderer,
    HelloElevationTraceRendererError
);

const THEME_TAG: &str = "app";
/// The oracle's registration tag — addressed over RPC as `/external/<field>`.
const TRACE_TAG: &str = "elevationtrace";
/// Reactive-cache key for the shared view state.
const VIEW_KEY: &str = "elevation-trace-view";

/// The slope domain, deliberately **asymmetric**: the descent is gentler than
/// the climb, which is what a real profile looks like and exactly when a linear
/// map misplaces level ground.
const DOMAIN_LOW: f64 = -4.0;
const DOMAIN_HIGH: f64 = 12.0;
/// The anchor: level ground.
const NEUTRAL: f64 = 0.0;

const WIN_W: u32 = 660;
const WIN_H: u32 = 460;
const CHART_X: u32 = 16;
const CHART_Y: u32 = 56;
const CHART_W: u32 = WIN_W - CHART_X * 2;
const CHART_H: u32 = 320;
const TITLE_FONT_PX: u32 = 16;
const STATUS_FONT_PX: u32 = 12;

// --- The data ---------------------------------------------------------------

/// One sample: distance (x), elevation (y), slope (the colour channel).
type Sample = (f64, f64, f64);

/// A profile whose HEIGHT and SLOPE readings deliberately diverge: the summit
/// (sample 5) is the highest point AND the flattest, and the steepest segment is
/// partway up. A colour tracking `y` would peak where this one goes neutral.
/// The x samples are deliberately UNEVEN — waypoints on a real route are not
/// equidistant, and evenly-spaced x would make the area gradient's "stops sit at
/// the samples' own x" claim untestable, since even spacing would then give the
/// same answer.
const PROFILE: [Sample; 9] = [
    (0.0, 120.0, 2.0),
    (0.4, 148.0, 6.0),
    (1.2, 206.0, 12.0),
    (2.6, 268.0, 9.0),
    (4.0, 310.0, 4.0),
    (5.0, 324.0, 0.0),
    (6.4, 296.0, -3.0),
    (7.2, 250.0, -4.0),
    (8.0, 228.0, -1.5),
];

fn profile() -> Vec<Series> {
    vec![Series::new(
        "ridge",
        PROFILE
            .iter()
            .map(|&(x, y, slope)| DataPoint::new(x, y).with_value(slope))
            .collect(),
    )]
}

/// A colour as the `#rrggbb` wire form the oracle publishes.
fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

// --- Shared view state -------------------------------------------------------

/// Which map the trace runs. `Off` is a real third state: the R1354 categorical
/// trace, one flat polyline with a swatch legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Encoding {
    Diverging,
    Sequential,
    Off,
}

impl Encoding {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "diverging" => Some(Self::Diverging),
            "sequential" => Some(Self::Sequential),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Diverging => "diverging",
            Self::Sequential => "sequential",
            Self::Off => "off",
        }
    }
}

/// Mutated by the oracle's `intervene`, read by the view: one reactive holder
/// shared by `Rc`, never two derived copies. `filled` is here too because it
/// selects WHICH geometry carries the encoding.
struct ViewState {
    encoding: Signal<Encoding>,
    filled: Signal<bool>,
}

/// `Owner::cache`-keyed accessor for the shared [`ViewState`].
fn use_view_state() -> Rc<ViewState> {
    Owner::current()
        .expect("use_view_state requires an active Owner scope")
        .cache(VIEW_KEY, || ViewState {
            encoding: Signal::new(Encoding::Diverging),
            filled: Signal::new(true),
        })
}

/// The scale both live encodings ride: a colour-blind-safe blue → neutral →
/// orange ramp built from the crate's Okabe-Ito constants.
fn scale() -> ColorScale {
    ColorScale::blue_orange()
}

/// The chart for the current state — the single place the arms are chosen, so
/// the view, the a11y node and the oracle cannot drift apart.
fn chart(encoding: Encoding, filled: bool) -> LineChart {
    let base = LineChart::new(profile()).filled(filled);
    match encoding {
        Encoding::Diverging => base
            .with_color_domain(DOMAIN_LOW, DOMAIN_HIGH)
            .color_by_diverging(scale(), NEUTRAL),
        Encoding::Sequential => base
            .with_color_domain(DOMAIN_LOW, DOMAIN_HIGH)
            .color_by(scale()),
        Encoding::Off => base,
    }
}

/// The colour a slope receives under the ACTIVE encoding, or `None` when off.
fn encoded_color(slope: f64, encoding: Encoding) -> Option<Color> {
    match encoding {
        Encoding::Diverging => Some(scale().map_diverging(slope, DOMAIN_LOW, NEUTRAL, DOMAIN_HIGH)),
        Encoding::Sequential => Some(scale().map(slope, DOMAIN_LOW, DOMAIN_HIGH)),
        Encoding::Off => None,
    }
}

/// The colour segment `k` should receive: the LINEAR-LIGHT midpoint of its two
/// endpoints' colours, which is the chart's documented rule.
///
/// Published beside [`encoded_color`] so a client can see the mean differ from
/// either endpoint — if a segment were coloured by its start value instead, this
/// is the number that would disagree.
fn segment_color(k: usize, encoding: Encoding) -> Option<Color> {
    let a = encoded_color(PROFILE.get(k)?.2, encoding)?;
    let b = encoded_color(PROFILE.get(k + 1)?.2, encoding)?;
    Some(a.lerp(b, 0.5))
}

/// Where sample `i` sits along the mark, `0.0..=1.0` in x.
///
/// The area gradient's stops are checkable against these rather than against
/// even spacing, and [`PROFILE`]'s x samples are uneven precisely so the two
/// answers differ — an evenly-sampled profile would let a stop-per-index
/// implementation pass a test that meant nothing.
fn x_fraction(i: usize) -> Option<f64> {
    let x = PROFILE.get(i)?.0;
    let first = PROFILE.first()?.0;
    let last = PROFILE.last()?.0;
    Some((x - first) / (last - first))
}

// --- The oracle (primary External) -------------------------------------------

/// Publishes the encoding, the slope domain, and per-sample / per-segment
/// colours, so a client verifies the two geometries without a pixel.
struct TraceOracle {
    state: Option<Rc<ViewState>>,
}

/// `External` requires `Debug`, and the holder is a pair of shared `Signal`s
/// rather than printable values — so print what a reader actually wants.
impl core::fmt::Debug for TraceOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TraceOracle")
            .field("attached", &self.state.is_some())
            .field("encoding", &self.encoding().name())
            .field("filled", &self.filled())
            .finish()
    }
}

impl TraceOracle {
    fn new() -> Self {
        Self { state: None }
    }

    /// Attach the SHARED reactive holder the view reads (never a second copy).
    fn attach_state(&mut self, state: Rc<ViewState>) {
        self.state = Some(state);
    }

    fn encoding(&self) -> Encoding {
        self.state
            .as_ref()
            .map_or(Encoding::Diverging, |s| s.encoding.get())
    }

    fn filled(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.filled.get())
    }

    /// R1564 §5.15 (PINION-PR82) — the sentence for "this chart's current
    /// encoding assigns no colour here". Two arms answer it, and the fact an
    /// operator needs is that the chart is not USING that visual channel —
    /// saying so is why the arm refuses instead of returning an unused colour.
    const UNENCODED: &str = "this trace's current encoding assigns no colour to that segment";

    /// Parse an index argument. A non-string argument is a
    /// [`TypeMismatch`](InvokeError::TypeMismatch) (the same shape cannot
    /// succeed on retry); an unparseable or out-of-range one is
    /// [`Rejected`](InvokeError::Rejected).
    fn parse_index(arg: &IntrospectValue) -> Result<usize, InvokeError> {
        let text = match arg {
            IntrospectValue::Text(s) => s.trim().to_string(),
            _ => return Err(InvokeError::TypeMismatch),
        };
        text.parse::<usize>()
            .map_err(|_| InvokeError::rejected(format!("{text:?} is not a sample index")))
    }
}

impl ExternalIntrospect for TraceOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The two writable paths.
                    SchemaField::new("encoding", "string"),
                    SchemaField::new("filled", "bool"),
                    // The (asymmetric) slope domain and its neutral anchor.
                    SchemaField::new("domain_low", "float"),
                    SchemaField::new("domain_high", "float"),
                    SchemaField::new("neutral", "float"),
                    SchemaField::new("neutral_offset", "float"),
                    SchemaField::new("neutral_hex", "string"),
                    // Shape of the trace: samples and therefore segments.
                    SchemaField::new("sample_count", "float"),
                    SchemaField::new("segment_count", "float"),
                    // The index of the highest sample and of the steepest one —
                    // they differ, which is the two-channel claim.
                    SchemaField::new("peak_index", "float"),
                    SchemaField::new("steepest_index", "float"),
                    // Per-sample oracles, arg = the index.
                    SchemaField::action("slope_at", "string"),
                    SchemaField::action("elevation_at", "string"),
                    SchemaField::action("endpoint_color_at", "string"),
                    SchemaField::action("x_fraction_at", "string"),
                    // Per-segment oracle, arg = the segment index.
                    SchemaField::action("segment_color_at", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let index_of = |key: fn(&Sample) -> f64| {
            let mut best = 0;
            for (i, s) in PROFILE.iter().enumerate() {
                if key(s) > key(&PROFILE[best]) {
                    best = i;
                }
            }
            f64::from(u32::try_from(best).unwrap_or(u32::MAX))
        };
        match path {
            "encoding" => Some(IntrospectValue::Text(self.encoding().name().to_string())),
            "filled" => Some(IntrospectValue::Bool(self.filled())),
            "domain_low" => Some(IntrospectValue::Float(DOMAIN_LOW)),
            "domain_high" => Some(IntrospectValue::Float(DOMAIN_HIGH)),
            "neutral" => Some(IntrospectValue::Float(NEUTRAL)),
            "neutral_offset" => Some(IntrospectValue::Float(
                (NEUTRAL - DOMAIN_LOW) / (DOMAIN_HIGH - DOMAIN_LOW),
            )),
            "neutral_hex" => Some(IntrospectValue::Text(hex(scale().sample(0.5)))),
            "sample_count" => Some(IntrospectValue::Float(f64::from(
                u32::try_from(PROFILE.len()).unwrap_or(u32::MAX),
            ))),
            "segment_count" => Some(IntrospectValue::Float(f64::from(
                u32::try_from(PROFILE.len().saturating_sub(1)).unwrap_or(u32::MAX),
            ))),
            "peak_index" => Some(IntrospectValue::Float(index_of(|s| s.1))),
            "steepest_index" => Some(IntrospectValue::Float(index_of(|s| s.2))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "encoding" => {
                let IntrospectValue::Text(mode) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let encoding = Encoding::parse(mode.as_str()).ok_or_else(|| {
                    InterveneError::out_of_range(format!("{mode:?} is not an encoding"))
                })?;
                if let Some(state) = self.state.as_ref() {
                    state.encoding.set(encoding);
                }
                Ok(())
            }
            "filled" => {
                let IntrospectValue::Bool(filled) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                if let Some(state) = self.state.as_ref() {
                    state.filled.set(filled);
                }
                Ok(())
            }
            "domain_low" | "domain_high" | "neutral" | "neutral_offset" | "neutral_hex"
            | "sample_count" | "segment_count" | "peak_index" | "steepest_index" => {
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
        let encoding = self.encoding();
        let sample = |i: usize| {
            PROFILE.get(i).copied().ok_or_else(|| {
                InvokeError::rejected(format!(
                    "no sample {i} in this profile (it has {})",
                    PROFILE.len()
                ))
            })
        };
        match path {
            "slope_at" => Ok(IntrospectValue::Float(sample(Self::parse_index(&args)?)?.2)),
            "elevation_at" => Ok(IntrospectValue::Float(sample(Self::parse_index(&args)?)?.1)),
            "endpoint_color_at" => {
                let slope = sample(Self::parse_index(&args)?)?.2;
                // With the encoding off there is no value→colour map at all, and
                // saying so is more honest than returning an unused colour.
                encoded_color(slope, encoding)
                    .map(|c| IntrospectValue::Text(hex(c)))
                    .ok_or_else(|| InvokeError::rejected(Self::UNENCODED))
            }
            "segment_color_at" => {
                let k = Self::parse_index(&args)?;
                segment_color(k, encoding)
                    .map(|c| IntrospectValue::Text(hex(c)))
                    .ok_or_else(|| InvokeError::rejected(Self::UNENCODED))
            }
            "x_fraction_at" => {
                let i = Self::parse_index(&args)?;
                x_fraction(i).map(IntrospectValue::Float).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "x_fraction_at: no sample {i} in this profile (it has {})",
                        PROFILE.len()
                    ))
                })
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl External for TraceOracle {
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

/// The status line: which geometry currently carries the encoding.
fn status_text(encoding: Encoding, filled: bool) -> String {
    match (encoding, filled) {
        (Encoding::Off, _) => "colour = series identity (one flat polyline)".to_string(),
        (_, true) => format!(
            "{} slope — segments stroke the line, a gradient fills the area",
            encoding.name()
        ),
        (_, false) => format!(
            "{} slope — segments stroke the line, no area to gradient",
            encoding.name()
        ),
    }
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors the WidgetCore::view(&Frame) trait signature"
)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_view_state();
    let (encoding, filled) = (state.encoding.get(), state.filled.get());

    let title = Scene::Text(TextNode::styled(
        "Ridge profile — height is y, slope is colour",
        Rect::new(CHART_X, 18, CHART_W, TITLE_FONT_PX + 4),
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
    let plot = chart(encoding, filled).build(Rect::new(CHART_X, CHART_Y, CHART_W, CHART_H), &style);

    let status = Scene::Text(TextNode::styled(
        status_text(encoding, filled),
        Rect::new(CHART_X, CHART_Y + CHART_H + 24, CHART_W, STATUS_FONT_PX + 4),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, plot, status])
            .with_tag(TRACE_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct TraceView;

impl WidgetCore for TraceView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = TraceOracle::new();
        oracle.attach_state(use_view_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        TRACE_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-elevation-trace (R1440 §5.35)"
    }
}

impl WidgetA11y for TraceView {
    /// The chart is a WAI-ARIA group whose value text names BOTH channels. A
    /// two-channel trace described by its heights alone would leave an AT user
    /// with the half a plain list already gives them.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let encoding = use_view_state().encoding.get();
        let description = match encoding {
            Encoding::Off => format!(
                "Elevation profile of {} samples; colour is categorical",
                PROFILE.len()
            ),
            Encoding::Diverging => format!(
                "Elevation profile of {} samples; line colour encodes slope, \
                 diverging about level over {DOMAIN_LOW:.0} to {DOMAIN_HIGH:.0}",
                PROFILE.len()
            ),
            Encoding::Sequential => format!(
                "Elevation profile of {} samples; line colour ranks slope over \
                 {DOMAIN_LOW:.0} to {DOMAIN_HIGH:.0}",
                PROFILE.len()
            ),
        };
        vec![
            AccessNode::new(TRACE_TAG, AriaRole::Group)
                .with_name("Ridge profile")
                .with_value(AccessValue::Text(description)),
        ]
    }
}

impl WidgetView for TraceView {
    type Renderer = HelloElevationTraceRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TraceView>();
}

#[cfg(test)]
mod tests {
    use super::{
        DOMAIN_HIGH, DOMAIN_LOW, Encoding, NEUTRAL, PROFILE, chart, encoded_color, hex, profile,
        scale, segment_color, x_fraction,
    };
    use pinion_chart::ChartStyle;
    use pinion_core::Scene;
    use pinion_core::scene::Rect;

    fn build(encoding: Encoding, filled: bool) -> Scene {
        chart(encoding, filled).build(Rect::new(0, 0, 600, 320), &ChartStyle::default())
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

    /// ★ Height and slope rank the samples differently — without this the
    /// second channel would be redundant and every assertion below vacuous.
    #[test]
    fn r1440_the_peak_is_not_the_steepest_point() {
        let arg_max = |key: fn(&(f64, f64, f64)) -> f64| {
            (0..PROFILE.len())
                .max_by(|&a, &b| key(&PROFILE[a]).total_cmp(&key(&PROFILE[b])))
                .expect("a non-empty profile")
        };
        let peak = arg_max(|s| s.1);
        let steepest = arg_max(|s| s.2);
        assert_ne!(peak, steepest, "height and slope must not co-rank");
        assert!(
            (PROFILE[peak].2 - NEUTRAL).abs() < f64::EPSILON,
            "the summit is FLAT"
        );
        assert!(
            steepest < peak,
            "and the steepest stretch is partway up, not at the top"
        );
        // Every sample carries the channel and the set spans the domain.
        let slopes: Vec<f64> = PROFILE.iter().map(|s| s.2).collect();
        assert!(slopes.iter().any(|&v| v < NEUTRAL), "a descent");
        assert!(slopes.iter().any(|&v| v > NEUTRAL), "a climb");
        let hi = slopes.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let lo = slopes.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            (hi - DOMAIN_HIGH).abs() < f64::EPSILON,
            "reaches the high end"
        );
        assert!((lo - DOMAIN_LOW).abs() < f64::EPSILON, "and the low one");
    }

    /// The trace is emitted as one stroked path per segment, and the flat
    /// polyline is not also drawn.
    #[test]
    fn r1440_the_encoded_line_is_one_path_per_segment() {
        let scene = build(Encoding::Diverging, false);
        for k in 0..PROFILE.len() - 1 {
            assert!(
                find(&scene, &format!("chart.series.0.seg.{k}")).is_some(),
                "segment {k} is drawn"
            );
        }
        assert!(
            find(&scene, &format!("chart.series.0.seg.{}", PROFILE.len() - 1)).is_none(),
            "and there is no segment past the last pair"
        );
        assert!(find(&scene, "chart.series.0").is_none(), "no flat polyline");
    }

    /// ★ A segment carries the MEAN of its endpoints, which differs from either
    /// endpoint everywhere on this profile.
    #[test]
    fn r1440_a_segment_is_the_mean_not_an_endpoint() {
        let scene = build(Encoding::Diverging, false);
        let stroke = |k: usize| match find(&scene, &format!("chart.series.0.seg.{k}")) {
            Some(Scene::Path(p)) => p.style.stroke.expect("stroked").color,
            _ => panic!("segment {k} missing"),
        };
        for (k, sample) in PROFILE.iter().enumerate().take(PROFILE.len() - 1) {
            let expected = segment_color(k, Encoding::Diverging).expect("a live encoding");
            assert_eq!(hex(stroke(k)), hex(expected), "segment {k} is the mean");
            let start = encoded_color(sample.2, Encoding::Diverging).expect("live");
            assert_ne!(
                hex(stroke(k)),
                hex(start),
                "★ segment {k} is NOT its start value's colour"
            );
        }
    }

    /// The area takes a real gradient with one stop per sample, at the samples'
    /// own x fractions.
    #[test]
    fn r1440_the_filled_area_is_a_gradient_along_x() {
        let scene = build(Encoding::Diverging, true);
        let Some(Scene::Path(area)) = find(&scene, "chart.area.0") else {
            panic!("the area is a path")
        };
        let stops = &area.style.gradient.as_ref().expect("a gradient").stops;
        assert_eq!(stops.len(), PROFILE.len(), "one stop per sample");
        for (i, stop) in stops.iter().enumerate() {
            let want = x_fraction(i).expect("a fraction");
            // The stop is placed against the path's box, which is a px wider
            // than the vertex span, so allow that one px of slack.
            assert!(
                (f64::from(stop.offset) - want).abs() < 0.01,
                "stop {i} at {} should be near {want}",
                stop.offset
            );
        }
        // Turning the fill off removes the mark entirely (not just its gradient).
        assert!(
            find(&build(Encoding::Diverging, false), "chart.area.0").is_none(),
            "no area when unfilled"
        );
    }

    /// The legend changes kind with the encoding, and reverts with it.
    #[test]
    fn r1440_the_legend_follows_the_encoding() {
        let encoded = build(Encoding::Sequential, true);
        assert!(find(&encoded, "chart.colorbar.strip").is_some(), "a bar");
        assert!(
            find(&encoded, "chart.legend.0.swatch").is_none(),
            "no swatch while colour means magnitude"
        );
        let off = build(Encoding::Off, true);
        assert!(find(&off, "chart.colorbar.strip").is_none(), "no bar");
        assert!(
            find(&off, "chart.legend.0.swatch").is_some(),
            "the swatch row returns"
        );
        assert!(
            find(&off, "chart.series.0").is_some(),
            "and so does the single polyline"
        );
    }

    /// The two maps disagree at the neutral — the refutation channel.
    #[test]
    fn r1440_the_diverging_map_puts_level_ground_on_the_centre_stop() {
        let diverging = encoded_color(NEUTRAL, Encoding::Diverging).expect("live");
        assert_eq!(hex(diverging), hex(scale().sample(0.5)));
        let sequential = encoded_color(NEUTRAL, Encoding::Sequential).expect("live");
        assert_ne!(
            hex(diverging),
            hex(sequential),
            "a linear map does NOT — the asymmetric domain is why"
        );
        // Both agree at the domain ends, so the disagreement is not an artefact.
        for end in [DOMAIN_LOW, DOMAIN_HIGH] {
            assert_eq!(
                hex(encoded_color(end, Encoding::Diverging).expect("live")),
                hex(encoded_color(end, Encoding::Sequential).expect("live")),
            );
        }
        assert_eq!(profile().len(), 1, "one series carries the whole trace");
    }
}
