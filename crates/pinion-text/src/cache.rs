//! R47.2 §5.36 — [`LayoutCache`]: LRU-bounded cache from
//! `(text, style, max_width)` to a shaped [`Layout`].
//!
//! Realizes the §5.36 R47.2 `LayoutCache` output: text content that does
//! not change frame-to-frame (button labels, static UI strings) shapes
//! once and reuses on subsequent frames. Per-frame parley work is
//! reduced to a hashmap probe in the steady state.
//!
//! `FontContext` and `LayoutContext` live inside the cache so callers
//! never own parley state directly. The cache is intentionally not
//! `Send` / `Sync` — parley's contexts hold single-thread state that
//! aligns with §6.3 view-fn purity (sync, single-thread). Per-thread
//! caches are the textbook pattern; multi-thread shaping is R47.x
//! carry.

use crate::layout::Layout;
use lru::LruCache;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, GenericFamily,
    LayoutContext, LineHeight as ParleyLineHeight, StyleProperty,
};
use pinion_core::scene::StyleRun;
use pinion_core::style::{Color, FontStyle, LineHeight, TextAlign, TextStyle};
use std::borrow::Cow;
use std::num::NonZeroUsize;

/// Cache key. Captures the input the cache uses to identify a parley
/// `Layout` output: text content, style, and optional max width (the
/// line break point in pixels). `max_width = None` means no wrap
/// (single line / unbounded).
///
/// # Conservative key (R587 §5.36)
///
/// The key includes `TextStyle` *in full*, which means `fg_color`
/// participates via `Hash + Eq`. Strictly, `fg_color` lands in parley as
/// `StyleProperty::Brush`, which is paint metadata: it carries through
/// to glyph runs but does not influence shape (glyph cluster
/// composition, advances, line breaks). So the current key is
/// *over-specified* — two layouts that differ only in `fg_color` are
/// shape-identical, yet the cache treats them as distinct entries.
///
/// Effect during the R57.X.theme-fade cross-fade: the active palette's
/// `fg_color` is in-flight (linear-space spring lerp toward the target
/// for ~200ms / ~12 frames @ 60fps), so each frame's view-fn emits a
/// `TextStyle` with a fresh `fg_color` and the cache treats every frame
/// as a miss — parley re-shapes per frame for the duration of the
/// fade.
///
/// Round 587 measurement (`hello-textfield` single-line label) places
/// the per-frame shape cost at ~1-2 ms, and a 12-frame fade therefore
/// burns ~6-12% of the 60fps frame budget — below the visible jitter
/// threshold for this widget. The same arithmetic with a long /
/// multi-line buffer (a multiline editor consumer, R47.x carry) would
/// push per-frame shape past 5-10 ms and into visible regression
/// territory, so the over-specification is a *latent* perf hazard,
/// not a current one.
///
/// # Why not split now
///
/// The textbook substrate fix is to separate *shape-determinants*
/// (`font_family`, `font_size_px`, `font_weight`, `font_style`,
/// `letter_spacing`, `line_height`, `text_align`) from
/// *paint-metadata* (`fg_color`, decoration colors), key only on the
/// former, and apply the latter as a post-cache brush override on the
/// returned `Layout`. That mirrors the parley boundary
/// (`StyleProperty::Brush` is the canonical paint-only property) and
/// trades the current 1-call API for either a `TextStyle` type-split
/// or a `layout_with_brush(...)` overload. It also widens the public
/// surface — both `pinion-core::style::TextStyle` and every consumer
/// that shapes via the cache.
///
/// Rule of Three discipline (cf. `abstraction-needs-second-consumer`):
/// the only consumer that exercises in-flight `fg_color` today is
/// the R57.X.theme-fade animated palette, and the measured impact is
/// well below the visible threshold for `hello-textfield`. Defer the
/// split until a second consumer (a multi-line text input / paragraph
/// editor) crosses the visible threshold; carry tracked in
/// `[[r57-x-theme-fade-substrate]]` Rule of Three list. See
/// `different_fg_color_creates_new_entry` for the regression that
/// pins the current behavior so the split lands deliberately.
/// # R713 styled runs
///
/// `runs` carries the [`StyleRun`] spans (rich / multi-style text). It
/// participates in the key via `Hash + Eq`: a node whose runs differ
/// (a highlight toggling a span's weight) is a distinct cache entry, so
/// the layout re-shapes when the styling changes. The common
/// single-style node has `runs.is_empty()` and the vector contributes
/// nothing to the hash beyond its zero length.
#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    style: TextStyle,
    runs: Vec<StyleRun>,
    max_width: Option<u32>,
}

/// LRU-bounded cache over [`Layout`] values keyed by
/// `(text, style, max_width)`. Construct via [`LayoutCache::new`]
/// (default capacity) or [`LayoutCache::with_capacity`] for explicit
/// sizing.
pub struct LayoutCache {
    inner: LruCache<LayoutKey, Layout>,
    font_cx: FontContext,
    layout_cx: LayoutContext<Color>,
}

