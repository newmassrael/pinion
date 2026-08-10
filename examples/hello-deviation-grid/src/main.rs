// R1436 §5.35 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-deviation-grid` — R1436 §5.35 — a **diverging** colour scale over a
//! deviation-from-baseline matrix, the second consumer of
//! [`pinion_chart::ColorScale`] (after `hello-heatmap`'s sequential ramp).
//!
//! ## Why a diverging scale is a different thing
//!
//! A sequential ramp answers "how big"; a diverging ramp answers "how far, and
//! WHICH WAY, from a meaningful zero" — a service running under vs over its
//! latency target, a correlation above vs below none, a forecast error. The
//! defining property is that **zero must land on the neutral colour exactly**,
//! and the trap is that real deviation data is almost never symmetric: this
//! demo's matrix runs `-14..=+32`, so mapping it linearly would paint the
//! neutral at about a third of the way up the ramp and a genuinely positive
//! deviation would read as "on target". [`ColorScale::map_diverging`] normalises
//! each side on its own width, which fixes exactly that.
//!
//! The demo asserts the difference rather than describing it: the oracle
//! exposes `neutral_hex` (the ramp's centre stop), `color_at "r,c"` for every
//! cell, and `linear_color_at "r,c"` — the SAME value through the linear
//! [`ColorScale::map`] — so a client can read the zero cell both ways and see
//! that only the diverging map is neutral there.
//!
//! ## Legibility is data, not decoration
//!
//! Every cell prints its value in [`pinion_chart::readable_ink`], the higher
//! WCAG contrast of two pinned inks against that cell's own background. The
//! oracle publishes `min_contrast` — the worst contrast ratio over the whole
//! grid — so an AI verifies the accessibility floor (WCAG 4.5 for small text)
//! over the wire, with no pixel and no eyeballing.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! Cells are tagged `dev.cell.{r}.{c}` with their rects in the paint scene, and
//! the model is queryable through the [`DeviationOracle`]: `rows` / `cols` /
//! `min` / `mid` / `max` / `neutral_hex` / `min_contrast`, plus the
//! `value_at` / `color_at` / `linear_color_at` / `ink_at` oracles. See
//! `tools/demos/r1436_deviation_grid.py`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ColorScale, contrast_ratio, readable_ink};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDeviationGridRenderer, HelloDeviationGridRendererError);

const THEME_TAG: &str = "app";

/// The grid's primary tag — the [`DeviationOracle`]'s registration tag,
/// addressed over RPC as `/external/<field>`.
const GRID_TAG: &str = "deviation";

/// The grid extent. Declared in `u32` (the pixel-arithmetic type) and narrowed
/// to `usize` for indexing: `u32 -> usize` is lossless on every target pinion
/// builds for, whereas `usize -> u32` is the truncating direction.
const COLS_U32: u32 = 10;
const ROWS_U32: u32 = 6;
const COLS: usize = COLS_U32 as usize;
const ROWS: usize = ROWS_U32 as usize;

/// The deviation domain. Deliberately **asymmetric** — this is the case a
/// linear map gets wrong, and the reason `map_diverging` exists.
const MIN_DEV: f64 = -14.0;
const MAX_DEV: f64 = 32.0;
/// The baseline every value is measured against: the neutral anchor.
const MID_DEV: f64 = 0.0;

const CELL_W: u32 = 62;
const CELL_H: u32 = 40;
const CELL_GAP: u32 = 3;
const GRID_X: u32 = 24;
const GRID_Y: u32 = 64;

const WIN_W: u32 = GRID_X * 2 + COLS_U32 * (CELL_W + CELL_GAP);
const WIN_H: u32 = GRID_Y + ROWS_U32 * (CELL_H + CELL_GAP) + 92;

const TITLE_FONT_PX: u32 = 16;
const CELL_FONT_PX: u32 = 13;
const STATUS_FONT_PX: u32 = 12;

