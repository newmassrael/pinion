//! R1067 §5.37.11 — production font-load bridge: an OS system font → a parsed,
//! *selected* §5.37 [`Font`].
//!
//! This is the connecting layer the §5.37 self-hosted text engine has lacked.
//! The engine ([`pinion_text_font`]) can shape + rasterize, and the
//! R1063–R1066 paint seam (`paint_adapter::draw_atlased_glyphs`) can turn its
//! glyph atlas into real pixels — but nothing supplied a *production* font: the
//! bundled Noto / Nanum files are §5.37.1 parser fixtures, and committing a
//! production font is disallowed. [`pinion_platform_fonts`] (§5.37.11) enumerates
//! the OS's installed font files; this module is where the parser lives, so this
//! is also where **font selection** happens — picking a default regular sans by
//! the font's OWN parsed metadata (`name` / `OS/2` tables), never by guessing
//! from a filename. Enumeration is OS knowledge (the platform layer); selection
//! is font knowledge (here).
//!
//! [`load_system_font`] is the single connector. The R1068 opt-in `Scene::Text`
//! paint arm ([`crate::paint_adapter::to_vello_with_text_engine`]) consumes a
//! [`SelfHostedTextEngine`] built from it; the parley path stays the default.

use crate::layout::{TextBox, TextMeasure};
use pinion_core::scene::StyleRun;
use pinion_core::style::{FontFamily, FontStyle, FontWeight, LineHeight, TextAlign, TextStyle};
use pinion_text::{CaretRect, TextLayout, VisualLineMetric};
use pinion_text_font::{Font, shape_paragraph_with_fallback};
use std::path::PathBuf;

/// R1068 §5.37 — a self-hosted text engine handle that caches the parsed
/// production [`Font`] across frames, so the opt-in `Scene::Text` paint arm
/// ([`crate::paint_adapter::to_vello_with_text_engine`]) never re-discovers or
/// re-parses the font per frame.
///
/// What this caches and what it does NOT: it holds the parsed [`Font`] (parsing
/// is the per-frame cost this removes). It does **not** yet cache the rasterized
/// glyph atlas — the arm re-runs `render_paragraph_atlased` each paint; a
/// cross-frame glyph-atlas cache is a later campaign step. It also holds a
/// single font today (the R1068 single-style arm); a fallback stack is the
/// multi-font step. Construct once at app / window init via
/// [`SelfHostedTextEngine::from_system_font`].
///
/// R1479 §5.37 — it also carries the family unset text resolves to in this
/// process ([`Self::with_default_family`]), because holding one face means the
/// arm has to know which requests that face answers. See [`Self::serves`].
pub struct SelfHostedTextEngine {
    font: Font,
    /// The process-wide default family (R1472 `ShellConfig::with_default_font_family`),
    /// or `None` when unset text means the platform stack. Boot-time, like the
    /// font: what a `TextStyle` leaves out is resolved against this before the
    /// arm decides whether it may render the leaf.
    default_family: Option<FontFamily>,
}

impl SelfHostedTextEngine {
    /// Build an engine from the selected default system font ([`load_system_font`]).
    ///
    /// # Errors
    ///
    /// Returns [`LoadFontError::NoSystemFont`] when no usable system font is found.
    pub fn from_system_font() -> Result<Self, LoadFontError> {
        Ok(Self {
            font: load_system_font()?,
            default_family: None,
        })
    }

    /// Build an engine from an already-parsed [`Font`] — the deterministic
    /// constructor a test drives with a bundled fixture (system discovery is
    /// environment-dependent), and the seam a future caller uses to supply a
    /// specific (e.g. embedded brand) font.
    #[must_use]
    pub fn from_font(font: Font) -> Self {
        Self {
            font,
            default_family: None,
        }
    }

    /// R1479 §5.37 — declare the family unset text resolves to in this process,
    /// so the arm resolves a leaf's request the same way the platform shaper
    /// does before deciding whether it may render it ([`Self::serves`]).
    ///
    /// The shell passes what it set on the render cache
    /// (`LayoutCache::set_default_font_family`), from the one `ShellConfig`
    /// value, so the two shapers cannot disagree about what "unset" means.
    #[must_use]
    pub fn with_default_family(mut self, family: Option<FontFamily>) -> Self {
        self.default_family = family;
        self
    }

    /// The engine's font.
    #[must_use]
    pub fn font(&self) -> &Font {
        &self.font
    }

    /// R1479 §5.37 — the family the engine's face declares in its own `name`
    /// table, or `None` on a face that declares none.
    ///
    /// This is the whole of what the arm can render, so it is also what the
    /// shell publishes as [`SelfHostedFace`](pinion_core::reactive::SelfHostedFace).
    #[must_use]
    pub fn served_family(&self) -> Option<String> {
        self.font.family_name()
    }

    /// R1479 §5.37 — may this arm render this leaf? The eligibility SSOT both
    /// arms ask, and the only entry point either should use.
    ///
    /// Two independent questions, deliberately answered in one call so a caller
    /// cannot satisfy one and forget the other (the split-rule class R1449 fixed
    /// elsewhere):
    ///
    /// 1. is the leaf's SHAPE one the arm reproduces — [`self_hosted_text_eligible`];
    /// 2. is the leaf's FACE the one the arm holds — [`Self::serves_family`].
    #[must_use]
    pub fn serves(
        &self,
        content: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        caret_bearing: bool,
    ) -> bool {
        self_hosted_text_eligible(content, style, runs, caret_bearing) && self.serves_family(style)
    }

    /// R1479 §5.37 — does this leaf's requested face resolve to the ONE face the
    /// engine holds?
    ///
    /// The arm shapes with `self.font` whatever the style says, so before R1479
    /// a leaf naming a family was measured and painted in a face nobody asked
    /// for — and an application default ([`Self::with_default_family`]) was
    /// silently overridden for every leaf that named nothing, which is the whole
    /// of what R1472 built. The face is not a rendering detail: it changes the
    /// advance, so the *measured box* is wrong too, and text that folds through
    /// the requested face collapses to one line through this one.
    ///
    /// Resolution mirrors the platform shaper's: the style's own family, else
    /// the process default, else nothing.
    ///
    /// - **nothing requested** — the arm is the default face by construction
    ///   (this is the R1068 premise, and the configuration every §5.37 test and
    ///   demo has exercised): serve it.
    /// - **a named family** — serve it only when the engine's face declares that
    ///   name (ASCII-case-insensitive, as font family matching is). Anything
    ///   else defers.
    /// - **a CSS generic** (`sans-serif`, `monospace`, …) — defer, always. The
    ///   engine cannot *prove* its face is what this host resolves the keyword
    ///   to; only fontconfig / the platform database knows, and a guess here
    ///   would be the same silent wrong-face bug in a smaller costume.
    ///
    /// Serving a strict subset is always safe: everything declined renders
    /// through the platform shaper, which is where all of it rendered before the
    /// arm existed.
    #[must_use]
    pub fn serves_family(&self, style: &TextStyle) -> bool {
        match style.font_family.as_ref().or(self.default_family.as_ref()) {
            None => true,
            Some(FontFamily::Named(name)) => self
                .served_family()
                .is_some_and(|served| served.eq_ignore_ascii_case(name)),
            Some(FontFamily::Generic(_)) => false,
        }
    }
}

