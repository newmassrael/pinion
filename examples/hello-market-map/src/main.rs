//! `hello-market-map` — R1439 §5.35: the **two-variable treemap**. A tile's
//! AREA encodes one measure and its COLOUR an independent second one.
//!
//! ## What this demonstrates
//!
//! `hello-treemap` (R1382) is area-only: the rectangles are sized by a weight
//! and coloured by category, so the colour carries no information a legend could
//! not have given away. That is only half of what the form was invented for.
//! Wattenberg's Map of the Market (1998) — the display every market map, disk
//! map and service map since has copied — is a two-variable chart: size for
//! magnitude, colour for a second, independent measure. The thing you are
//! looking for is the big rectangle that is ALSO the wrong colour.
//!
//! So each [`Tile`] here carries a weight (its area) AND a signed change (its
//! [`Tile::with_color_value`]), and
//! [`Treemap::color_by_diverging`] maps the second through a [`ColorScale`].
//! The data is authored so the two rankings DISAGREE — the largest sector has
//! the worst change and the best change belongs to the second-largest — because
//! a treemap whose colour tracked its area would be encoding one number twice.
//!
//! ## The vertical colour bar
//!
//! Colours that mean magnitude cannot be explained by a swatch row, so — as on
//! the scatter (R1438) — the legend becomes a colour bar. Here it is **vertical**,
//! standing in a gutter taken off the right, because a treemap tiles its whole
//! frame and has no axis band to lay a horizontal bar in.
//!
//! A vertical bar is not a rotated horizontal one, and this demo's sharpest
//! assertion is about exactly that. The value axis runs UP (high at the top, the
//! convention every colour-scale legend uses) while a vertical gradient paints
//! DOWN, so the stops must be mirrored. Get it wrong and nothing crashes — the
//! legend simply lies, telling the reader that the ramp's low colour means a
//! high value. The oracle publishes `strip_offset_at "<value>"`, the offset a
//! value occupies IN THE STRIP, so a client can check the mirror rather than
//! trust it.
//!
//! ## Falsifiable over the wire, not asserted in prose
//!
//! `color_at` (the live encoding) sits beside `linear_color_at` (the same value
//! through a plain linear map): they disagree at the neutral, which is the whole
//! reason the diverging map exists, and agree at the domain ends, which shows
//! the disagreement is not an artefact. `tile_color`/`tile_change`/`tile_weight`
//! answer per sector, so "colour ranks the change, not the weight" is checkable.
//! `intervene` on `encoding` cycles diverging -> sequential -> off, and the
//! tiles re-pack as the bar's gutter appears and vanishes.
//!
//! ## Qt reference
//!
//! Qt ships no treemap at all — neither Qt Charts nor Qt Graphs has an
//! area-encoded part-of-whole form, so a two-variable market map is custom
//! `QGraphicsScene` work there, and a `QCPColorScale`-style legend is a pixel
//! widget. Here the bar's gradient stops and tick values ride in the scene as
//! data, so an AI verifies the encoding — including the mirror — without
//! sampling a pixel (§2 #7). See `tools/demos/r1439_market_map.py`.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{CategoricalPalette, ChartStyle, ColorScale, Tile, Treemap};
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
vello_renderer_impl!(HelloMarketMapRenderer, HelloMarketMapRendererError);

const THEME_TAG: &str = "app";
/// The oracle's registration tag — addressed over RPC as `/external/<field>`.
const MAP_TAG: &str = "marketmap";
/// Reactive-cache key for the shared encoding state.
const ENCODING_KEY: &str = "market-map-encoding";

/// The change domain, deliberately **asymmetric** — the down wing is a third of
/// the up wing, which is both what real change data looks like and exactly when
/// a linear map misplaces the neutral and a diverging map does not.
const DOMAIN_LOW: f64 = -3.2;
const DOMAIN_HIGH: f64 = 9.6;
/// The anchor: no change.
const NEUTRAL: f64 = 0.0;