/// The legend strip: the ramp end to end, with the zero anchor marked.
const LEGEND_STEPS_U32: u32 = 24;
const LEGEND_STEPS: usize = LEGEND_STEPS_U32 as usize;
const LEGEND_H: u32 = 14;

// --- The data ---------------------------------------------------------------

/// The deviation matrix: a fixed, deterministic pattern spanning
/// [`MIN_DEV`]`..=`[`MAX_DEV`] and crossing zero, so both wings of the ramp and
/// the neutral itself are exercised. Row 2 is pinned to exact zeros — the cells
/// whose colour must BE the neutral stop, which is the property under test.
fn matrix() -> [[f64; COLS]; ROWS] {
    let mut m = [[0.0_f64; COLS]; ROWS];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = if r == 2 {
                // The on-target row: exactly the baseline.
                0.0
            } else {
                // A deterministic spread that reaches both domain ends: the
                // column drives the magnitude, the row the sign and scale.
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "row / column indices are single digits; f64 represents them exactly"
                )]
                let (rf, cf) = (r as f64, c as f64);
                let span = if r < 2 { MIN_DEV } else { MAX_DEV };
                let depth = (rf - 2.0).abs() / 3.0_f64.max(1.0);
                let across = (cf + 1.0) / f64::from(COLS_U32);
                (span * depth * across * 1.15).clamp(MIN_DEV, MAX_DEV)
            };
        }
    }
    m
}

/// The value rounded for display / wire: one decimal is enough to read a
/// deviation and keeps the label inside its cell.
fn fmt_value(v: f64) -> String {
    if v > 0.0 {
        format!("+{v:.1}")
    } else {
        format!("{v:.1}")
    }
}

/// A colour as the `#rrggbb` wire form the oracle publishes — the introspect
/// surface has no colour type, and hex is what a client can compare exactly.
fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

// --- The palette ------------------------------------------------------------

/// The diverging ramp + the two candidate inks. The ramp is the crate's
/// colour-blind-safe [`ColorScale::blue_orange`] default (Okabe-Ito blue →
/// neutral → vermillion), NOT a theme colour: a deviation's colour must mean
/// the same thing in light and dark mode, which is the theme-decoupling
/// `pinion-chart` documents. The inks are pinned for the same reason — see
/// [`INK_DARK`].
#[derive(Debug, Clone)]
struct Palette {
    ramp: ColorScale,
    ink_dark: Color,
    ink_light: Color,
}

impl Palette {
    fn new(ink_dark: Color, ink_light: Color) -> Self {
        Self {
            ramp: ColorScale::blue_orange(),
            ink_dark,
            ink_light,
        }
    }

    /// The cell background for a deviation — the diverging map, anchored on the
    /// baseline.
    fn cell_color(&self, value: f64) -> Color {
        self.ramp.map_diverging(value, MIN_DEV, MID_DEV, MAX_DEV)
    }

    /// The SAME value through the linear map — published only so a client can
    /// see that it is NOT neutral at zero. Never painted.
    fn linear_color(&self, value: f64) -> Color {
        self.ramp.map(value, MIN_DEV, MAX_DEV)
    }

    /// The legible ink on this cell, computed per cell.
    fn cell_ink(&self, value: f64) -> Color {
        readable_ink(self.cell_color(value), self.ink_dark, self.ink_light)
    }

    /// The ramp's neutral (centre) stop — what a zero cell must be.
    fn neutral(&self) -> Color {
        self.ramp.sample(0.5)
    }
}