/// R1070 §5.37 — the STYLE-SHAPE half of the eligibility SSOT, shared by the
/// paint arm (`crate::paint_adapter::self_hosted_eligible`) and the measure arm
/// ([`SelfHostedTextEngine`]'s [`TextMeasure`] impl).
///
/// R1479 — the other half is the FACE ([`SelfHostedTextEngine::serves_family`]),
/// which needs the engine and so cannot live in a free function. Both arms call
/// [`SelfHostedTextEngine::serves`], which is the two together; this is public
/// because the conditions below are a property of the leaf alone and a caller
/// may want to ask about them without an engine in hand.
///
/// These are the NECESSARY conditions for a `Scene::Text` leaf to be both
/// *rendered and sized* by the §5.37 self-hosted engine so paint and measure
/// register exactly — every one is a case the arm would otherwise diverge from
/// parley:
///
/// - **single style** (`runs` empty) — styled runs are the multi-style step;
/// - **no hard line break** (`'\n'`) — the arm is one line, not a paragraph;
/// - **default `Start` alignment** — the arm pens from the box left, so Center /
///   End would shift versus parley;
/// - **`Normal` line height** — the arm's baseline matches parley's natural line
///   box only in `Normal` mode; a fixed / multiplied height moves parley's
///   baseline by leading the arm does not model;
/// - **`Normal` weight and `Normal` style** (R1510) — the arm holds ONE parsed
///   face, so it can only ever pen that face's outlines. A leaf asking for bold
///   or italic would be served in Regular and its declaration silently lost,
///   which is the R1505 defect (a declaration that does not reach the glyphs) in
///   a second channel; parley selects a real face for the same request. Measured
///   when R1510 gave a header label `FontWeight::BOLD`: the predicate read
///   alignment, line height, decoration and runs, never the weight, so a
///   `Start`-aligned bold label was the arm's and painted Regular. This is the
///   same shape as the alignment condition above — the arm declines what it
///   cannot reproduce rather than approximating it;
/// - **not caret-bearing** (`caret_bearing` false) — a [`TextField`] derives its
///   caret / selection / hit-test geometry from a separate parley shaping of this
///   same string ([`pinion_core::scene::TextNode::caret_bearing`]), so re-shaping
///   the painted glyphs through §5.37 would drift those overlays off the text;
///   editable text therefore stays on parley for both arms (the R1070.1
///   "exclude caret-bearing text" contract).
///
/// Necessary, not sufficient: both arms additionally decline a single line that
/// would soft-wrap (see [`SelfHostedTextEngine::measure_text`] and
/// `paint_text_self_hosted`). Everything excluded here stays on parley for BOTH
/// measure and paint, so the two never disagree on which path renders a leaf.
///
/// [`TextField`]: pinion_core::widgets::text_field
#[must_use]
pub fn self_hosted_text_eligible(
    content: &str,
    style: &TextStyle,
    runs: &[StyleRun],
    caret_bearing: bool,
) -> bool {
    !caret_bearing
        && runs.is_empty()
        && !content.contains('\n')
        && matches!(style.text_align, TextAlign::Start)
        && matches!(style.line_height, LineHeight::Normal)
        && style.font_weight == FontWeight::NORMAL
        && matches!(style.font_style, FontStyle::Normal)
        && !style.decoration.underline.is_on()
        && !style.decoration.strikethrough
        // R1546 §5.36 — a declared background DECLINES to this arm rather than
        // being dropped by it. The arm paints glyphs from its own atlas and
        // knows nothing of the band derivation, so serving such a leaf would
        // render it without the background the scene declares — a silent
        // divergence between what the scene says and what is on screen, which
        // is the one failure this eligibility SSOT exists to make impossible.
        // Declining is always safe: the leaf renders through parley, where
        // every leaf rendered before this arm existed.
        && style.bg_color.is_none()
        // R1551 §5.36 — a declared CSS `text-indent` DECLINES to this arm for
        // the same reason a background does. The indent is a *breaking* input:
        // parley narrows the selected line's available width and offsets it,
        // and this arm's `wrap_paragraph` knows neither. Serving such a leaf
        // would paint a paragraph flush that the scene declares indented — and
        // `scene/text_blocks` would publish the declaration beside line boxes
        // that do not honour it, so the divergence would reach the wire too.
        //
        // Keyed on `is_none()` (the amount) rather than on the whole value: the
        // two CSS keywords cannot move a line by themselves, so a zero-amount
        // indent carrying `hanging` is nothing to reproduce and stays eligible.
        && style.text_indent.is_none()
        // R1641.7 §5.36 — declared TRACKING declines to this arm, for exactly
        // the reason the background and the indent above it do. This arm shapes
        // through its own pen and never applies a letter- or word-spacing
        // value, so serving such a leaf would paint text at the font's natural
        // advances while the scene declares it tracked — the silent divergence
        // this predicate exists to make impossible.
        //
        // It had no clause here from R47.5 until R1641.7, and an audit found it
        // rather than a test: the arm reports a box and paints glyphs, and both
        // halves ignore the same field, so measure and paint agree with EACH
        // OTHER while both disagree with the scene. Two arms consistent with one
        // another is what made it invisible.
        //
        // Keyed on the RESOLVED amount rather than on the variant: `PxX100(0)`
        // and `EmX1000(0)` are callers who asked for no tracking and get
        // exactly that, so declining them would cost an eligible leaf and
        // reproduce nothing. The first draft matched on `Normal` and its own
        // test caught the disagreement between that and this paragraph.
        && style.letter_spacing.resolved_px_x100(style.font_size_px) == 0
        && style.word_spacing.resolved_px_x100(style.font_size_px) == 0
}

/// R1070.1 §5.37 — the vertical metrics of one `Normal` line box for `font` at
/// `px`, derived ONCE from the font's own `hhea` metrics so the §5.37 paint
/// baseline ([`crate::paint_adapter`]'s `paint_text_self_hosted`) and the §5.37
/// measure box height ([`SelfHostedTextEngine::measure_text`]) cannot drift apart.
/// This is the SSOT for the "paint + measure register exactly" contract — the
/// same lift discipline R1070 applied to eligibility ([`self_hosted_text_eligible`]),
/// now applied to the metric math itself (R1070.1 audit-clearance).
///
/// `descender` is negative, so `ascender − descender + line_gap` is the full line
/// box; the first baseline sits at `ascender + line_gap/2` (half-leading split
/// above the ascent), so the ascent above the baseline and the descent + remaining
/// half-leading below it fill `height_px` by construction.
///
/// PARLEY PARITY (R1079): the metrics come from [`Font::vertical_line_metrics`],
/// which applies the same `OS/2` `USE_TYPO_METRICS` selection parley uses (via
/// `skrifa::metrics::Metrics`), so for a single line this box matches parley's for
/// the same font — the precondition for flipping the §5.37 engine on by default.
/// Pre-R1079 this read `hhea` directly and diverged on fonts like `NanumGothic`
/// that set `USE_TYPO_METRICS` with typo metrics ≠ `hhea` (a 25%-of-em line box).
#[derive(Clone, Copy)]
pub(crate) struct LineBoxMetrics {
    /// First-baseline offset from the box top, device px = `(ascender + line_gap/2)·px/upem`.
    pub baseline_px: f64,
    /// Total `Normal` line-box height, device px = `(ascender − descender + line_gap)·px/upem`.
    pub height_px: f64,
}

impl LineBoxMetrics {
    /// Compute the line box, or `None` on a malformed font (`units_per_em` 0);
    /// both arms then fall through to parley.
    pub(crate) fn from_font(font: &Font, px: f32) -> Option<Self> {
        let upem = f64::from(font.units_per_em());
        if upem <= 0.0 {
            return None;
        }
        let scale = f64::from(px) / upem;
        // R1079 §5.37 — the OS/2 `USE_TYPO_METRICS`-aware selection parley uses,
        // not raw `hhea`, so the box registers with parley's for the same font.
        let vm = font.vertical_line_metrics();
        let ascender = f64::from(vm.ascender);
        let descender = f64::from(vm.descender);
        let line_gap = f64::from(vm.line_gap);
        Some(Self {
            baseline_px: (ascender + line_gap / 2.0) * scale,
            height_px: (ascender - descender + line_gap) * scale,
        })
    }
}