impl LayoutCache {
    /// Default cache capacity (cached layouts). A `NonZeroUsize`
    /// compile-time constant so [`LayoutCache::new`] needs no runtime
    /// unwrap.
    pub const DEFAULT_CAPACITY: NonZeroUsize = NonZeroUsize::new(256)
        .expect("256 is non-zero");

    /// Construct a cache with `capacity` slots. Use [`LayoutCache::new`]
    /// for the default.
    #[must_use]
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        Self {
            inner: LruCache::new(capacity),
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }

    /// Construct a cache with [`Self::DEFAULT_CAPACITY`] slots.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// Shape `text` with `style` and (optionally) wrap at `max_width`
    /// pixels. Returns a reference to the cached Layout and promotes
    /// it to most-recently-used. Subsequent calls with the same inputs
    /// return the same entry without re-running parley.
    ///
    /// # Panics
    ///
    /// Never panics in practice. The internal `expect()` upholds the
    /// `LruCache` invariant that a key just inserted via `put` is
    /// retrievable via `get` on the same call sequence; an LRU
    /// implementation violating that would be a backing-library bug.
    pub fn layout(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<u32>,
    ) -> &Layout {
        self.layout_with_runs(text, style, &[], max_width)
    }

    /// R713 §5.36 — shape `text` with a base `style` plus styled-run
    /// overrides and (optionally) wrap at `max_width` pixels.
    ///
    /// `runs` is the [`StyleRun`] list: the base `style` is pushed as
    /// the default, then each run's fully-resolved style is pushed over
    /// its UTF-8 byte range (later runs win on overlap, matching
    /// parley's range resolution). `runs.is_empty()` is exactly
    /// [`Self::layout`] — the single-style fast path — and produces a
    /// byte-identical cache key, so pre-R713 callers are unaffected.
    ///
    /// # Panics
    ///
    /// Never panics in practice — same `LruCache` invariant as
    /// [`Self::layout`].
    pub fn layout_with_runs(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> &Layout {
        let key = LayoutKey {
            text: text.to_owned(),
            style: style.clone(),
            runs: runs.to_vec(),
            max_width,
        };
        if !self.inner.contains(&key) {
            let layout = self.shape(text, style, runs, max_width);
            self.inner.put(key.clone(), layout);
        }
        self.inner
            .get(&key)
            .expect("entry just inserted on cache miss")
    }

    /// Number of currently cached entries (test + diagnostic surface).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the cache holds zero entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn shape(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> Layout {
        // R51.31 §5.37.4 — UAX #9 L4 mirroring is applied here, at the
        // cache boundary, so paint_adapter sees `cache.layout(raw)` and
        // the single LRU entry covers both the mirror substitution and
        // the parley shape pass. `mirror_paired_brackets` short-circuits
        // to `Cow::Borrowed` for LTR / bracket-free content (the common
        // case for static UI labels), so the integration is allocation-
        // free on the hot path. Pre-R51.31 the call lived in
        // `paint_adapter::paint_text` and ran per-frame even on shape
        // cache hits; folding it in here lets a cache hit skip both
        // the BIDI pipeline and the shape engine.
        let mirrored = pinion_text_unicode::bidi::mirror_paired_brackets(text);
        let shape_input = mirrored.as_ref();
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, shape_input, 1.0, true);
        // R47.6 §5.36 — the base style is pushed as the run default
        // (the whole-string style). R713 §5.36 — each StyleRun then
        // pushes its fully-resolved style over its UTF-8 byte range;
        // parley resolves overlaps last-push-wins, so list order is the
        // run priority. `runs.is_empty()` collapses to the pre-R713
        // default-only path.
        for prop in style_properties(style) {
            builder.push_default(prop);
        }
        for run in runs {
            let range = run.start as usize..run.end as usize;
            for prop in style_properties(&run.style) {
                builder.push(prop, range.clone());
            }
        }
        let mut layout = builder.build(shape_input);
        #[allow(
            clippy::cast_precision_loss,
            reason = "max_width <= 2^24 px in practice"
        )]
        let break_at = max_width.map(|w| w as f32);
        layout.break_all_lines(break_at);
        layout.align(map_text_align(style.text_align), AlignmentOptions::default());
        layout
    }
}

