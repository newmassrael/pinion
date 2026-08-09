//! R1003 §5.36 — [`LayoutCacheMonospaceMetrics`]: the parley-backed
//! [`MonospaceMetrics`] provider.
//!
//! This is the concrete impl of the pinion-core [`MonospaceMetrics`] boundary
//! (the `Executor` / `RepaintSink` boundary-trait pattern: core declares the
//! trait, this crate supplies the parley-backed impl). The shell seeds one on
//! the root `Owner` at boot so the pure, sync `view` fn can derive a
//! font-correct [`CellMetric`] for a `Scene::TextGrid` via
//! [`measured_monospace_cell`](pinion_core::measured_monospace_cell) without
//! owning a `FontContext`.

use crate::cache::LayoutCache;
use pinion_core::style::TextStyle;
use pinion_core::{CellMetric, MonospaceMetrics, TextExtent, TextMetrics};
use std::cell::RefCell;

/// R1003 §5.36 — a [`MonospaceMetrics`] provider backed by a dedicated
/// [`LayoutCache`].
///
/// Holds its own cache (not the paint cache) so measurement never entangles
/// with rendering. The interior [`RefCell`] bridges the trait's `&self` to
/// [`LayoutCache::measure_monospace_cell`]'s `&mut self`; single-thread (the
/// view runs on the UI thread), so a `RefCell` suffices — no lock. Repeated
/// measurements at the same size hit the cache's LRU.
#[derive(Default)]
pub struct LayoutCacheMonospaceMetrics {
    cache: RefCell<LayoutCache>,
}

impl LayoutCacheMonospaceMetrics {
    /// Construct a provider with a fresh measurement cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: RefCell::new(LayoutCache::new()),
        }
    }
}

impl MonospaceMetrics for LayoutCacheMonospaceMetrics {
    fn monospace_cell(&self, font_size_px: u32) -> Option<CellMetric> {
        self.cache.borrow_mut().measure_monospace_cell(font_size_px)
    }
}

/// R1453 §5.36 — a [`TextMetrics`] provider backed by a dedicated
/// [`LayoutCache`]: the parley-backed answer to "how wide is this string in
/// this style", the toolkit's `horizontalAdvance` / `boundingRect`.
///
/// Separate from [`LayoutCacheMonospaceMetrics`] rather than folded into it:
/// the two answer different questions (one cell of a fixed-pitch face at a
/// size, versus one arbitrary string in an arbitrary style) and a caller wants
/// one or the other, so a merged trait would force every consumer to depend on
/// a capability it does not use. They share the pattern, not the surface.
///
/// Holds its own cache (not the paint cache) so measurement never entangles
/// with rendering; the interior [`RefCell`] bridges the trait's `&self` to
/// [`LayoutCache::layout`]'s `&mut self`. Repeated measurements of the same
/// (text, style, width) hit the cache's LRU, which is what makes measuring
/// every cell of a grid in a view fn affordable.
#[derive(Default)]
pub struct LayoutCacheTextMetrics {
    cache: RefCell<LayoutCache>,
}

impl LayoutCacheTextMetrics {
    /// Construct a provider with a fresh measurement cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: RefCell::new(LayoutCache::new()),
        }
    }
}

impl TextMetrics for LayoutCacheTextMetrics {
    fn measure(&self, text: &str, style: &TextStyle, max_width: Option<u32>) -> Option<TextExtent> {
        let mut cache = self.cache.borrow_mut();
        let layout = cache.layout(text, style, max_width);
        // parley reports fractional advances; a size hint that rounded DOWN
        // would clip the last glyph, so both axes round up — the same
        // direction `measure_monospace_cell` takes for its cell height. The
        // `max(0.0)` is what makes the cast total, matching that function's
        // idiom rather than inventing a second one.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a laid-out extent is small positive px; ceil then floored at 0 \
                      leaves the integer part exact, as measure_monospace_cell does"
        )]
        let (w, h) = (
            layout.width().ceil().max(0.0) as u32,
            layout.height().ceil().max(0.0) as u32,
        );
        Some(TextExtent::new(w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use std::rc::Rc;

    /// The provider, seeded on an `Owner`, delivers the *same* measurement the
    /// cache produces directly — proving the seam carries the real metric into
    /// an owner scope (where a pure `view` reads it). The forcing consumer of
    /// the R1003 seam (it would catch a regression in the wiring or the bridge).
    #[test]
    fn seam_delivers_measured_cell_into_owner_scope() {
        // R1573 — the host path deliberately: this measures the GENERIC
        // `monospace` family through the platform, which is what the seam under
        // test carries. Both sides of the equality read the same host, so the
        // assertion is a comparison rather than an absolute metric.
        let direct = LayoutCache::new()
            .measure_monospace_cell(32)
            .expect("32px monospace measures");

        let owner = Owner::new();
        pinion_core::MONOSPACE_METRICS.provide(&owner, Rc::new(LayoutCacheMonospaceMetrics::new()));
        let via_seam = owner.run(|| pinion_core::measured_monospace_cell(32));

        assert_eq!(
            via_seam,
            Some(direct),
            "seam must deliver the measured cell"
        );
        assert!(
            direct.cell_w() < direct.cell_h(),
            "measured cell is taller than wide"
        );
    }

    /// R1453 — the same proof for arbitrary text: the seam carries the real
    /// shaper's answer into an owner scope, and the answer is a function of the
    /// string (a provider that ignored its input would tie every column of a
    /// content-fitted grid to the same width).
    #[test]
    fn text_seam_delivers_a_real_per_string_measurement() {
        let style = TextStyle::new().with_size_px(13);
        let owner = Owner::new();
        pinion_core::TEXT_METRICS.provide(&owner, Rc::new(LayoutCacheTextMetrics::new()));

        let (short, long, empty) = owner.run(|| {
            (
                pinion_core::measured_text_extent("Type", &style, None).expect("measures"),
                pinion_core::measured_text_extent("report.pdf", &style, None).expect("measures"),
                pinion_core::measured_text_extent("", &style, None).expect("measures"),
            )
        });
        assert!(
            short.width() < long.width(),
            "a longer string measures wider: {} vs {}",
            short.width(),
            long.width()
        );
        assert_eq!(empty.width(), 0, "and an empty string has no advance");
        assert!(short.height() > 0, "both axes come back measured");

        // The style reaches the shaper: the same string at twice the size is
        // wider. A seam that dropped the style would answer identically.
        let big = TextStyle::new().with_size_px(26);
        let bigger = owner
            .run(|| pinion_core::measured_text_extent("report.pdf", &big, None))
            .expect("measures");
        assert!(
            bigger.width() > long.width(),
            "26px is wider than 13px: {} vs {}",
            bigger.width(),
            long.width()
        );

        // Rounding is UP: a hint that rounded down would clip the last glyph.
        // Recover the fractional width the same way the provider does and check
        // the reported one is its ceiling.
        // R1573 — the host path deliberately: `long.width()` came from the
        // production provider, which shapes through the PLATFORM fonts, so the
        // raw advance it is compared against has to come from the same place.
        // Shaping one side with the deterministic fixture and the other with
        // the host compares two DIFFERENT fonts — which is exactly how this
        // passed locally and failed on CI (65 vs 62.31).
        let mut host = LayoutCache::new();
        let raw = f64::from(host.layout("report.pdf", &style, None).width());
        let reported = f64::from(long.width());
        assert!(
            reported >= raw && reported < raw + 1.0,
            "the extent is the CEILING of the shaped advance: {reported} vs {raw}"
        );
    }
}