/// R1070.1 §5.37 — does a single §5.37 line of width `advance` px overflow the
/// available width `bound`? SSOT for the soft-wrap decline shared by the measure
/// arm ([`SelfHostedTextEngine::measure_text`], `bound` = taffy `max_width`) and
/// the paint arm (`paint_text_self_hosted`, `bound` derived from `rect.w`).
///
/// Each arm maps its own "unbounded" encoding to `None` before calling (taffy
/// passes `None` for a min-/max-content probe; the paint arm maps an unmeasured
/// `rect.w == 0` to `None`), so the *comparison* `advance > bound` is single-sourced
/// here while the genuinely-different bound normalisation stays explicit per arm.
#[allow(
    clippy::cast_precision_loss,
    reason = "bound <= 2^24 px in practice — exact in f32"
)]
pub(crate) fn single_line_overflows(advance: f32, bound: Option<u32>) -> bool {
    matches!(bound, Some(w) if advance > w as f32)
}

/// R1070 §5.37 — opt-in self-hosted MEASURE arm: size a single-style `Scene::Text`
/// leaf by the §5.37 engine so its box registers with the §5.37 paint arm
/// ([`crate::paint_adapter::to_vello_with_text_engine`]), closing the R1068
/// paint-only gap where a string wider than the §5.37 advance overflowed the
/// parley measured box.
impl TextMeasure for SelfHostedTextEngine {
    /// Returns `None` (defer to the parley measure — byte-identical to pre-R1070)
    /// when the leaf is ineligible ([`SelfHostedTextEngine::serves`] — its shape
    /// is one the arm does not reproduce, or its face is not the one it holds);
    /// the font is
    /// malformed (`units_per_em` 0) or the size non-positive; nothing shapes
    /// (empty content); or the single line would soft-wrap inside a bounded
    /// `max_width`. The soft-wrap decline shares the SSOT comparison
    /// `single_line_overflows` with `paint_text_self_hosted`, so for INK-BEARING
    /// content whenever measure picks §5.37 the paint arm also paints §5.37
    /// (advance ≤ box, no overflow) and whenever measure defers the paint arm
    /// declines too. The one asymmetry: all-whitespace content shapes to blank
    /// glyphs, so measure sizes its whitespace advance while the paint arm declines
    /// on `placed.is_empty()` (no ink) — measure cannot cheaply replicate that
    /// rasterizer-level check, but it is harmless (nothing inks, so nothing overflows;
    /// only the box width of a degenerate whitespace-only leaf differs from parley's).
    ///
    /// On the §5.37 path the box is `(advance, height)` from the SSOT
    /// `LineBoxMetrics`, whose `height_px` is the same `Normal` line box the paint
    /// baseline `baseline_px` sits inside — so measure and the §5.37 *paint* arm
    /// register exactly. Since R1079 that box uses the same `OS/2` `USE_TYPO_METRICS`
    /// selection parley applies (see `LineBoxMetrics`), so for a single line it also
    /// matches parley's box for the same font.
    fn measure_text(
        &self,
        content: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
        caret_bearing: bool,
    ) -> Option<TextBox> {
        if !self.serves(content, style, runs, caret_bearing) {
            return None;
        }
        let font = &self.font;
        #[allow(
            clippy::cast_precision_loss,
            reason = "font_size_px <= 2^24 px in practice — exact in f32"
        )]
        let px = style.font_size_px as f32;
        if px <= 0.0 {
            return None;
        }
        let shaped = shape_paragraph_with_fallback(&[font], content, px);
        if shaped.glyphs.is_empty() {
            return None;
        }
        // Soft-wrap decline mirror (shared SSOT comparison): a bounded box the single
        // §5.37 line overflows is exactly what parley wraps to multiple lines — defer
        // so the measured height reflects the wrap. The paint arm tests the same
        // advance against rect.w through the same `single_line_overflows`, so the two
        // arms decide "single line fits" identically.
        if single_line_overflows(shaped.advance, max_width) {
            return None;
        }
        // §5.37 line-box height from the SSOT shared with the paint baseline
        // (`LineBoxMetrics`); `?` defers on a malformed font (upem 0).
        let metrics = LineBoxMetrics::from_font(font, px)?;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a single line box height is a small positive px value — fits f32"
        )]
        let height = metrics.height_px as f32;
        // R1641 §5.21 — the baseline this arm reports is the SAME
        // `LineBoxMetrics::baseline_px` its paint arm draws on, so a row
        // aligning baselines aligns against where the glyphs actually land. An
        // engine that measured a box and declined to say where its baseline is
        // would take its leaves out of `AlignItems::Baseline` silently, and the
        // symptom — one item in a row not moving — points at the row, not here.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a first-baseline offset is a small positive px value — fits f32"
        )]
        let baseline = metrics.baseline_px as f32;
        // R1344 §5.12 — single-line BY CONSTRUCTION: `self_hosted_text_eligible`
        // rejects hard breaks and the `single_line_overflows` check above
        // declines anything that would soft-wrap, so this arm only ever measures
        // one line. That premise now travels WITH the impl that holds it rather
        // than being assumed by the layout caller.
        Some(TextBox {
            baseline: Some(baseline),
            ..TextBox::single_line(shaped.advance, height)
        })
    }
}

/// R1077 §5.37 — the §5.37 self-hosted engine's implementor of the
/// shaper-agnostic [`pinion_text::TextLayout`] caret / hit-test / line-metric
/// surface: the second implementor beside parley ([`pinion_text::Layout`]),
/// which is what makes the editable-text caret geometry stop naming one
/// concrete shaper (the R1077 directive).
///
/// It is a **local newtype** in this crate — the one crate that deps both
/// `pinion-text` (the trait) and `pinion-text-font` (the §5.37 shaping output)
/// — so the `impl` is orphan-rule-legal (a foreign trait on a local type).
///
/// Scope: a single §5.37 line, the engine's shipping frontier
/// ([`self_hosted_text_eligible`] excludes hard breaks and soft wrap). It is
/// built from the same [`shape_paragraph_with_fallback`] the measure arm uses
/// and the same `LineBoxMetrics` the §5.37 paint baseline sits inside, so a
/// caret drawn over this layout registers with the §5.37 *paint* glyphs by
/// construction. Multi-line §5.37 caret geometry arrives with multi-line
/// §5.37 paint.
///
/// LTR single line: the caret stops are the per-glyph pen origins (each
/// glyph's `x` is the insertion point *before* it) plus the trailing advance
/// (the insertion point after the last glyph, byte = `content.len()`),
/// ascending in both byte and x.
pub struct SelfHostedLayout {
    /// Caret stops `(byte, x)` ascending: one per glyph pen-origin plus the
    /// trailing `(content_len, advance)`. `x` is layout-space device px.
    stops: Vec<(usize, f32)>,
    /// Total line advance (device px) — the x of the end-of-text caret and the
    /// right clamp for hit-testing.
    advance: f32,
    /// UTF-8 byte length of the shaped content (the end-of-text caret byte).
    content_len: usize,
    /// `Normal` line-box height (device px) from `LineBoxMetrics` — the caret
    /// and selection-band height, the same box the §5.37 paint line occupies.
    height: f32,
}