/// R713 §5.36 — lower a [`TextStyle`] to the parley [`StyleProperty`]
/// set, ready to push either as the run default or over a styled-run
/// range. Pulling this out of `shape` lets the base style and every
/// [`StyleRun`] share one mapping — the styled-run path is exactly the
/// single-style path applied over a sub-range.
///
/// The returned properties own their data (`'static`), so the same
/// list pushes via `push_default` (range = whole text) or `push(_,
/// range)` (a run) without lifetime juggling.
fn style_properties(style: &TextStyle) -> Vec<StyleProperty<'static, Color>> {
    // u32 → f32: font_size_px fits f32 mantissa losslessly up to 2^24
    // px, far beyond any realistic UI font size.
    #[allow(
        clippy::cast_precision_loss,
        reason = "font_size_px <= 2^24 px in practice"
    )]
    let font_size = style.font_size_px as f32;
    // letter_spacing i32 → f32 px (signed). Realistic UI ranges
    // (-32..=32) fit f32 exactly; the cast is loss-free.
    #[allow(
        clippy::cast_precision_loss,
        reason = "letter_spacing |v| <= 2^24 in practice"
    )]
    let letter_spacing_px = style.letter_spacing as f32;
    let mut props = vec![
        StyleProperty::FontSize(font_size),
        StyleProperty::Brush(style.fg_color),
        StyleProperty::FontWeight(parley::FontWeight::new(f32::from(style.font_weight.0))),
        StyleProperty::FontStyle(map_font_style(style.font_style)),
        StyleProperty::LineHeight(map_line_height(style.line_height)),
        StyleProperty::LetterSpacing(letter_spacing_px),
        StyleProperty::Underline(style.decoration.underline),
        StyleProperty::Strikethrough(style.decoration.strikethrough),
    ];
    // R47.6 — pinned font family override; `None` keeps parley's
    // default font stack (system fallback). When `Some`, route the
    // requested family through `FontFamily::Named` and append the
    // GenericFamily::SansSerif fallback so a missing name does not
    // produce a "tofu" run.
    if let Some(family) = style.font_family.as_deref() {
        let families: Vec<FontFamilyName<'static>> = vec![
            FontFamilyName::Named(Cow::Owned(family.to_owned())),
            FontFamilyName::Generic(GenericFamily::SansSerif),
        ];
        props.push(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(families))));
    }
    props
}

/// R47.6 — pinion `FontStyle` → parley `FontStyle`. `Oblique` widens
/// from `Option<i16>` (pinion: Hash-safe) to `Option<f32>` (parley:
/// `slnt` axis).
fn map_font_style(style: FontStyle) -> parley::FontStyle {
    match style {
        FontStyle::Italic => parley::FontStyle::Italic,
        FontStyle::Oblique(angle) => parley::FontStyle::Oblique(angle.map(f32::from)),
        // Normal + any future #[non_exhaustive] variant.
        _ => parley::FontStyle::Normal,
    }
}

/// R47.6 — pinion `LineHeight` → parley `LineHeight`. `MultiplierX100`
/// widens from fixed-point u16 (pinion: Hash-safe) to `f32`.
fn map_line_height(line_height: LineHeight) -> ParleyLineHeight {
    match line_height {
        LineHeight::Px(px) => {
            #[allow(
                clippy::cast_precision_loss,
                reason = "line_height px <= 2^24 in practice"
            )]
            ParleyLineHeight::Absolute(px as f32)
        }
        LineHeight::MultiplierX100(m) => {
            ParleyLineHeight::FontSizeRelative(f32::from(m) / 100.0)
        }
        // Normal + any future #[non_exhaustive] variant → parley
        // default (MetricsRelative(1.0)).
        _ => ParleyLineHeight::MetricsRelative(1.0),
    }
}

/// R47.6 — pinion `TextAlign` → parley `Alignment`. Both enums share
/// the writing-mode-aware Start/End shape; Center / Justify map 1:1.
fn map_text_align(align: TextAlign) -> Alignment {
    match align {
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        TextAlign::Justify => Alignment::Justify,
        // Start + any future #[non_exhaustive] variant.
        _ => Alignment::Start,
    }
}


