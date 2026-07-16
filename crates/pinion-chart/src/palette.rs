//! Categorical series colours.
//!
//! Charting's second missing half (after the drawing primitive): a
//! per-series colour scale that stays distinguishable for colour-blind
//! viewers and reads on both light and dark surfaces. The default is the
//! Okabe-Ito qualitative palette — the de-facto colour-blind-safe
//! categorical set (Okabe & Ito, 2008) — re-ordered so the hues with the
//! strongest dual-theme contrast come first, and with canonical black
//! swapped for a mid slate so a series is never invisible on a dark
//! surface.

use pinion_core::style::Color;

/// The Okabe-Ito qualitative palette, dual-theme ordered.
///
/// Faithful to the eight Okabe-Ito hues except the final entry: the
/// canonical palette's eighth colour is pure black, which vanishes on a
/// dark surface, so it is replaced with a neutral slate that reads on
/// both. The first six are the high-contrast hues; yellow (index 6) is
/// kept but demoted because it is weak on light surfaces.
const OKABE_ITO: [Color; 8] = [
    Color::rgb(0x00, 0x72, 0xB2), // blue
    Color::rgb(0xE6, 0x9F, 0x00), // orange
    Color::rgb(0x00, 0x9E, 0x73), // bluish green
    Color::rgb(0xD5, 0x5E, 0x00), // vermillion
    Color::rgb(0x56, 0xB4, 0xE9), // sky blue
    Color::rgb(0xCC, 0x79, 0xA7), // reddish purple
    Color::rgb(0xF0, 0xE4, 0x42), // yellow
    Color::rgb(0x6E, 0x77, 0x81), // slate (replaces canonical black)
];

/// Neutral fallback returned when a palette is empty — never invisible on
/// either theme.
const FALLBACK: Color = Color::rgb(0x80, 0x80, 0x80);

/// An ordered set of categorical colours, indexed per series and wrapping
/// when there are more series than colours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoricalPalette {
    colors: Vec<Color>,
}

impl CategoricalPalette {
    /// The dual-theme-ordered Okabe-Ito palette (8 colours). This is the
    /// [`Default`].
    #[must_use]
    pub fn okabe_ito() -> Self {
        Self {
            colors: OKABE_ITO.to_vec(),
        }
    }

    /// A palette from an explicit colour list. An empty list makes
    /// [`color`](Self::color) return the neutral fallback.
    #[must_use]
    pub fn from_colors(colors: Vec<Color>) -> Self {
        Self { colors }
    }

    /// The colour for series `index`, wrapping modulo the palette length.
    /// An empty palette returns a neutral fallback.
    #[must_use]
    pub fn color(&self, index: usize) -> Color {
        if self.colors.is_empty() {
            return FALLBACK;
        }
        self.colors[index % self.colors.len()]
    }

    /// Number of distinct colours before wrapping.
    #[must_use]
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// Whether the palette carries no colours (every lookup falls back).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }
}

impl Default for CategoricalPalette {
    fn default() -> Self {
        Self::okabe_ito()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_eight_okabe_ito_colours() {
        let p = CategoricalPalette::default();
        assert_eq!(p.len(), 8);
        assert!(!p.is_empty());
        assert_eq!(p.color(0), Color::rgb(0x00, 0x72, 0xB2));
    }

    #[test]
    fn index_wraps_modulo_length() {
        let p = CategoricalPalette::okabe_ito();
        assert_eq!(p.color(0), p.color(8));
        assert_eq!(p.color(3), p.color(11));
    }

    #[test]
    fn first_six_colours_are_all_distinct() {
        let p = CategoricalPalette::okabe_ito();
        for i in 0..6 {
            for j in (i + 1)..6 {
                assert_ne!(p.color(i), p.color(j), "series {i} vs {j} distinct");
            }
        }
    }

    #[test]
    fn dual_theme_ordering_never_leads_with_black() {
        // The canonical Okabe-Ito black is dropped entirely; no colour is
        // pure black (invisible on dark) or pure white (invisible on light).
        let p = CategoricalPalette::okabe_ito();
        for i in 0..p.len() {
            assert_ne!(p.color(i), Color::rgb(0, 0, 0));
            assert_ne!(p.color(i), Color::rgb(0xFF, 0xFF, 0xFF));
        }
    }

    #[test]
    fn empty_palette_returns_fallback() {
        let p = CategoricalPalette::from_colors(Vec::new());
        assert!(p.is_empty());
        assert_eq!(p.color(0), FALLBACK);
        assert_eq!(p.color(5), FALLBACK);
    }
}