const WIN_W: u32 = 640;
const WIN_H: u32 = 470;
const MAP_X: u32 = 16;
const MAP_Y: u32 = 56;
const MAP_W: u32 = WIN_W - MAP_X * 2;
const MAP_H: u32 = 340;
const TITLE_FONT_PX: u32 = 16;
const STATUS_FONT_PX: u32 = 12;

// --- The data ---------------------------------------------------------------

/// One sector: name, weight (the AREA channel), change (the COLOUR channel).
type Sector = (&'static str, f64, f64);

/// Eight sectors whose two rankings DISAGREE by construction — the property
/// that makes the display two-variable rather than one number drawn twice.
///
/// The largest sector has the worst change and the second largest the best, so
/// "big" and "green" are independent. Two sectors share a change of exactly
/// `0.0` at very different weights, which pins the converse: equal measure,
/// equal colour, whatever the area.
const SECTORS: [Sector; 8] = [
    ("Core Compute", 320.0, -3.2),
    ("Data Platform", 210.0, 9.6),
    ("Networking", 150.0, 0.0),
    ("Storage", 120.0, 2.4),
    ("Security", 90.0, -1.6),
    ("Observability", 60.0, 4.8),
    ("Edge", 40.0, 0.0),
    ("Tooling", 25.0, 6.4),
];

fn tiles() -> Vec<Tile> {
    SECTORS
        .iter()
        .map(|&(name, weight, change)| Tile::new(name, weight).with_color_value(change))
        .collect()
}

/// The original index of `label`, which is also its categorical palette slot.
fn sector_index(label: &str) -> Option<usize> {
    SECTORS.iter().position(|&(name, _, _)| name == label)
}

/// A colour as the `#rrggbb` wire form the oracle publishes — the introspect
/// surface has no colour type, and hex is what a client compares exactly.
fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// Labels joined by `,` in descending order of `key` — the two rankings the
/// demo compares. Ties keep the authored order, so the comparison is total.
fn order_by(key: impl Fn(&Sector) -> f64) -> String {
    let mut ranked: Vec<&Sector> = SECTORS.iter().collect();
    ranked.sort_by(|a, b| key(b).total_cmp(&key(a)));
    ranked
        .iter()
        .map(|&&(name, _, _)| name)
        .collect::<Vec<_>>()
        .join(",")
}

// --- Shared encoding state ---------------------------------------------------

/// Which map the treemap runs. `Off` is a real third state, not the absence of
/// one: it is the R1382 area-only treemap, and switching to it must restore the
/// categorical colours AND give the bar's gutter back to the tiles.
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
/// shared by `Rc`, never two derived copies.
struct EncodingState {
    encoding: Signal<Encoding>,
}

/// `Owner::cache`-keyed accessor for the shared [`EncodingState`].
fn use_encoding_state() -> Rc<EncodingState> {
    Owner::current()
        .expect("use_encoding_state requires an active Owner scope")
        .cache(ENCODING_KEY, || EncodingState {
            encoding: Signal::new(Encoding::Diverging),
        })
}

/// The scale both live encodings ride: a colour-blind-safe blue -> neutral ->
/// orange ramp built from the crate's Okabe-Ito constants.
fn scale() -> ColorScale {
    ColorScale::blue_orange()
}

/// The treemap for the current encoding — the single place the three arms are
/// chosen, so the view, the a11y node and the oracle cannot drift apart.
fn map(encoding: Encoding) -> Treemap {
    let base = Treemap::new(tiles());
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

/// The colour a change receives under the ACTIVE encoding, or `None` when the
/// encoding is off (colour then means category, not magnitude).
fn encoded_color(change: f64, encoding: Encoding) -> Option<Color> {
    match encoding {
        Encoding::Diverging => {
            Some(scale().map_diverging(change, DOMAIN_LOW, NEUTRAL, DOMAIN_HIGH))
        }
        Encoding::Sequential => Some(scale().map(change, DOMAIN_LOW, DOMAIN_HIGH)),
        Encoding::Off => None,
    }
}

/// The colour a change would receive under a PLAIN LINEAR map — the refutation
/// channel. At the neutral this disagrees with the diverging encoding, and that
/// disagreement is the contract's evidence.
fn linear_color(change: f64) -> Color {
    scale().map(change, DOMAIN_LOW, DOMAIN_HIGH)
}

/// Where the encoding seats a change along the VALUE axis, `0.0` at the domain
/// low. A quarter for the neutral here, not a half.
fn value_offset(change: f64) -> f64 {
    ((change - DOMAIN_LOW) / (DOMAIN_HIGH - DOMAIN_LOW)).clamp(0.0, 1.0)
}

/// Where that change sits in the drawn STRIP — the mirrored offset, because the
/// value axis runs up and a vertical gradient paints down. This is the number a
/// client compares against the scene's gradient stops; if the bar were built
/// without the mirror the two would disagree.
fn strip_offset(change: f64) -> f64 {
    1.0 - value_offset(change)
}

// --- The oracle (primary External) -------------------------------------------

/// Publishes the encoding, its domain, both maps' answers, and each sector's
/// two measures, so a client verifies the area/colour claims without a pixel.
struct MarketMapOracle {
    state: Option<Rc<EncodingState>>,
}

/// `External` requires `Debug`, and the holder is a shared `Signal` rather than
/// a printable value — so print what a reader actually wants: which encoding is
/// live, and whether the shared state is attached at all.
impl core::fmt::Debug for MarketMapOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MarketMapOracle")
            .field("attached", &self.state.is_some())
            .field("encoding", &self.encoding().name())
            .finish()
    }
}