impl Default for LayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(size: u32) -> TextStyle {
        // TextStyle is #[non_exhaustive] (forward-compat for R47.x
        // font_family / weight / decoration). Build through the public
        // constructor then override the size.
        let mut s = TextStyle::new();
        s.font_size_px = size;
        s
    }

    #[test]
    fn layout_produces_at_least_one_line() {
        let mut cache = LayoutCache::new();
        let layout = cache.layout("Hello", &style(16), None);
        assert!(layout.lines().count() >= 1);
    }

    #[test]
    fn repeated_layout_hits_cache() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("Cached", &s, None);
        assert_eq!(cache.len(), 1);
        let _ = cache.layout("Cached", &s, None);
        assert_eq!(cache.len(), 1, "second call should hit cache");
    }

    #[test]
    fn different_text_creates_new_entry() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("foo", &s, None);
        let _ = cache.layout("bar", &s, None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn different_max_width_creates_new_entry() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("text", &s, Some(100));
        let _ = cache.layout("text", &s, Some(200));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn different_style_creates_new_entry() {
        let mut cache = LayoutCache::new();
        let _ = cache.layout("text", &style(16), None);
        let _ = cache.layout("text", &style(24), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn capacity_evicts_oldest() {
        let cap = NonZeroUsize::new(2).expect("nonzero");
        let mut cache = LayoutCache::with_capacity(cap);
        let s = style(16);
        let _ = cache.layout("a", &s, None);
        let _ = cache.layout("b", &s, None);
        let _ = cache.layout("c", &s, None);
        assert_eq!(cache.len(), 2, "oldest entry evicted at capacity");
    }

    /// R587 §5.36 — pins the current `LayoutKey` shape: `fg_color`
    /// participates in the cache key via `TextStyle`'s `Hash + Eq`.
    /// A future shape-determinants / paint-metadata split (carried in
    /// `LayoutKey`'s doc and `[[r57-x-theme-fade-substrate]]`) will
    /// flip this assertion to `1`, at which point it should be
    /// rewritten — not silently relaxed — so the split lands as a
    /// deliberate behavior change rather than a quiet regression.
    #[test]
    fn different_fg_color_creates_new_entry() {
        let mut cache = LayoutCache::new();
        let red = TextStyle::new().with_size_px(16).with_fg(Color::rgb(255, 0, 0));
        let blue = TextStyle::new().with_size_px(16).with_fg(Color::rgb(0, 0, 255));
        let _ = cache.layout("text", &red, None);
        let _ = cache.layout("text", &blue, None);
        assert_eq!(
            cache.len(),
            2,
            "fg_color is in LayoutKey today; paint-style split is Rule of Three carry",
        );
    }

    /// R713 §5.36 — `layout_with_runs(.., &[], ..)` is the single-style
    /// fast path: it produces the same cache key as `layout(..)`, so a
    /// styled call with no runs hits the entry a plain call created.
    #[test]
    fn empty_runs_match_single_style_entry() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("text", &s, None);
        assert_eq!(cache.len(), 1);
        let _ = cache.layout_with_runs("text", &s, &[], None);
        assert_eq!(cache.len(), 1, "empty runs reuse the single-style entry");
    }

    /// R713 §5.36 — adding a styled run changes the key, so a run-styled
    /// node is a distinct cache entry from the same text single-styled.
    #[test]
    fn styled_runs_create_distinct_entry() {
        let mut cache = LayoutCache::new();
        let base = style(16);
        let red = TextStyle::new().with_size_px(16).with_fg(Color::rgb(255, 0, 0));
        let _ = cache.layout("AB", &base, None);
        let _ = cache.layout_with_runs("AB", &base, &[StyleRun::new(0, 1, red)], None);
        assert_eq!(cache.len(), 2, "a styled run is a distinct cache key");
    }

    /// R713 §5.36 — the substrate guarantee: a styled run actually
    /// reaches parley and restyles its byte range. A larger font size
    /// on the first character forces parley to split the text into two
    /// glyph runs with distinct metrics, observable in the shaped
    /// `Layout` (the same per-run data the paint adapter reads).
    #[test]
    fn styled_run_splits_and_restyles() {
        use parley::PositionedLayoutItem;
        let mut cache = LayoutCache::new();
        let base = style(16);
        let big = style(40);
        let runs = vec![StyleRun::new(0, 1, big)];
        let layout = cache.layout_with_runs("AB", &base, &runs, None);
        let mut sizes: Vec<f32> = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    sizes.push(run.run().font_size());
                }
            }
        }
        assert!(
            sizes.iter().any(|s| (*s - 40.0).abs() < 0.5),
            "the styled run shaped at 40px: {sizes:?}",
        );
        assert!(
            sizes.iter().any(|s| (*s - 16.0).abs() < 0.5),
            "the base run shaped at 16px: {sizes:?}",
        );
    }

    /// R713 §5.36 — a styled run carries its own brush through to the
    /// glyph run, which is what the paint adapter lowers to a per-run
    /// Vello brush. Distinct adjacent brushes split into distinct
    /// glyph runs.
    #[test]
    fn styled_run_carries_per_run_brush() {
        use parley::PositionedLayoutItem;
        let mut cache = LayoutCache::new();
        let base = TextStyle::new().with_size_px(16).with_fg(Color::rgb(0, 0, 0));
        let red = TextStyle::new().with_size_px(16).with_fg(Color::rgb(255, 0, 0));
        let runs = vec![StyleRun::new(0, 1, red.clone())];
        let layout = cache.layout_with_runs("AB", &base, &runs, None);
        let mut brushes: Vec<Color> = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    brushes.push(run.style().brush);
                }
            }
        }
        assert!(brushes.contains(&red.fg_color), "red run brush present: {brushes:?}");
        assert!(brushes.contains(&base.fg_color), "base brush present: {brushes:?}");
    }
}