/// The label inks — **pinned, not theme-derived**, and deliberately so.
///
/// The ramp is theme-independent (a deviation must mean the same thing in light
/// and dark mode), so a label drawn on top of it has to be too: a theme-derived
/// ink would be chosen against the app's SURFACE, not against the ramp cell it
/// actually sits on, and a dark theme's pale ink over the ramp's light neutral
/// would be unreadable. Pinning both candidates keeps the contrast the oracle
/// publishes EXACTLY the contrast the paint achieves — otherwise `min_contrast`
/// would be a claim about numbers no viewer ever sees.
///
/// The dark ink is PURE black, not the conventional near-black `#1a1a1a`,
/// because the measurement said so: the ramp's vermillion end is only
/// mid-luminance, and `#1a1a1a` on it computes to **4.5005:1** — clearing the
/// WCAG small-text floor by five ten-thousandths. A claim that survives only by
/// coincidence is not a claim; pure black takes the same cell to ~5.4:1, so the
/// floor is met with real margin.
const INK_DARK: Color = Color::rgb(0x00, 0x00, 0x00);
const INK_LIGHT: Color = Color::rgb(0xFF, 0xFF, 0xFF);

// --- The oracle (primary External) ------------------------------------------

/// Publishes the deviation model and its colour encoding so a client verifies
/// both the mapping and its legibility without a pixel.
#[derive(Debug)]
struct DeviationOracle {
    m: [[f64; COLS]; ROWS],
    palette: Palette,
}

impl DeviationOracle {
    fn new() -> Self {
        Self {
            m: matrix(),
            palette: Palette::new(INK_DARK, INK_LIGHT),
        }
    }

    /// Parse an `"r,c"` argument into an in-range cell.
    ///
    /// The two failure modes are distinguished because the [`InvokeError`]
    /// vocabulary distinguishes them: a non-string argument is a
    /// [`TypeMismatch`](InvokeError::TypeMismatch) (retrying with the same
    /// shape cannot help), while a malformed or out-of-range coordinate is a
    /// [`Rejected`](InvokeError::Rejected) (the path and type are right and
    /// another cell would succeed).
    fn parse_cell(arg: &IntrospectValue) -> Result<(usize, usize), InvokeError> {
        let IntrospectValue::Text(s) = arg else {
            return Err(InvokeError::TypeMismatch);
        };
        let parsed = s.split_once(',').and_then(|(r, c)| {
            let r: usize = r.trim().parse().ok()?;
            let c: usize = c.trim().parse().ok()?;
            (r < ROWS && c < COLS).then_some((r, c))
        });
        parsed.ok_or_else(|| {
            InvokeError::rejected(format!(
                "{s:?} does not address a cell (expected \"<row>,<col>\" with \
                 row < {ROWS} and col < {COLS})"
            ))
        })
    }

    /// The worst WCAG contrast ratio over every cell — the accessibility floor
    /// as a single number a client can assert against.
    fn min_contrast(&self) -> f32 {
        let mut worst = f32::MAX;
        for row in &self.m {
            for &v in row {
                let ratio = contrast_ratio(self.palette.cell_color(v), self.palette.cell_ink(v));
                worst = worst.min(ratio);
            }
        }
        worst
    }
}

