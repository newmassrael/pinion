//! R47.2 §5.36 — `Layout` type alias: parley `Layout` parameterized by
//! `pinion_core::style::Color` as the brush. The scene tree's
//! [`pinion_core::style::TextStyle::fg_color`] flows through parley's
//! [`parley::StyleProperty::Brush`] unchanged into each
//! [`parley::PositionedLayoutItem::GlyphRun`].
//!
//! Backend adapters (the `paint_adapter::Text` arm for Vello in R47.3,
//! future Headless / TUI paths) iterate `Layout::lines()` and emit
//! per-backend draw calls per `GlyphRun`. parley's `Brush` trait is a
//! blanket-impl on `Clone + PartialEq + Default + Debug`, so `Color`
//! satisfies it without an explicit impl block.

use pinion_core::style::Color;

/// Positioned glyph runs produced by parley shaping. The §5.36 R47
/// canonical output of [`crate::LayoutCache::layout`].
pub type Layout = parley::Layout<Color>;