impl SelfHostedLayout {
    /// Shape `content` as a single §5.37 line at `px` using `engine`'s font and
    /// return the queryable layout. `None` on a malformed font (`units_per_em`
    /// 0 — `LineBoxMetrics::from_font` declines), matching the measure arm's
    /// defer-to-parley.
    #[must_use]
    pub fn shape(engine: &SelfHostedTextEngine, content: &str, px: f32) -> Option<Self> {
        // R1078.1 audit — fail fast on the documented single-line precondition. The
        // §5.37 measure/paint eligibility gate (`self_hosted_text_eligible`) already
        // excludes `\n`, so production never reaches here with a hard break; this
        // catches a future caller that treats `SelfHostedLayout` as a general
        // multi-line layout (the single-line shaper would mis-place the break glyph
        // and `visual_lines` returns exactly one line).
        debug_assert!(
            !content.contains('\n'),
            "SelfHostedLayout::shape is single-line; multi-line text stays on parley",
        );
        let metrics = LineBoxMetrics::from_font(engine.font(), px)?;
        let shaped = shape_paragraph_with_fallback(&[engine.font()], content, px);
        let mut stops: Vec<(usize, f32)> = Vec::with_capacity(shaped.glyphs.len() + 1);
        for g in &shaped.glyphs {
            // One stop per distinct cluster: a ligature maps several glyphs to
            // one cluster (keep the first pen origin); ascending LTR clusters
            // keep `stops` sorted by byte for the binary search in `x_at`.
            if stops.last().is_none_or(|&(b, _)| b != g.cluster) {
                stops.push((g.cluster, g.x));
            }
        }
        stops.push((content.len(), shaped.advance));
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a single line box height is a small positive px value — fits f32"
        )]
        let height = metrics.height_px as f32;
        Some(Self {
            stops,
            advance: shaped.advance,
            content_len: content.len(),
            height,
        })
    }

    /// Layout-space x of the caret stop at `byte`. A byte that is not a glyph
    /// boundary (rare for a valid char-boundary caret) clamps to the next
    /// stop's x, or the trailing advance past the end — mirroring parley's
    /// total `Cursor::from_byte_index`.
    fn x_at(&self, byte: usize) -> f32 {
        match self.stops.binary_search_by_key(&byte, |&(b, _)| b) {
            Ok(i) => self.stops[i].1,
            Err(i) => self.stops.get(i).map_or(self.advance, |&(_, x)| x),
        }
    }
}

impl TextLayout for SelfHostedLayout {
    fn caret_rect(&self, byte_index: usize, caret_width: f32) -> CaretRect {
        CaretRect::new(self.x_at(byte_index), 0.0, caret_width, self.height)
    }

    fn byte_at_point(&self, x: f32, _y: f32) -> usize {
        // Single line: `y` selects the (only) line. Clamp x to the line extent
        // first — a point left of / right of the text snaps to the first / last
        // caret stop (parley's total `Cursor::from_point`), and the clamp also
        // sidesteps f32 precision loss when the derived `line_boundary` default
        // probes with `f32::MAX`.
        if x <= 0.0 {
            return self.stops.first().map_or(0, |&(b, _)| b);
        }
        if x >= self.advance {
            return self.content_len;
        }
        // Interior: the caret stop nearest in x (nearest-insertion semantics).
        self.stops
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(0, |&(b, _)| b)
    }

    fn selection_rects(&self, start: usize, end: usize) -> Vec<CaretRect> {
        if start == end {
            return Vec::new();
        }
        let xs = self.x_at(start);
        let xe = self.x_at(end);
        let (lo, hi) = if xs <= xe { (xs, xe) } else { (xe, xs) };
        vec![CaretRect::new(lo, 0.0, hi - lo, self.height)]
    }

    fn visual_lines(&self) -> Vec<VisualLineMetric> {
        // One §5.37 line: it opens logical line 0, box top at the layout origin.
        vec![VisualLineMetric::new(0.0, self.height, true)]
    }
}

/// Why [`load_system_font`] could not produce a [`Font`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadFontError {
    /// No usable (enumerable + parseable) font was found in the OS's standard
    /// font directories. On Linux this means the machine has no parseable
    /// `.ttf` / `.otf` installed; on macOS / Windows it is the current (deferred)
    /// state of [`pinion_platform_fonts::system_font_dirs`], which returns no
    /// directories there. Per-candidate parse failures are skipped (a corrupt
    /// font file does not abort selection), so this is the only failure surfaced.
    NoSystemFont,
}

impl std::fmt::Display for LoadFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSystemFont => {
                f.write_str("no usable system font found in the OS font directories")
            }
        }
    }
}

impl std::error::Error for LoadFontError {}

/// Preferred default sans-serif families, matched against a font's PARSED
/// `name`-table family (authoritative), not its filename. A best-effort "give me
/// a reasonable default UI font"; `select_default_sans` falls back to the first
/// regular non-mono face, then the first parseable font, when none is installed.
const PREFERRED_SANS_FAMILIES: &[&str] = &[
    "DejaVu Sans",
    "Noto Sans",
    "Liberation Sans",
    "FreeSans",
    "Ubuntu",
    "Cantarell",
    "Open Sans",
    "Roboto",
    "Arial",
];

/// Enumerate the OS's installed fonts and select a default regular sans, parsed
/// into a §5.37 [`Font`].
///
/// Selection (`select_default_sans`) is driven by each candidate's parsed
/// metadata, so the result does not depend on filenames. The font is not cached
/// here — [`SelfHostedTextEngine`] owns the cached handle; a per-frame caller
/// must cache itself.
///
/// # Errors
///
/// [`LoadFontError::NoSystemFont`] when no enumerable, parseable font exists.
pub fn load_system_font() -> Result<Font, LoadFontError> {
    select_default_sans(&pinion_platform_fonts::enumerate_fonts())
        .ok_or(LoadFontError::NoSystemFont)
}

/// Choose a default regular sans from enumerated font-file `paths`, parsing each
/// candidate so the choice rests on the font's own `name` / `OS/2` metadata.
///
/// Walks the (sorted, deterministic) candidates in three tiers: a
/// [`PREFERRED_SANS_FAMILIES`] regular face wins immediately; otherwise the first
/// regular-weight non-monospace face; otherwise the first parseable font. A
/// candidate that fails to parse is skipped (a corrupt file is not fatal).
fn select_default_sans(paths: &[PathBuf]) -> Option<Font> {
    let mut first_regular: Option<Font> = None;
    let mut first_parseable: Option<Font> = None;
    for path in paths {
        let Ok(bytes) = pinion_platform_fonts::read_font_bytes(path) else {
            continue;
        };
        let Ok(font) = Font::from_bytes(bytes) else {
            continue;
        };
        let regular_sans = is_regular(&font) && !font.is_monospace();
        if regular_sans && is_preferred_sans(&font) {
            return Some(font);
        }
        if regular_sans {
            first_regular.get_or_insert(font);
        } else {
            first_parseable.get_or_insert(font);
        }
    }
    first_regular.or(first_parseable)
}

/// `true` when the font's parsed `name`-table family is a [`PREFERRED_SANS_FAMILIES`]
/// member (case-insensitive, exact family match — not a substring of a filename).
fn is_preferred_sans(font: &Font) -> bool {
    font.family_name().is_some_and(|family| {
        PREFERRED_SANS_FAMILIES
            .iter()
            .any(|preferred| family.eq_ignore_ascii_case(preferred))
    })
}