impl External for DeviationOracle {
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

impl ExternalIntrospect for DeviationOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("rows", "int"),
                    SchemaField::new("cols", "int"),
                    // The diverging domain: the neutral sits at `mid`, and the
                    // two wings are deliberately unequal.
                    SchemaField::new("min", "float"),
                    SchemaField::new("mid", "float"),
                    SchemaField::new("max", "float"),
                    // The ramp's centre stop, `#rrggbb` — what a zero cell is.
                    SchemaField::new("neutral_hex", "string"),
                    // The worst WCAG contrast over the grid (>= 4.5 = legible).
                    SchemaField::new("min_contrast", "float"),
                    // Per-cell oracles, arg `"r,c"`.
                    SchemaField::action("value_at", "string"),
                    SchemaField::action("color_at", "string"),
                    SchemaField::action("linear_color_at", "string"),
                    SchemaField::action("ink_at", "string"),
                    SchemaField::action("contrast_at", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "rows" => Some(IntrospectValue::Int(i64::from(ROWS_U32))),
            "cols" => Some(IntrospectValue::Int(i64::from(COLS_U32))),
            "min" => Some(IntrospectValue::Float(MIN_DEV)),
            "mid" => Some(IntrospectValue::Float(MID_DEV)),
            "max" => Some(IntrospectValue::Float(MAX_DEV)),
            "neutral_hex" => Some(IntrospectValue::Text(hex(self.palette.neutral()))),
            "min_contrast" => Some(IntrospectValue::Float(f64::from(self.min_contrast()))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The whole surface is a read-only projection of fixed data.
            "rows" | "cols" | "min" | "mid" | "max" | "neutral_hex" | "min_contrast" => {
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
        match path {
            "value_at" | "color_at" | "linear_color_at" | "ink_at" | "contrast_at" => {
                let (r, c) = Self::parse_cell(&args)?;
                let v = self.m[r][c];
                Ok(match path {
                    "value_at" => IntrospectValue::Float(v),
                    "color_at" => IntrospectValue::Text(hex(self.palette.cell_color(v))),
                    "linear_color_at" => IntrospectValue::Text(hex(self.palette.linear_color(v))),
                    "ink_at" => IntrospectValue::Text(hex(self.palette.cell_ink(v))),
                    _ => IntrospectValue::Float(f64::from(contrast_ratio(
                        self.palette.cell_color(v),
                        self.palette.cell_ink(v),
                    ))),
                })
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The view ---------------------------------------------------------------

/// A cell's window-absolute rect.
fn cell_rect(r: usize, c: usize) -> Rect {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "ROWS / COLS are single-digit consts; the products are small pixel counts"
    )]
    let (rc, cc) = (r as u32, c as u32);
    Rect::new(
        GRID_X + cc * (CELL_W + CELL_GAP),
        GRID_Y + rc * (CELL_H + CELL_GAP),
        CELL_W,
        CELL_H,
    )
}

fn read_oracle(scene: &Scene) -> f32 {
    scene
        .find_external_with_tag(GRID_TAG)
        .and_then(|n| n.handle.introspect())
        .and_then(|i| match i.query("min_contrast") {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a WCAG ratio is 1.0..=21.0; f32 is its native precision"
            )]
            Some(IntrospectValue::Float(f)) => Some(f as f32),
            _ => None,
        })
        .unwrap_or(0.0)
}

/// view-fn (§6.3): pure sync mapping of the model to a scene.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference; the signature mirrors it"
)]
fn view(min_contrast: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let outline = theme.resolve(ColorRole::Outline);
    // The SAME palette the oracle publishes — see [`INK_DARK`]: pinned inks are
    // what make `min_contrast` a statement about the pixels, not about a
    // hypothetical theme.
    let palette = Palette::new(INK_DARK, INK_LIGHT);

    let m = matrix();
    let mut children: Vec<Scene> = Vec::with_capacity(ROWS * COLS * 2 + LEGEND_STEPS + 4);

    children.push(Scene::Text(
        TextNode::styled(
            "Deviation from baseline — blue below, orange above, neutral AT zero",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(GRID_X, 22)),
    ));

    for (r, row) in m.iter().enumerate() {
        for (c, &value) in row.iter().enumerate() {
            let rect = cell_rect(r, c);
            children.push(Scene::Box(
                BoxNode::new(
                    Rect::default(),
                    BoxStyle::filled(palette.cell_color(value))
                        .with_border(Border::new(outline, 1)),
                )
                .with_tag(cell_tag(r, c))
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(rect.x, rect.y)
                        .with_size(Size::px(rect.w, rect.h)),
                ),
            ));
            children.push(Scene::Text(
                TextNode::styled(
                    fmt_value(value),
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(CELL_FONT_PX)
                        .with_fg(palette.cell_ink(value)),
                )
                .with_layout(LayoutStyle::new().with_absolute_position(rect.x + 10, rect.y + 13)),
            ));
        }
    }

    // The legend: the ramp end to end, with the neutral step marked so a reader
    // can see WHERE zero sits on a scale whose two wings are unequal.
    let legend_y = GRID_Y + ROWS_U32 * (CELL_H + CELL_GAP) + 14;
    let step_w = (COLS_U32 * (CELL_W + CELL_GAP)) / LEGEND_STEPS_U32;
    for i in 0..LEGEND_STEPS {
        #[allow(
            clippy::cast_precision_loss,
            reason = "LEGEND_STEPS is a small const; the fraction is exact enough for a swatch"
        )]
        let t = i as f32 / (LEGEND_STEPS - 1) as f32;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "LEGEND_STEPS is a small const"
        )]
        let x = GRID_X + i as u32 * step_w;
        children.push(Scene::Box(
            BoxNode::new(Rect::default(), BoxStyle::filled(palette.ramp.sample(t))).with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x, legend_y)
                    .with_size(Size::px(step_w, LEGEND_H)),
            ),
        ));
    }

    children.push(Scene::Text(
        TextNode::styled(
            format!(
                "domain {MIN_DEV:.0}..{MAX_DEV:.0} (asymmetric) — neutral anchored at {MID_DEV:.0} \
                 — worst cell contrast {min_contrast:.1}:1",
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_tag("dev.readout")
        .with_layout(LayoutStyle::new().with_absolute_position(GRID_X, WIN_H - 30)),
    ));

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// A cell's paint tag, `dev.cell.{r}.{c}`. A plain `String`: the scene's tag
/// type is `Cow<'static, str>`, so an owned per-cell tag needs no leak and no
/// cache — the substrate already carries the ownership.
fn cell_tag(r: usize, c: usize) -> String {
    format!("dev.cell.{r}.{c}")
}

// --- The binding ------------------------------------------------------------

struct DeviationGridView;

impl WidgetCore for DeviationGridView {
    type State = f32;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(DeviationOracle::new())
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn read_state(scene: &Scene) -> f32 {
        read_oracle(scene)
    }

    fn view(state: f32, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-deviation-grid (R1436 §5.35 diverging colour scale)"
    }

    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &f32) -> String {
        format!("worst cell contrast {state:.2}:1")
    }
}

impl WidgetA11y for DeviationGridView {
    fn access_node(state: &f32, _focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(GRID_TAG, AriaRole::Group)
                .with_name("Deviation grid")
                .with_value(AccessValue::Text(format!(
                    "{ROWS} by {COLS} deviations from baseline, worst cell contrast {state:.1} to 1"
                ))),
        ]
    }
}

