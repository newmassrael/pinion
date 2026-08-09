//! R1453 §5.36 — `TextMetrics`: the view-time "how wide is this string in this
//! style" boundary. The toolkit's `horizontalAdvance` /
//! `boundingRect(text)`.
//!
//! # Why this exists
//!
//! [`MonospaceMetrics`](super::font_metrics::MonospaceMetrics) (R1003) answers
//! one question — the cell of a *monospace* face at a size — and it was the
//! **only** measurement a view fn could make. That is enough to size a
//! [`Scene::TextGrid`](crate::scene::Scene) and nothing else: a proportional
//! face has no cell, so "how wide is `report.pdf` in the grid's font" had no
//! answer at all, and a caller that needed one had to multiply a character
//! count by a monospace cell — an **upper bound**, and only for text it had
//! forced into a monospace face to begin with (the R1452 content-hint
//! caveat this retires).
//!
//! The toolkit puts this exact capability in font metrics, and its item views
//! lean on it: `sizeHint` measures the cell's text, which is how `ResizeToContents` knows what "the
//! content's size" is. So this is the seam that makes a content-fitted column
//! possible for any face, and it is a *synchronous* answer — no
//! measure-then-settle frame, unlike a layout-pass harvest.
//!
//! # Boundary, not implementation
//!
//! Same §6.3 pattern as its monospace sibling: pinion-core declares the
//! abstract trait, `pinion-text` supplies the `LayoutCache`-backed impl, and
//! the runtime seeds it at boot — so this layer never sees parley. Measuring is
//! **deterministic** (a property of the resolved face and the style), so
//! reading it from `view` preserves the §6.3 purity / `dry_run` guarantee.
//! Headless, no provider is seeded and [`NullTextMetrics`] answers `None`, so a
//! caller falls back deterministically instead of publishing a made-up number.

use super::provider_slot::ProviderSlot;
use crate::style::TextStyle;
use std::rc::Rc;

/// R1453 §5.36 — the measured extent of a laid-out string: the toolkit's
/// `boundingRect(text)` as a size.
///
/// Both axes come from one measurement because both come from one shaping
/// pass; a caller that wants only the advance reads [`width`](Self::width).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextExtent {
    width: u32,
    height: u32,
}

impl TextExtent {
    /// Build an extent from a measured width and height in logical pixels.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// The advance width — the toolkit's `horizontalAdvance`.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// The line box height the text occupies.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// R1453 §5.36 — the shell-supplied "measure this string in this style" edge.
///
/// The peer of [`MonospaceMetrics`](super::font_metrics::MonospaceMetrics) for
/// *arbitrary* text: it takes the same [`TextStyle`] the caller is about to
/// paint with, so the measurement and the paint cannot disagree about the face,
/// the size, or the family.
pub trait TextMetrics {
    /// Measure `text` in `style`, wrapping at `max_width` when given
    /// (`None` measures the whole string on one line — the toolkit's
    /// `horizontalAdvance`). `None` when no real face resolves.
    fn measure(&self, text: &str, style: &TextStyle, max_width: Option<u32>) -> Option<TextExtent>;
}

/// R1453 §5.36 — Null Object [`TextMetrics`] (the default when no shell has
/// provided a real one: headless screenshot, RPC-driven tests, unit tests).
/// Returns `None` — there is no font context to measure — so
/// [`measured_text_extent`] is callable unconditionally.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullTextMetrics;

impl TextMetrics for NullTextMetrics {
    fn measure(
        &self,
        _text: &str,
        _style: &TextStyle,
        _max_width: Option<u32>,
    ) -> Option<TextExtent> {
        None
    }
}

/// R1453 §5.36 — the text-metrics slot: its key, its Null default and its
/// inherit verdict as one expression, in the module that owns the capability.
///
/// **`Inherited`**, for the same reason as
/// [`MONOSPACE_METRICS`](super::font_metrics::MONOSPACE_METRICS): the shell
/// DRIVES this at the root owner, seeding the `pinion-text`-backed provider
/// before any binding factory or first `view` reads it. A per-scope verdict
/// would hand a secondary window (R680: each window's `view` runs in
/// `window_owner(id).run(..)`) a freshly minted [`NullTextMetrics`], whose
/// measurement is `None` — so every content-fitted column in that window would
/// silently fall back to its caller's estimate, with no panic and no log. The
/// same silent-desync class R1362 fixed.
/// [`provider_slot_tests!`](crate::provider_slot_tests) EMITS the verdict from
/// this declaration, so it cannot be forgotten the way R1365 forgot five.
pub static TEXT_METRICS: ProviderSlot<Rc<dyn TextMetrics>> =
    ProviderSlot::inherited("__pinion.reactive.text_metrics", || {
        Rc::new(NullTextMetrics)
    });

/// R1453 §5.36 — measure `text` in `style` via the active owner scope's
/// provider; `None` when called outside an `Owner` scope or when no shell
/// seeded a real provider (headless).
///
/// `max_width` wraps the measurement at that width (the toolkit's
/// `boundingRect(rect, flags, text)`); `None` measures the string on one line
/// (`horizontalAdvance`). The call a view fn makes when it needs the size of
/// text it is about to paint:
///
/// ```ignore
/// let style = TextStyle::new().with_size_px(13);
/// let width = match measured_text_extent(label, &style, None) {
///     Some(e) => e.width(),          // measured: snug
///     None => label.len() as u32 * 8 // headless: caller's own fallback
/// };
/// ```
///
/// Graceful (`Option` throughout, no panic) rather than the strict
/// `use_repaint_sink` shape, because a producer's view fn may be exercised
/// directly in tests without an installed provider.
#[must_use]
pub fn measured_text_extent(
    text: &str,
    style: &TextStyle,
    max_width: Option<u32>,
) -> Option<TextExtent> {
    let owner = super::owner::Owner::current()?;
    TEXT_METRICS.resolve(&owner).measure(text, style, max_width)
}

#[cfg(test)]
mod tests {
    use super::super::owner::Owner;
    use super::*;

