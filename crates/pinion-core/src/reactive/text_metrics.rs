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
use std::cell::RefCell;
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
/// provider; `None` when no shell has seeded a real provider (headless).
///
/// ★★★★★ **R1686 — it answers outside an `Owner` scope too, with the provider
/// that last measured something.** Before this round it answered `None` there,
/// and the consequence was not a missing measurement but a *disagreement*: a
/// caller falls back to its own per-character estimate, so any layout derived
/// from measured text came out one way in the paint (which runs inside the
/// scope) and another way in a pointer handler (which does not). Measured on
/// the analyser's settings form, where the estimate makes `qos.priority` 99px
/// and the shaper makes it 92px: the offered-key chips wrapped onto a different
/// number of lines in the two passes, and pressing the chip painted for one
/// configuration path added another. Both boxes were "right" and only their
/// derivation disagreed — the [[debt-paint-and-gesture-read-two-facts]] class.
///
/// The record is written by the scope that HAS the provider, which is R1684.4's
/// shape one axis over: the pass that already knows the fact writes it down,
/// rather than every screen inventing a cache
/// ([[debt-a-widget-cannot-read-its-own-size-outside-a-scope]]).
///
/// Headless is unchanged: with nothing ever seeded there is nothing to record,
/// so the answer stays `None` and a caller's fallback stays deterministic.
///
/// **The stated limit**: the record is written by a measurement, so an
/// out-of-scope call made before *any* string has been measured still answers
/// `None`. That window is between the shell's boot and its first paint, and it
/// is strictly narrower than the old behaviour rather than a new gap — the
/// alternative, recording at seed time, would be a second writer to keep in
/// step with this one, and a record that can drift from the provider is worse
/// than one that arrives a frame late.
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
    if let Some(owner) = super::owner::Owner::current() {
        let provider = TEXT_METRICS.resolve(&owner);
        let measured = provider.measure(text, style, max_width);
        if measured.is_some() {
            remember(&provider);
        }
        return measured;
    }
    LAST_MEASURING
        .with(|held| held.borrow().clone())?
        .measure(text, style, max_width)
}

thread_local! {
    /// ★★ R1686 — the provider that last answered a measurement, so a caller
    /// with no owner scope measures against the same face the paint did.
    ///
    /// See [`measured_text_extent`]'s doc for why this exists. Held as the
    /// provider rather than as a cache of answers because the question is
    /// open-ended — any string in any style — and a cache would have to be
    /// invalidated by something that knows when a face changes, which is the
    /// provider itself.
    static LAST_MEASURING: RefCell<Option<Rc<dyn TextMetrics>>> =
        const { RefCell::new(None) };
}

/// Record the provider that just measured something, if it is not the one
/// already recorded.
fn remember(provider: &Rc<dyn TextMetrics>) {
    LAST_MEASURING.with(|held| {
        let mut held = held.borrow_mut();
        if held.as_ref().is_none_or(|was| !Rc::ptr_eq(was, provider)) {
            *held = Some(Rc::clone(provider));
        }
    });
}

/// Forget the recorded provider — for a test that needs the headless answer
/// after one that seeded a face, since the record outlives an [`Owner`].
///
/// [`Owner`]: super::owner::Owner
pub fn forget_measuring_provider() {
    LAST_MEASURING.with(|held| held.borrow_mut().take());
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