impl WidgetView for DeviationGridView {
    type Renderer = HelloDeviationGridRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<DeviationGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::test_fixtures::assert_refused_saying;

    fn palette() -> Palette {
        Palette::new(INK_DARK, INK_LIGHT)
    }

    #[test]
    fn the_matrix_spans_the_domain_and_crosses_zero() {
        let m = matrix();
        let flat: Vec<f64> = m.iter().flatten().copied().collect();
        let lo = flat.iter().copied().fold(f64::MAX, f64::min);
        let hi = flat.iter().copied().fold(f64::MIN, f64::max);
        assert!(lo < -8.0, "the negative wing is exercised: {lo}");
        assert!(hi > 20.0, "the positive wing is exercised: {hi}");
        assert!(lo >= MIN_DEV && hi <= MAX_DEV, "inside the domain");
        assert!(m[2].iter().all(|v| *v == 0.0), "row 2 is exactly on target");
    }

    #[test]
    fn a_zero_cell_is_exactly_the_neutral_and_the_linear_map_is_not() {
        // The property the whole demo exists to show. `map_diverging` puts the
        // baseline on the ramp's centre stop; the linear `map` cannot, because
        // the domain's two wings are not the same width.
        let p = palette();
        assert_eq!(p.cell_color(0.0), p.neutral(), "zero is the neutral colour");
        assert_ne!(
            p.linear_color(0.0),
            p.neutral(),
            "the linear map misplaces the neutral on an asymmetric domain"
        );
    }