impl MarketMapOracle {
    fn new() -> Self {
        Self { state: None }
    }

    /// Attach the SHARED reactive holder the view reads (never a second copy).
    fn attach_state(&mut self, state: Rc<EncodingState>) {
        self.state = Some(state);
    }

    fn encoding(&self) -> Encoding {
        self.state
            .as_ref()
            .map_or(Encoding::Diverging, |s| s.encoding.get())
    }

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

    /// Resolve a sector by label into `(index, weight, change)`.
    fn parse_sector(arg: &IntrospectValue) -> Result<(usize, f64, f64), InvokeError> {
        let IntrospectValue::Text(label) = arg else {
            return Err(InvokeError::TypeMismatch);
        };
        let idx = sector_index(label.trim()).ok_or_else(|| {
            InvokeError::rejected(format!("no sector named {:?} on this map", label.trim()))
        })?;
        let (_, weight, change) = SECTORS[idx];
        Ok((idx, weight, change))
    }
}

impl ExternalIntrospect for MarketMapOracle {
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
                    // Where the neutral sits on the VALUE axis, and in the drawn
                    // strip — the mirror, published as a number.
                    SchemaField::new("neutral_offset", "float"),
                    SchemaField::new("neutral_strip_offset", "float"),
                    // The ramp's centre stop, `#rrggbb`.
                    SchemaField::new("neutral_hex", "string"),
                    // The two rankings, comma-joined: they must differ.
                    SchemaField::new("area_order", "string"),
                    SchemaField::new("change_order", "string"),
                    // Per-value oracles, arg = the change.
                    SchemaField::action("color_at", "string"),
                    SchemaField::action("linear_color_at", "string"),
                    SchemaField::action("strip_offset_at", "string"),
                    // Per-sector oracles, arg = the label.
                    SchemaField::action("tile_weight", "string"),
                    SchemaField::action("tile_change", "string"),
                    SchemaField::action("tile_color", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "encoding" => Some(IntrospectValue::Text(self.encoding().name().to_string())),
            "domain_low" => Some(IntrospectValue::Float(DOMAIN_LOW)),
            "domain_high" => Some(IntrospectValue::Float(DOMAIN_HIGH)),
            "neutral" => Some(IntrospectValue::Float(NEUTRAL)),
            "neutral_offset" => Some(IntrospectValue::Float(value_offset(NEUTRAL))),
            "neutral_strip_offset" => Some(IntrospectValue::Float(strip_offset(NEUTRAL))),
            "neutral_hex" => Some(IntrospectValue::Text(hex(scale().sample(0.5)))),
            "area_order" => Some(IntrospectValue::Text(order_by(|&(_, w, _)| w))),
            "change_order" => Some(IntrospectValue::Text(order_by(|&(_, _, c)| c))),
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
            "domain_low"
            | "domain_high"
            | "neutral"
            | "neutral_offset"
            | "neutral_strip_offset"
            | "neutral_hex"
            | "area_order"
            | "change_order" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let encoding = self.encoding();
        match path {
            "color_at" => {
                let value = Self::parse_value(&args)?;
                // With the encoding off there is no value->colour map at all,
                // and saying so is more honest than returning a colour the
                // chart is not using.
                encoded_color(value, encoding)
                    .map(|c| IntrospectValue::Text(hex(c)))
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "the {encoding:?} encoding this chart is using assigns \
                             no colour to that value"
                        ))
                    })
            }
            "linear_color_at" => Ok(IntrospectValue::Text(hex(linear_color(Self::parse_value(
                &args,
            )?)))),
            "strip_offset_at" => Ok(IntrospectValue::Float(strip_offset(Self::parse_value(
                &args,
            )?))),
            "tile_weight" => Ok(IntrospectValue::Float(Self::parse_sector(&args)?.1)),
            "tile_change" => Ok(IntrospectValue::Float(Self::parse_sector(&args)?.2)),
            "tile_color" => {
                let (idx, _, change) = Self::parse_sector(&args)?;
                let color = encoded_color(change, encoding)
                    .unwrap_or_else(|| CategoricalPalette::default().color(idx));
                Ok(IntrospectValue::Text(hex(color)))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl External for MarketMapOracle {
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

/// The status line: what the colours currently mean.
fn status_text(encoding: Encoding) -> String {
    match encoding {
        Encoding::Diverging => format!(
            "area = weight, colour = change diverging about {NEUTRAL:.0} — \
             the neutral sits {:.0}% up the bar",
            value_offset(NEUTRAL) * 100.0
        ),
        Encoding::Sequential => {
            "area = weight, colour = change ranked across the whole domain".to_string()
        }
        Encoding::Off => "area = weight, colour = category (no second measure)".to_string(),
    }
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors the WidgetCore::view(&Frame) trait signature"
)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let encoding = use_encoding_state().encoding.get();

    let title = Scene::Text(TextNode::styled(
        "Sector map — size is weight, colour is daily change",
        Rect::new(MAP_X, 18, MAP_W, TITLE_FONT_PX + 4),
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
    let plot = map(encoding).build(Rect::new(MAP_X, MAP_Y, MAP_W, MAP_H), &style);

    let status = Scene::Text(TextNode::styled(
        status_text(encoding),
        Rect::new(MAP_X, MAP_Y + MAP_H + 24, MAP_W, STATUS_FONT_PX + 4),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, plot, status])
            .with_tag(MAP_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct MarketMapView;

impl WidgetCore for MarketMapView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = MarketMapOracle::new();
        oracle.attach_state(use_encoding_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        MAP_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-market-map (R1439 §5.35)"
    }
}

impl WidgetA11y for MarketMapView {
    /// The map is a WAI-ARIA `img` whose value text names BOTH encodings. A
    /// two-variable chart that described only its areas would leave an AT user
    /// with half the display — the half a plain list already gives them.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let encoding = use_encoding_state().encoding.get();
        let description = match encoding {
            Encoding::Diverging => format!(
                "Treemap of {} sectors; tile area encodes weight and tile colour \
                 encodes daily change, diverging about {NEUTRAL:.0} over \
                 {DOMAIN_LOW:.1} to {DOMAIN_HIGH:.1}",
                SECTORS.len()
            ),
            Encoding::Sequential => format!(
                "Treemap of {} sectors; tile area encodes weight and tile colour \
                 ranks daily change over {DOMAIN_LOW:.1} to {DOMAIN_HIGH:.1}",
                SECTORS.len()
            ),
            Encoding::Off => format!(
                "Treemap of {} sectors; tile area encodes weight, colour is \
                 categorical",
                SECTORS.len()
            ),
        };
        vec![
            AccessNode::new(MAP_TAG, AriaRole::Group)
                .with_name("Sector map")
                .with_value(AccessValue::Text(description)),
        ]
    }
}

impl WidgetView for MarketMapView {
    type Renderer = HelloMarketMapRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<MarketMapView>();
}

#[cfg(test)]
mod tests {
    use super::{
        DOMAIN_HIGH, DOMAIN_LOW, Encoding, NEUTRAL, SECTORS, encoded_color, hex, linear_color, map,
        order_by, scale, strip_offset, tiles, value_offset,
    };
    use pinion_chart::{ChartStyle, color_value_bounds};
    use pinion_core::Scene;
    use pinion_core::scene::Rect;

    fn build(encoding: Encoding) -> Scene {
        map(encoding).build(Rect::new(0, 0, 600, 340), &ChartStyle::default())
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

    /// ★ The two rankings disagree — without this the display would be one
    /// number drawn twice and every assertion below would be vacuous.
    #[test]
    fn r1439_area_rank_and_colour_rank_disagree() {
        let by_area = order_by(|&(_, w, _)| w);
        let by_change = order_by(|&(_, _, c)| c);
        assert_ne!(by_area, by_change, "the two measures rank sectors alike");
        assert!(
            by_area.starts_with("Core Compute"),
            "the largest sector by weight, got {by_area}"
        );
        assert!(
            by_change.starts_with("Data Platform"),
            "the best sector by change, got {by_change}"
        );
        // The largest is also the WORST — the case a one-variable map hides.
        assert!(
            by_change.ends_with("Core Compute"),
            "the largest sector has the worst change, got {by_change}"
        );
    }

    /// The authored data spans the domain exactly, and two sectors of very
    /// different weight share a change — the equal-measure/equal-colour probe.
    #[test]
    fn r1439_the_data_spans_the_domain_and_repeats_a_measure() {
        assert_eq!(
            color_value_bounds(&tiles()),
            Some((DOMAIN_LOW, DOMAIN_HIGH))
        );
        let on_target: Vec<&str> = SECTORS
            .iter()
            .filter(|&&(_, _, c)| c == NEUTRAL)
            .map(|&(n, _, _)| n)
            .collect();
        assert_eq!(on_target.len(), 2, "two sectors sit exactly on target");
        let weights: Vec<f64> = SECTORS
            .iter()
            .filter(|&&(_, _, c)| c == NEUTRAL)
            .map(|&(_, w, _)| w)
            .collect();
        assert!(
            weights[0] > weights[1] * 2.0,
            "and at very different weights, so equal colour cannot come from equal area"
        );
    }

    /// The domain is asymmetric, so the neutral is NOT at the bar's midpoint.
    #[test]
    fn r1439_the_domain_is_asymmetric_by_construction() {
        let below = NEUTRAL - DOMAIN_LOW;
        let above = DOMAIN_HIGH - NEUTRAL;
        assert!(
            (above - below * 3.0).abs() < 1e-9,
            "the up wing is three times the down wing"
        );
        assert!((value_offset(NEUTRAL) - 0.25).abs() < 1e-9, "a quarter up");
        // ★ And in the drawn strip that is three quarters DOWN, because the
        // value axis runs up while a vertical gradient paints down.
        assert!((strip_offset(NEUTRAL) - 0.75).abs() < 1e-9, "mirrored");
    }

    /// The two maps DISAGREE at the neutral — the refutation channel.
    #[test]
    fn r1439_the_linear_map_misplaces_the_neutral() {
        let encoded = encoded_color(NEUTRAL, Encoding::Diverging).expect("a live encoding");
        assert_eq!(
            hex(encoded),
            hex(scale().sample(0.5)),
            "the diverging map puts no-change on the ramp's centre stop"
        );
        assert_ne!(
            hex(encoded),
            hex(linear_color(NEUTRAL)),
            "a linear map does NOT — this disagreement is the evidence"
        );
        // The ends are shared, so the disagreement is not a trivial artefact.
        for end in [DOMAIN_LOW, DOMAIN_HIGH] {
            assert_eq!(
                hex(encoded_color(end, Encoding::Diverging).expect("a live encoding")),
                hex(linear_color(end)),
                "both maps agree at {end}"
            );
        }
    }

    /// ★ The strip's gradient stops match the MIRRORED offsets the oracle
    /// publishes. Building the bar without the mirror would paint the ramp
    /// upside down while every value-space number stayed right.
    #[test]
    fn r1439_the_strip_stops_match_the_published_mirror() {
        let scene = build(Encoding::Diverging);
        let Some(Scene::Box(strip)) = find(&scene, "chart.colorbar.strip") else {
            panic!("the encoded map has a colour bar")
        };
        let stops = &strip.style.gradient.as_ref().expect("gradient").stops;
        assert_eq!(stops.len(), 3, "blue_orange has three stops");
        assert!(
            (f64::from(stops[1].offset) - strip_offset(NEUTRAL)).abs() < 1e-3,
            "the neutral stop sits at the mirrored offset {}, got {}",
            strip_offset(NEUTRAL),
            stops[1].offset
        );
        // The ramp's HIGH colour paints at the top of the strip (offset 0).
        assert_eq!(
            stops[0].color,
            *scale().stops().last().expect("a high stop"),
            "high at the top"
        );
        assert_eq!(stops[2].color, scale().stops()[0], "low at the bottom");
    }

    /// Turning the encoding off restores the categorical treemap AND gives the
    /// bar's gutter back to the tiles — the third state is not a no-op.
    #[test]
    fn r1439_switching_the_encoding_off_restores_the_area_only_map() {
        let tile_right = |scene: &Scene| {
            (0..SECTORS.len())
                .filter_map(|i| find(scene, &format!("chart.tile.{i}")))
                .filter_map(|s| match s {
                    Scene::Box(b) => Some(b.rect.x + b.rect.w),
                    _ => None,
                })
                .max()
                .expect("tiles are drawn")
        };
        let encoded = build(Encoding::Diverging);
        let off = build(Encoding::Off);
        assert!(find(&encoded, "chart.colorbar.strip").is_some());
        assert!(
            find(&off, "chart.colorbar.strip").is_none(),
            "no bar without an encoding"
        );
        assert!(
            tile_right(&off) > tile_right(&encoded),
            "the tiles reclaim the gutter ({} > {})",
            tile_right(&off),
            tile_right(&encoded)
        );

        // And the colours change kind: on-target sectors share a colour under
        // the encoding, but take their own palette slots with it off.
        let fill = |scene: &Scene, i: usize| match find(scene, &format!("chart.tile.{i}")) {
            Some(Scene::Box(b)) => b.style.fill,
            _ => panic!("tile {i} missing"),
        };
        // Draw order is by descending area: 2 = Networking (0.0), 6 = Edge (0.0).
        assert_eq!(
            fill(&encoded, 2),
            fill(&encoded, 6),
            "equal change -> equal colour while encoded"
        );
        assert_ne!(
            fill(&off, 2),
            fill(&off, 6),
            "but distinct categories once the encoding is off"
        );
    }
}