    /// A provider whose measurement is a function of the input, so a test can
    /// tell "the seam carried the real provider" from "the Null default
    /// answered".
    #[derive(Debug)]
    struct CountingMetrics;

    impl TextMetrics for CountingMetrics {
        fn measure(
            &self,
            text: &str,
            style: &TextStyle,
            max_width: Option<u32>,
        ) -> Option<TextExtent> {
            let px = style.font_size_px;
            let w = u32::try_from(text.chars().count()).unwrap_or(0) * px / 2;
            Some(TextExtent::new(max_width.map_or(w, |m| w.min(m)), px))
        }
    }

    crate::provider_slot_tests!(
        r1453_text_metrics_inherits,
        super::TEXT_METRICS,
        || -> Rc<dyn TextMetrics> { Rc::new(CountingMetrics) }
    );

    #[test]
    fn null_default_measures_none() {
        // No provider seeded: the lazy default is NullTextMetrics, and the
        // caller learns it has no measurement rather than getting a zero it
        // could mistake for "an empty string".
        let owner = Owner::new();
        let style = TextStyle::new().with_size_px(13);
        assert_eq!(
            TEXT_METRICS
                .resolve(&owner)
                .measure("report.pdf", &style, None),
            None
        );
        assert_eq!(owner.run(|| measured_text_extent("x", &style, None)), None);
    }

    #[test]
    fn the_seam_carries_the_style_and_the_wrap_bound() {
        // The measurement must be a function of BOTH the string and the style
        // the caller is about to paint with — a seam that dropped the style
        // would answer the same width for two different font sizes.
        let owner = Owner::new();
        TEXT_METRICS.provide(&owner, Rc::new(CountingMetrics));
        let small = TextStyle::new().with_size_px(10);
        let large = TextStyle::new().with_size_px(20);
        owner.run(|| {
            let a = measured_text_extent("abcd", &small, None).expect("measured");
            let b = measured_text_extent("abcd", &large, None).expect("measured");
            assert_eq!(a.width(), 20);
            assert_eq!(b.width(), 40, "the style reaches the provider");
            assert_eq!(a.height(), 10, "and both axes come back");
            // The wrap bound reaches it too.
            let capped = measured_text_extent("abcd", &large, Some(25)).expect("measured");
            assert_eq!(capped.width(), 25);
        });
    }

    #[test]
    fn measuring_outside_an_owner_scope_is_none_not_a_panic() {
        // A view fn always has a scope; a unit test exercising a producer
        // directly may not, and that must degrade rather than abort.
        let style = TextStyle::new().with_size_px(13);
        assert_eq!(measured_text_extent("abc", &style, None), None);
    }
}