/// `true` when the font declares itself a regular (upright, non-bold) face — read
/// from its `name`-table subfamily (the font's own style label), falling back to
/// the `OS/2` `usWeightClass` (`weight_class`) when the subfamily name is absent.
fn is_regular(font: &Font) -> bool {
    match font.subfamily_name() {
        Some(subfamily) => matches!(
            subfamily.to_ascii_lowercase().as_str(),
            "regular" | "book" | "roman" | "normal"
        ),
        None => font.weight_class() == 400,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTO_FIXTURE: &[u8] =
        include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");

    const NANUM_FIXTURE: &[u8] =
        include_bytes!("../../pinion-text-font/tests/fonts/NanumGothic-Regular.ttf");

    #[test]
    fn selection_predicates_read_parsed_metadata_not_filename() {
        // Deterministic forcing consumer for the selection logic: the bundled
        // NotoSans-Regular fixture's PARSED metadata classifies it as a preferred
        // regular sans. This proves selection rests on the `name`/`OS2` tables —
        // independent of the file's path (the fixture is loaded from bytes).
        let font = Font::from_bytes(NOTO_FIXTURE.to_vec()).expect("parse NotoSans fixture");
        assert!(
            is_preferred_sans(&font),
            "parsed family name should match a preferred sans (got {:?})",
            font.family_name()
        );
        assert!(
            is_regular(&font),
            "parsed subfamily should be regular (got {:?})",
            font.subfamily_name()
        );
        assert!(!font.is_monospace(), "NotoSans is proportional");
    }

    #[test]
    fn load_system_font_yields_a_shapeable_font_when_present() {
        // Forcing consumer (default gate): drive the real production path —
        // enumerate → parse → select → shape — not a call-and-ignore smoke test.
        // A CI runner with fonts installed exercises the whole chain; a font-less
        // environment surfaces `NoSystemFont` (the realgpu seam test carries the
        // pixel-level end-to-end proof). The assertions are font-agnostic (any
        // sans-serif shapes ASCII), so they are deterministic across machines —
        // sidestepping the system-font pixel-metric debt.
        match load_system_font() {
            Ok(font) => {
                assert!(font.num_glyphs() > 0, "a real font has glyphs");
                let shaped = pinion_text_font::shape_paragraph(&font, "Text", 16.0);
                assert!(
                    !shaped.glyphs.is_empty(),
                    "a selected system font must shape ASCII text into glyphs"
                );
            }
            Err(LoadFontError::NoSystemFont) => {
                // Acceptable: no usable font installed in this environment.
            }
        }
    }

    #[test]
    fn load_font_error_displays() {
        // The public error type formats for diagnostics (it is the bridge's
        // surfaced failure mode).
        assert_eq!(
            LoadFontError::NoSystemFont.to_string(),
            "no usable system font found in the OS font directories"
        );
    }

    fn noto_engine() -> SelfHostedTextEngine {
        SelfHostedTextEngine::from_font(
            Font::from_bytes(NOTO_FIXTURE.to_vec()).expect("parse NotoSans fixture"),
        )
    }

    #[test]
    fn measure_text_box_height_equals_independently_computed_hhea_line_box() {
        // R1070.1 — pins the §5.37 measure HEIGHT (the R1070 test asserted width
        // only). The expected height is recomputed here straight from the font's
        // raw hhea metrics — NOT via LineBoxMetrics — so this is an independent
        // check of the box-height formula, not a restatement of the impl. (For
        // NotoSans typo == hhea, so this hhea oracle equals the R1079 selected box;
        // `box_parity_matches_parley_metric_engine_via_skrifa` covers the typo !=
        // hhea path on NanumGothic.)
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let f = engine.font();
        let px = 20.0_f32;
        let measured = engine
            .measure_text(
                "Measure",
                &TextStyle::new().with_size_px(20),
                &[],
                None,
                false,
            )
            .expect("eligible single-line text measures via §5.37");
        assert_eq!(
            measured.line_count, 1,
            "R1344 §5.12 — the §5.37 arm is single-line by construction",
        );
        let h = measured.height;
        let upem = f64::from(f.units_per_em());
        let expected_h = (f64::from(f.ascender()) - f64::from(f.descender())
            + f64::from(f.line_gap()))
            * f64::from(px)
            / upem;
        assert!(
            (f64::from(h) - expected_h).abs() < 1e-3,
            "§5.37 box height {h} must equal the hhea line box {expected_h}"
        );
        // The paint baseline (same SSOT) sits strictly inside that box — the
        // vertical-coherence invariant, code-checked rather than prose-asserted.
        let m = LineBoxMetrics::from_font(f, px).expect("noto has a valid head");
        assert!(
            m.baseline_px > 0.0 && m.baseline_px < m.height_px,
            "paint baseline {} must lie inside the box height {}",
            m.baseline_px,
            m.height_px
        );
    }

    #[test]
    fn box_parity_matches_parley_metric_engine_via_skrifa() {
        // R1079 §5.37 cross-shaper box parity — the precondition for flipping the
        // engine on by default. parley sizes a `Normal` line box from
        // `skrifa::metrics::Metrics` as `ascent - descent + leading`
        // (parley/src/layout/data.rs: `MetricsRelative(1.0)`), so `skrifa::Metrics`
        // IS parley's metric engine and is the faithful oracle here — ZERO-FLAKE,
        // no system-font resolution, the same committed fixture both shapers see.
        use skrifa::metrics::Metrics;
        use skrifa::prelude::{FontRef, LocationRef, Size};

        // parley's line box for `bytes` in font design units (`Size::unscaled`).
        let parley_box_fu = |bytes: &[u8]| -> f32 {
            let fr = FontRef::new(bytes).expect("skrifa parses the fixture");
            let m = Metrics::new(&fr, Size::unscaled(), LocationRef::default());
            m.ascent - m.descent + m.leading
        };

        for (name, bytes, diverges) in [
            ("NotoSans", NOTO_FIXTURE, false),
            ("NanumGothic", NANUM_FIXTURE, true),
        ] {
            let font = Font::from_bytes(bytes.to_vec()).expect("parse fixture");
            let vm = font.vertical_line_metrics();
            let s537_box_fu =
                f32::from(vm.ascender) - f32::from(vm.descender) + f32::from(vm.line_gap);
            let parley_fu = parley_box_fu(bytes);
            // Font-unit boxes are integer-valued, so ±0.5 (half a unit) is far
            // tighter than the 250-unit divergence this rules out — an exact check
            // in practice while staying clippy-clean (no float `==`).
            assert!(
                (s537_box_fu - parley_fu).abs() < 0.5,
                "{name}: §5.37 line box {s537_box_fu} must equal parley's {parley_fu} (font units)"
            );

            // Regression witness: the pre-R1079 raw-hhea box diverges exactly for a
            // font that sets USE_TYPO_METRICS with typo != hhea (NanumGothic), and
            // agrees for the trivial case (NotoSans, typo == hhea).
            let hhea_box_fu = f32::from(font.ascender()) - f32::from(font.descender())
                + f32::from(font.line_gap());
            if diverges {
                assert!(
                    (hhea_box_fu - parley_fu).abs() > 0.5,
                    "{name} must exercise the typo != hhea divergence the fix closes \
                     (hhea box {hhea_box_fu} vs parley {parley_fu})"
                );
            } else {
                assert!(
                    (hhea_box_fu - parley_fu).abs() < 0.5,
                    "{name} is the trivial-parity case (typo == hhea)"
                );
            }

            // The px-scaled production path (`LineBoxMetrics::from_font`) also
            // tracks parley's metrics at a real size.
            let px = 24.0_f32;
            let fr = FontRef::new(bytes).expect("skrifa parses the fixture");
            let m_px = Metrics::new(&fr, Size::new(px), LocationRef::default());
            let parley_px = f64::from(m_px.ascent - m_px.descent + m_px.leading);
            let lbm = LineBoxMetrics::from_font(&font, px).expect("valid head");
            assert!(
                (lbm.height_px - parley_px).abs() < 0.01,
                "{name}: §5.37 box height {} must equal parley's {parley_px} at {px}px",
                lbm.height_px
            );
        }
    }

    #[test]
    fn measure_text_declines_when_a_bounded_single_line_would_soft_wrap() {
        // R1070.1 — exercises the soft-wrap decline path the R1070 test never hit.
        // A bound below the advance => None (parley wraps); a bound at/above the
        // advance => Some. This is the measure half of the shared decline mirror.
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let style = TextStyle::new().with_size_px(20);
        let advance = shape_paragraph_with_fallback(&[engine.font()], "Measure", 20.0).advance;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "advance is a small positive px value"
        )]
        let advance_u = advance.ceil() as u32;
        assert!(
            engine
                .measure_text("Measure", &style, &[], Some(advance_u / 2), false)
                .is_none(),
            "a bound below the advance must defer to parley (soft-wrap)"
        );
        assert!(
            engine
                .measure_text("Measure", &style, &[], Some(advance_u), false)
                .is_some(),
            "a bound at/above the advance fits one §5.37 line"
        );
        // Unbounded probe (taffy min/max-content) always fits a single line.
        assert!(
            engine
                .measure_text("Measure", &style, &[], None, false)
                .is_some()
        );
    }

    #[test]
    fn measure_text_defers_for_ineligible_leaves() {
        // R1070.1 — the measure arm honours the shared eligibility SSOT: every
        // ineligible shape returns None (parley), so measure and paint never split.
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let base = TextStyle::new().with_size_px(20);
        // hard line break — the arm is single-line.
        assert!(
            engine
                .measure_text("a\nb", &base, &[], None, false)
                .is_none()
        );
        // styled runs — the multi-style step.
        let run = StyleRun::new(0, 1, base.clone());
        assert!(
            engine
                .measure_text("ab", &base, &[run], None, false)
                .is_none()
        );
        // decorated — the arm draws no underline/strikethrough.
        let underlined = base
            .clone()
            .with_decoration(pinion_core::style::TextDecoration::underline());
        assert!(
            engine
                .measure_text("ab", &underlined, &[], None, false)
                .is_none()
        );
    }

    /// R1546 §5.36 §5.37 — a leaf that declares a BACKGROUND is not this arm's
    /// to paint.
    ///
    /// The arm rasterises glyphs from its own atlas and knows nothing of the
    /// band derivation, so serving such a leaf would render it without the
    /// background the scene declares — the scene saying one thing and the
    /// screen showing another, which is the single failure this eligibility
    /// SSOT exists to make impossible. Declining is always safe: the leaf
    /// renders through parley, where every leaf rendered before the arm
    /// existed.
    ///
    /// Asserted on both arms, because eligibility is their shared SSOT.
    #[test]
    fn r1546_a_leaf_declaring_a_background_is_not_this_arms_to_paint() {
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let base = TextStyle::new().with_size_px(20);

        // The baseline, so the assertions below single out the background
        // rather than some unrelated ineligibility.
        assert!(
            self_hosted_text_eligible("Modified", &base, &[], false),
            "a plain leaf must be eligible, or this test proves nothing",
        );

        let highlighted = base
            .clone()
            .with_bg_color(pinion_core::style::Color::rgb(0xFF, 0xF1, 0x76));
        assert!(
            !self_hosted_text_eligible("Modified", &highlighted, &[], false),
            "a declared background must not be eligible: this arm would paint \
             the glyphs and drop the band",
        );
        assert!(
            !engine.serves("Modified", &highlighted, &[], false),
            "nor served by the paint arm",
        );
        assert!(
            engine
                .measure_text("Modified", &highlighted, &[], None, false)
                .is_none(),
            "and the measure arm defers too — paint and measure must not split",
        );

        // A background cleared back to `None` is eligible again, so the
        // predicate keys on the declaration and not on having been touched.
        assert!(
            self_hosted_text_eligible("Modified", &highlighted.without_bg_color(), &[], false),
            "clearing it restores eligibility",
        );
    }

    /// R1551 §5.36 §5.37 — a leaf that declares a CSS `text-indent` is not this
    /// arm's to paint.
    ///
    /// The indent is a BREAKING input, not a paint offset: parley narrows the
    /// selected line's available width and shifts it, and this arm's own
    /// wrapper does neither. Serving such a leaf would paint flush what the
    /// scene declares indented, and `scene/text_blocks` would publish the
    /// declaration beside line boxes that do not honour it — the same silent
    /// divergence the background arm above exists to rule out, one round later
    /// on a new paragraph-level field.
    ///
    /// Asserted on both arms, because eligibility is their shared SSOT.
    #[test]
    fn r1551_a_leaf_declaring_a_text_indent_is_not_this_arms_to_paint() {
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let base = TextStyle::new().with_size_px(20);

        // The baseline, so the assertions below single out the indent rather
        // than some unrelated ineligibility.
        assert!(
            self_hosted_text_eligible("Indented", &base, &[], false),
            "a plain leaf must be eligible, or this test proves nothing",
        );

        let indented = base
            .clone()
            .with_text_indent(pinion_core::style::TextIndent::first_line(24));
        assert!(
            !self_hosted_text_eligible("Indented", &indented, &[], false),
            "a declared indent must not be eligible: this arm would paint the \
             paragraph flush",
        );
        assert!(
            !engine.serves("Indented", &indented, &[], false),
            "nor served by the paint arm",
        );
        assert!(
            engine
                .measure_text("Indented", &indented, &[], None, false)
                .is_none(),
            "and the measure arm defers too — paint and measure must not split",
        );

        // A hanging indent is the same declaration selecting other lines, so it
        // declines identically.
        let hanging = base
            .clone()
            .with_text_indent(pinion_core::style::TextIndent::hanging(24));
        assert!(
            !self_hosted_text_eligible("Indented", &hanging, &[], false),
            "and so does the hanging form",
        );

        // The keywords alone move nothing, so they do not cost eligibility —
        // the predicate keys on the amount, which is what a reader can see.
        let keywords_only = base
            .clone()
            .with_text_indent(pinion_core::style::TextIndent::hanging(0).with_each_line());
        assert!(
            self_hosted_text_eligible("Indented", &keywords_only, &[], false),
            "a zero amount indents no line, whatever the keywords say",
        );
    }

    /// R1505 §5.36 §5.37 — a leaf that asked to be aligned is NOT this arm's
    /// to paint, and that exclusion is now load-bearing in production.
    ///
    /// [`self_hosted_text_eligible`] has always required `Start`: the arm pens
    /// from the box left, so it would render `Center` / `End` flush left while
    /// parley slides them — the same text in two places depending on which arm
    /// happened to take it. The condition was documented from R1068 but never
    /// asserted, and the sibling `measure_text_defers_for_ineligible_leaves`
    /// covers hard breaks / styled runs / decoration and stops short of it.
    ///
    /// R1504 is what made that gap matter: it moved `ColumnLayout`'s default alignment to
    /// `Center` (the toolkit's header view default), so EVERY header label is now
    /// an aligned leaf and every one of them leaves this arm for parley. That
    /// is the correct outcome — parley is the reference, and R1505's pixel
    /// guard is what proves the alignment it applies is real — but it is a
    /// behavioural consequence of R1504 that nothing recorded, and a future
    /// relaxation of this predicate would silently paint header labels flush
    /// left again.
    ///
    /// Asserted on both arms, because eligibility is their shared SSOT: a split
    /// here is a leaf measured against one layout and painted from another.
    #[test]
    fn r1505_an_aligned_leaf_is_not_this_arms_to_paint() {
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let base = TextStyle::new().with_size_px(20);

        // The baseline: Start IS served, so the assertions below single out
        // alignment rather than some unrelated ineligibility.
        assert!(
            self_hosted_text_eligible("Modified", &base, &[], false),
            "a plain Start leaf must be eligible, or this test proves nothing",
        );
        assert!(engine.serves("Modified", &base, &[], false));
        assert!(
            engine
                .measure_text("Modified", &base, &[], None, false)
                .is_some(),
            "the measure arm takes the Start leaf",
        );

        // Every non-Start alignment defers — including `Justify`, which reads
        // like `Start` on the single line this arm renders and would be the
        // easy one to wave through.
        for align in [TextAlign::Center, TextAlign::End, TextAlign::Justify] {
            let styled = base.clone().with_align(align);
            assert!(
                !self_hosted_text_eligible("Modified", &styled, &[], false),
                "{align:?} must not be eligible: the arm pens from the box \
                 left and would paint it flush left",
            );
            assert!(
                !engine.serves("Modified", &styled, &[], false),
                "{align:?} must not be served by the paint arm",
            );
            assert!(
                engine
                    .measure_text("Modified", &styled, &[], None, false)
                    .is_none(),
                "{align:?} must defer on the measure arm too — paint and \
                 measure share one eligibility SSOT",
            );
        }
    }

    /// R1510 §5.36 §5.37 — a leaf that asked for a face this arm does not hold
    /// is not this arm's either, and for the same reason an aligned leaf is not:
    /// it would be rendered as something else and the request lost.
    ///
    /// The arm holds ONE parsed face and pens its outlines whatever the style
    /// says, so `bold` and `italic` are requests it cannot honour — and until
    /// R1510 the predicate did not read them. That was harmless only while no
    /// production leaf declared either without also declaring something else
    /// that deferred (styled runs carry weight and already defer on
    /// `runs.is_empty()`). R1510's header highlight is the first: a
    /// `Start`-aligned label bolded because the selection reached its column,
    /// which measured as served and would have painted Regular.
    ///
    /// Asserted on both arms, because eligibility is their shared SSOT.
    /// R1641.7 §5.36 §5.37 — declared TRACKING is not this arm's to paint.
    ///
    /// The eligibility predicate exists so a leaf whose declared style this arm
    /// cannot reproduce renders through the platform shaper instead of being
    /// painted wrong. Background (R1546) and text-indent (R1551) each got a
    /// clause and a test; letter spacing had neither from R47.5 until an audit
    /// went looking, and word spacing arrived at R1641.3 with the same hole.
    ///
    /// It hid because BOTH arms ignore the field: the measure reports a box at
    /// the natural advances and the paint draws at the natural advances, so
    /// they register perfectly with each other while both contradict the scene.
    /// The "measure and paint agree" contract this module is built around was
    /// satisfied and the text was still wrong.
    ///
    /// Asserted on both arms, because eligibility is their shared SSOT.
    #[test]
    fn r1641_7_declared_tracking_is_not_this_arms_to_paint() {
        use crate::layout::TextMeasure;
        use pinion_core::style::TextSpacing;
        let engine = noto_engine();
        let base = TextStyle::new().with_size_px(20);
        assert!(
            self_hosted_text_eligible("Tracked", &base, &[], false),
            "the plain leaf must be eligible, or this test proves nothing",
        );

        for (label, styled) in [
            (
                "letter px",
                base.clone().with_letter_spacing(TextSpacing::PxX100(-150)),
            ),
            (
                "letter em",
                base.clone().with_letter_spacing(TextSpacing::EmX1000(-20)),
            ),
            (
                "word px",
                base.clone().with_word_spacing(TextSpacing::PxX100(200)),
            ),
            (
                "word em",
                base.clone().with_word_spacing(TextSpacing::EmX1000(120)),
            ),
        ] {
            assert!(
                !self_hosted_text_eligible("Tracked", &styled, &[], false),
                "{label} must not be eligible: this arm never applies it",
            );
            assert!(
                !engine.serves("Tracked", &styled, &[], false),
                "{label} must not be served by the paint arm",
            );
            assert!(
                engine
                    .measure_text("Tracked", &styled, &[], None, false)
                    .is_none(),
                "{label} must defer on the measure arm too — one SSOT",
            );
        }

        // A caller who explicitly asked for NO tracking gets the arm, because
        // declining that would cost an eligible leaf and reproduce nothing.
        assert!(
            self_hosted_text_eligible(
                "Tracked",
                &base.clone().with_letter_spacing(TextSpacing::PxX100(0)),
                &[],
                false,
            ),
            "an explicit zero is the same rendering as Normal",
        );
    }

    #[test]
    fn r1510_a_face_this_arm_does_not_hold_is_not_this_arms_to_paint() {
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let base = TextStyle::new().with_size_px(20);
        assert!(
            self_hosted_text_eligible("Modified", &base, &[], false),
            "the plain leaf must be eligible, or this test proves nothing",
        );

        for (label, styled) in [
            ("bold", base.clone().with_weight(FontWeight::BOLD)),
            ("light", base.clone().with_weight(FontWeight::LIGHT)),
            ("italic", base.clone().with_style(FontStyle::Italic)),
        ] {
            assert!(
                !self_hosted_text_eligible("Modified", &styled, &[], false),
                "{label} must not be eligible: one face cannot pen it",
            );
            assert!(
                !engine.serves("Modified", &styled, &[], false),
                "{label} must not be served by the paint arm",
            );
            assert!(
                engine
                    .measure_text("Modified", &styled, &[], None, false)
                    .is_none(),
                "{label} must defer on the measure arm too — one SSOT",
            );
        }
    }

    #[test]
    fn caret_bearing_text_defers_to_parley_for_both_arms() {
        // R1072 — the R1070.1 caret contract: an editable TextField's text is
        // single-style / single-line / Start / Normal / undecorated (so it
        // would otherwise be eligible), but its caret / selection / hit-test
        // geometry comes from a separate parley shaping. The shared eligibility
        // SSOT rejects it when `caret_bearing` is set, so MEASURE defers to
        // parley — and because `paint_adapter::self_hosted_eligible` consults
        // the same predicate, PAINT defers too (both arms together).
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let style = TextStyle::new().with_size_px(20);
        // Identical content/style: eligible when not caret-bearing, deferred
        // when it is — the only difference is the marker.
        assert!(
            engine
                .measure_text("Name", &style, &[], None, false)
                .is_some(),
            "a non-caret single-style line is eligible for §5.37 measure"
        );
        assert!(
            engine
                .measure_text("Name", &style, &[], None, true)
                .is_none(),
            "the same line, caret-bearing, must defer to parley measure"
        );
        // The eligibility SSOT itself is the single decision point shared with paint.
        assert!(self_hosted_text_eligible("Name", &style, &[], false));
        assert!(!self_hosted_text_eligible("Name", &style, &[], true));
    }

    /// The two fixture families, read from the fonts' own `name` tables and
    /// asserted DIFFERENT — the premise every face test below rests on. Two
    /// fixtures that happened to declare one family would make "defers for a
    /// face it does not hold" pass without holding.
    fn fixture_families() -> (String, String) {
        let noto = noto_engine()
            .served_family()
            .expect("the NotoSans fixture declares a family");
        let nanum = Font::from_bytes(NANUM_FIXTURE.to_vec())
            .expect("parse NanumGothic fixture")
            .family_name()
            .expect("the NanumGothic fixture declares a family");
        assert_ne!(
            noto, nanum,
            "premise: the two fixtures declare different families, or a leaf \
             asking for one could be served by the other for free"
        );
        (noto, nanum)
    }

    #[test]
    fn r1479_the_arm_serves_only_the_face_it_holds() {
        // R1479 — the arm shapes with ONE font. Before this, a leaf naming a
        // family was measured and painted in whatever face the engine happened
        // to hold: a different advance, so a wrong box as well as wrong glyphs.
        use crate::layout::TextMeasure;
        let engine = noto_engine();
        let (served, other) = fixture_families();
        let base = TextStyle::new().with_size_px(20);
        let measured = |style: &TextStyle| engine.measure_text("Face", style, &[], None, false);

        // Names nothing: the arm IS the unnamed face (the R1068 premise).
        assert!(
            measured(&base).is_some(),
            "text naming no family is what the arm was always for"
        );
        // Names the face it holds: served, and case-insensitively — family
        // matching is not case-sensitive, and refusing here would push text onto
        // the platform shaper over a capital letter.
        assert!(
            measured(&base.clone().with_font_family(served.clone())).is_some(),
            "a leaf naming the engine's own family is served"
        );
        assert!(
            measured(&base.clone().with_font_family(served.to_ascii_uppercase())).is_some(),
            "and the match ignores ASCII case"
        );
        // Names a different face: deferred. This is the defect.
        assert!(
            measured(&base.clone().with_font_family(other.clone())).is_none(),
            "a leaf naming {other} must not be measured in {served}"
        );
        // A CSS generic: deferred, because only the platform database knows what
        // this host resolves the keyword to. Guessing would be the same bug.
        assert!(
            measured(
                &base
                    .clone()
                    .with_generic_family(pinion_core::style::GenericFontFamily::SansSerif)
            )
            .is_none(),
            "the engine cannot prove its face is this host's `sans-serif`"
        );
        // The paint arm asks the same question through the same method, so the
        // two arms cannot split on the face any more than on the shape.
        assert!(engine.serves("Face", &base, &[], false));
        assert!(!engine.serves(
            "Face",
            &base.clone().with_font_family(other.clone()),
            &[],
            false
        ));
        // And the face half is genuinely independent of the shape half: the
        // style-shape predicate accepts the leaf the face predicate refuses.
        assert!(self_hosted_text_eligible(
            "Face",
            &base.clone().with_font_family(other),
            &[],
            false
        ));
    }

    #[test]
    fn r1479_an_application_default_is_a_request_the_arm_must_answer() {
        // R1479 — the half R1472 built and this arm ignored. `ShellConfig::
        // with_default_font_family` makes an application's face what unset text
        // resolves to; the arm claimed every unset leaf regardless, so turning
        // the arm on silently put the declared face back out of use — for the
        // exact text (unstyled UI prose) it was declared to serve.
        use crate::layout::TextMeasure;
        let (served, other) = fixture_families();
        let unset = TextStyle::new().with_size_px(20);
        let measure = |engine: &SelfHostedTextEngine, style: &TextStyle| {
            engine
                .measure_text("Face", style, &[], None, false)
                .is_some()
        };

        // Default names another face: unset text belongs to that face, not this
        // arm's. Same leaf, same engine, opposite verdict from the declaration
        // alone.
        let foreign_default =
            noto_engine().with_default_family(Some(FontFamily::Named(other.clone().into())));
        assert!(
            !measure(&foreign_default, &unset),
            "unset text resolves to {other}, which this arm does not hold"
        );
        // Default names the arm's own face: served, so declaring a default does
        // not disable the arm — it aims it.
        let own_default =
            noto_engine().with_default_family(Some(FontFamily::Named(served.clone().into())));
        assert!(
            measure(&own_default, &unset),
            "a default naming the engine's face is the arm's own text"
        );
        // No default at all: unchanged from before R1479.
        assert!(measure(&noto_engine(), &unset));
        // The style's own family still wins over the default, both ways — the
        // arm resolves the request the way the platform shaper does rather than
        // letting a process-wide default overrule a leaf that spelled its face.
        assert!(
            measure(&foreign_default, &unset.clone().with_font_family(served)),
            "a leaf naming the engine's face is served despite a foreign default"
        );
        assert!(
            !measure(&own_default, &unset.with_font_family(other)),
            "and a leaf naming another face defers despite a matching default"
        );
    }

    // R1077 §5.37 — SelfHostedLayout is the second `TextLayout` implementor
    // (parley is the first). These exercise the §5.37 caret / hit-test /
    // line-metric geometry through the same public free functions a TextField
    // calls, proving the inverted surface is genuinely shaper-agnostic. The
    // Noto fixture shapes ASCII deterministically, so the geometry is pinned
    // exactly (ZERO-FLAKE) — independent of any system font.

    const SHL_PX: f32 = 20.0;

    fn shl_abc() -> SelfHostedLayout {
        SelfHostedLayout::shape(&noto_engine(), "abc", SHL_PX).expect("noto shapes abc")
    }

    #[test]
    fn self_hosted_layout_caret_x_matches_shaped_pen_origins() {
        // caret-before-glyph-i == that glyph's pen origin; the end caret sits at
        // the line advance. Recomputed straight from the shaper (independent of
        // the layout's stored stops), so this checks the mapping, not the impl.
        let layout = shl_abc();
        let shaped = shape_paragraph_with_fallback(&[noto_engine().font()], "abc", SHL_PX);
        for (i, g) in shaped.glyphs.iter().enumerate() {
            assert_eq!(g.cluster, i, "ascii is one glyph per byte");
            let caret = pinion_text::caret_rect_for_byte_offset(&layout, i, 1.0);
            assert!(
                (caret.x - g.x).abs() < 1e-4,
                "caret x at byte {i} ({}) must equal pen origin {}",
                caret.x,
                g.x
            );
        }
        let end = pinion_text::caret_rect_for_byte_offset(&layout, "abc".len(), 1.0);
        assert!(
            (end.x - shaped.advance).abs() < 1e-4,
            "end caret sits at the advance"
        );
        assert!((end.width - 1.0).abs() < 1e-6, "caret width passes through");
    }

    #[test]
    fn self_hosted_layout_hit_test_round_trips_and_clamps() {
        let layout = shl_abc();
        let mid = pinion_text::visual_line_metrics(&layout)[0].height * 0.5;
        for b in 0..="abc".len() {
            let x = pinion_text::caret_rect_for_byte_offset(&layout, b, 1.0).x;
            assert_eq!(
                pinion_text::byte_offset_for_point(&layout, x, mid),
                b,
                "hit-test round-trips byte {b}"
            );
        }
        // A point left of / right of the text clamps to line start / end.
        assert_eq!(pinion_text::byte_offset_for_point(&layout, -50.0, mid), 0);
        assert_eq!(
            pinion_text::byte_offset_for_point(&layout, 1.0e6, mid),
            "abc".len()
        );
    }

    #[test]
    fn self_hosted_layout_visual_line_is_single_logical_box() {
        let layout = shl_abc();
        let f = noto_engine();
        let f = f.font();
        let lines = pinion_text::visual_line_metrics(&layout);
        assert_eq!(lines.len(), 1, "a single §5.37 line yields one visual line");
        assert!(lines[0].y.abs() < 1e-6, "box top at the layout origin");
        assert!(lines[0].starts_logical_line, "it opens logical line 0");
        // Independent hhea height recomputation (mirrors the measure test).
        let upem = f64::from(f.units_per_em());
        let expected_h = (f64::from(f.ascender()) - f64::from(f.descender())
            + f64::from(f.line_gap()))
            * f64::from(SHL_PX)
            / upem;
        assert!(
            (f64::from(lines[0].height) - expected_h).abs() < 1e-3,
            "line box height {} must equal the hhea box {expected_h}",
            lines[0].height
        );
    }

    #[test]
    fn self_hosted_layout_selection_band_spans_the_range() {
        let layout = shl_abc();
        assert!(
            pinion_text::selection_rects_for_range(&layout, 2, 2).is_empty(),
            "a collapsed range has no band"
        );
        let bands = pinion_text::selection_rects_for_range(&layout, 1, 3);
        assert_eq!(bands.len(), 1, "a single line is one selection band");
        let x1 = pinion_text::caret_rect_for_byte_offset(&layout, 1, 1.0).x;
        let x3 = pinion_text::caret_rect_for_byte_offset(&layout, 3, 1.0).x;
        assert!(
            (bands[0].x - x1).abs() < 1e-6,
            "band starts at the range start"
        );
        assert!(
            (bands[0].width - (x3 - x1)).abs() < 1e-4,
            "band spans start..end"
        );
        assert!(bands[0].y.abs() < 1e-6, "band sits at the line top");
    }

    #[test]
    fn self_hosted_layout_inherits_derived_line_navigation() {
        // The derived `line_move` / `line_boundary` defaults (the trait's, which
        // parley overrides but §5.37 inherits) drive a single line: Home / Up to
        // the start, End / Down to the end.
        let layout = shl_abc();
        assert_eq!(
            pinion_text::byte_offset_for_line_boundary(&layout, 2, false),
            0,
            "Home -> line start"
        );
        assert_eq!(
            pinion_text::byte_offset_for_line_boundary(&layout, 2, true),
            3,
            "End -> line end"
        );
        assert_eq!(
            pinion_text::byte_offset_for_line_move(&layout, 1, -1, None).0,
            0,
            "Up on a single line -> start"
        );
        assert_eq!(
            pinion_text::byte_offset_for_line_move(&layout, 1, 1, None).0,
            3,
            "Down on a single line -> end"
        );
    }
}
