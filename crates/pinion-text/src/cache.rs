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
use pinion_core::style::{Color, FontStyle, LineHeight, TextAlign, TextStyle};
use std::borrow::Cow;
use std::num::NonZeroUsize;

/// Cache key. Captures the input that fully determines a parley
/// `Layout` output: text content, style, and optional max width (the
/// line break point in pixels). `max_width = None` means no wrap
/// (single line / unbounded).
#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    style: TextStyle,
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
        let key = LayoutKey {
            text: text.to_owned(),
            style: style.clone(),
            max_width,
        };
        if !self.inner.contains(&key) {
            let layout = self.shape(text, style, max_width);
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

    fn shape(&mut self, text: &str, style: &TextStyle, max_width: Option<u32>) -> Layout {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        // u32 → f32: font_size_px fits f32 mantissa losslessly up to
        // 2^24 px, which is far beyond any realistic UI font size.
        #[allow(
            clippy::cast_precision_loss,
            reason = "font_size_px <= 2^24 px in practice"
        )]
        let font_size = style.font_size_px as f32;
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(StyleProperty::Brush(style.fg_color));
        // R47.6 §5.36 — pinion-core's Figma-fidelity TextStyle fields
        // map 1:1 into parley StyleProperty. Each push_default sets
        // the base run style for the whole text (range overrides land
        // in R47.x widget catalog).
        builder.push_default(StyleProperty::FontWeight(parley::FontWeight::new(
            f32::from(style.font_weight.0),
        )));
        builder.push_default(StyleProperty::FontStyle(map_font_style(style.font_style)));
        builder.push_default(StyleProperty::LineHeight(map_line_height(style.line_height)));
        // letter_spacing i32 → f32 px (signed). Realistic UI ranges
        // (-32..=32) fit f32 exactly; the cast is loss-free.
        #[allow(
            clippy::cast_precision_loss,
            reason = "letter_spacing |v| <= 2^24 in practice"
        )]
        let letter_spacing_px = style.letter_spacing as f32;
        builder.push_default(StyleProperty::LetterSpacing(letter_spacing_px));
        builder.push_default(StyleProperty::Underline(style.decoration.underline));
        builder.push_default(StyleProperty::Strikethrough(style.decoration.strikethrough));
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
            builder.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(
                families,
            ))));
        }
        let mut layout = builder.build(text);
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
}