    #[test]
    fn each_wing_is_normalised_on_its_own_width() {
        // Half of the short (negative) wing and half of the long (positive) one
        // are equally saturated — that is what "anchored" means.
        let p = palette();
        assert_eq!(p.cell_color(MIN_DEV / 2.0), p.ramp.sample(0.25));
        assert_eq!(p.cell_color(MAX_DEV / 2.0), p.ramp.sample(0.75));
        // And the ends are the ends.
        assert_eq!(p.cell_color(MIN_DEV), p.ramp.sample(0.0));
        assert_eq!(p.cell_color(MAX_DEV), p.ramp.sample(1.0));
    }

    #[test]
    fn every_cell_clears_the_wcag_small_text_floor() {
        // The accessibility claim, asserted rather than asserted-in-prose: with
        // the ink computed per cell, no cell in the grid falls below 4.5:1.
        let o = DeviationOracle::new();
        assert!(
            o.min_contrast() >= 4.5,
            "worst cell contrast {} must clear 4.5:1",
            o.min_contrast()
        );
        // ...and with MARGIN. The near-black `#1a1a1a` this demo first used
        // cleared the floor by 0.0005 on the vermillion end, which is a
        // coincidence rather than a design (see `INK_DARK`). Asserting the
        // margin keeps a future ramp tweak from silently landing back on the
        // edge.
        assert!(
            o.min_contrast() >= 5.0,
            "the ink pair must clear the floor with margin, got {}",
            o.min_contrast()
        );
    }

    #[test]
    fn the_oracle_publishes_the_model_and_the_encoding() {
        let o = DeviationOracle::new();
        assert!(
            matches!(o.query("rows"), Some(IntrospectValue::Int(r)) if r == i64::from(ROWS_U32))
        );
        assert!(matches!(o.query("mid"), Some(IntrospectValue::Float(m)) if m == MID_DEV));
        let neutral = match o.query("neutral_hex") {
            Some(IntrospectValue::Text(s)) => s,
            other => panic!("neutral_hex must be text, got {other:?}"),
        };
        assert!(neutral.starts_with('#') && neutral.len() == 7, "{neutral}");
        assert_eq!(o.query("nope"), None, "an unknown path reads as None");
    }

    #[test]
    fn the_cell_oracles_answer_and_reject_out_of_range() {
        let mut o = DeviationOracle::new();
        // Row 2 is the on-target row, so its colour IS the neutral.
        let neutral = match o.query("neutral_hex") {
            Some(IntrospectValue::Text(s)) => s,
            other => panic!("neutral_hex must be text, got {other:?}"),
        };
        let at = o
            .invoke("color_at", IntrospectValue::Text("2,4".to_owned()))
            .expect("color_at answers an in-range cell");
        assert_eq!(at, IntrospectValue::Text(neutral));
        // Out of range and malformed args reject rather than clamping into a
        // neighbouring cell's answer — and the two failure kinds are told
        // apart: a wrong ARG TYPE can never succeed, a wrong CELL might.
        assert_refused_saying(
            &o.invoke("value_at", IntrospectValue::Text("99,0".to_owned())),
            "\"99,0\" does not address a cell",
        );
        assert_refused_saying(
            &o.invoke("value_at", IntrospectValue::Text("nope".to_owned())),
            "\"nope\" does not address a cell",
        );
        assert!(matches!(
            o.invoke("value_at", IntrospectValue::Float(3.0)),
            Err(InvokeError::TypeMismatch)
        ));
        assert!(matches!(
            o.invoke("unknown", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        ));
    }

    #[test]
    fn cells_are_tagged_and_laid_out_without_overlap() {
        let a = cell_rect(0, 0);
        let b = cell_rect(0, 1);
        let c = cell_rect(1, 0);
        assert_eq!(a.x + a.w + CELL_GAP, b.x, "columns abut with the gap");
        assert_eq!(a.y + a.h + CELL_GAP, c.y, "rows abut with the gap");
        assert_eq!(cell_tag(3, 7), "dev.cell.3.7");
        let last = cell_rect(ROWS - 1, COLS - 1);
        assert!(last.x + last.w <= WIN_W, "the grid fits the window in x");
        assert!(last.y + last.h <= WIN_H, "the grid fits the window in y");
    }
}
