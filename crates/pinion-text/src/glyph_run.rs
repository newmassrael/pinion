//! R1531 §5.36 — [`PositionedRun`]: the draw list a painter replays.
//!
//! # Why a shaped layout is not yet a draw list
//!
//! [`crate::LayoutCache`] caches what parley *shaped*. A painter cannot hand a
//! [`Layout`](crate::Layout) to a rasteriser: it has to walk
//! `lines() -> items() -> GlyphRun`, read each run's font / size / brush, and
//! run [`positioned_glyphs`](parley::GlyphRun::positioned_glyphs), which
//! accumulates each glyph's pen advance into an absolute position. That walk is
//! a **pure function of the layout** — the same layout yields the same runs
//! forever — and until R1531 it ran on every paint of every text leaf.
//!
//! It is not a rounding error. Measured (release, 1,200 text leaves, the shape
//! cache warm, one frame that re-encodes — i.e. a §5.16 fragment-cache miss,
//! which is what an ordinary keystroke or scroll produces):
//!
//! | | per frame |
//! |---|---:|
//! | walking parley and encoding as we go | **1,596 µs** |
//! | encoding a pre-derived draw list | **555 µs** |
//! | *of which* the shape-cache lookups alone | 84 µs |
//!
//! **2.9x**, or 1.33 µs -> 0.46 µs per leaf. The walk costs more than its own
//! standalone measurement (709 µs) suggests, because interleaving it with the
//! encoder's work costs both of them cache locality — which is the case for
//! hoisting it out entirely rather than merely making it cheaper.
//!
//! # The canonical shape
//!
//! Every professional 2D stack separates *shaping* from a *replayable
//! positioned glyph list*: Skia's `SkTextBlob`, Qt's `QGlyphRun` (and the
//! `QStaticText` that caches one). The list is immutable, cheap to hold, and
//! drawn many times per build. This module is that type for pinion, and the
//! cache that already owns the shaped layout owns it too — same key, same
//! lifetime, same eviction — because it is the second half of one derivation
//! rather than a separate thing to keep in sync.
//!
//! # Backend-orthogonal by construction
//!
//! `pinion-text` is renderer-agnostic (§5.36), so these types name no
//! rasteriser: a [`PositionedGlyph`] is an id and a position, and a
//! [`RunDecoration`] is a resolved horizontal rule. The Vello adapter maps
//! each to `vello::Glyph` / a stroked line; a future backend maps them
//! elsewhere. The mapping is a field copy over a slice — the cost the
//! measurement above already includes.

use parley::FontData;
use pinion_core::style::Color;

/// One glyph, positioned in its layout's own coordinate frame.
///
/// `x` / `y` are absolute within the layout (parley's line offset and baseline
/// already folded in), so a painter places the whole run with one transform and
/// never re-derives a pen position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedGlyph {
    /// Glyph id within [`PositionedRun::font`].
    pub id: u32,
    /// Layout-relative x of the glyph origin.
    pub x: f32,
    /// Layout-relative y of the glyph origin (the baseline).
    pub y: f32,
}

/// A resolved decoration rule — an underline or a strikethrough — as the
/// horizontal line a painter strokes.
///
/// Resolved, not raw: parley reports a decoration as an *optional* offset and
/// size that fall back to the run's font metrics, and the offset is measured
/// **upward from the baseline** while screen y grows downward. Both the
/// fallback and the sign flip are properties of the shaped run, so they are
/// applied once here rather than by every painter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunDecoration {
    /// Layout-relative y of the rule.
    pub y: f32,
    /// Pen width. A painter still clamps to a visible hairline; the font may
    /// report a sub-pixel metric.
    pub size: f32,
    /// The rule's colour — parley defaults it to the run's foreground.
    pub brush: Color,
}

/// One shaped, positioned glyph run: everything a painter needs to draw it,
/// with nothing left to derive.
///
/// Produced by [`LayoutCache::positioned_runs`](crate::LayoutCache::positioned_runs)
/// and cached beside the [`Layout`](crate::Layout) it came from.
///
/// `brush` is the run's *own* foreground, which is what styled-run text
/// ([`crate::LayoutCache::layout_with_runs`]) needs — each run may differ. A
/// caller that paints one uniform colour regardless (the §5.41 cell grid, whose
/// colour comes from the terminal palette, not from the shaped style) passes
/// its own brush and ignores this field. Both callers exist, which is why the
/// field is data rather than baked into the glyph list.
#[derive(Debug, Clone)]
pub struct PositionedRun {
    /// The face these glyph ids index.
    pub font: FontData,
    /// Shaped size in pixels.
    pub font_size: f32,
    /// The run's own foreground colour.
    pub brush: Color,
    /// The run's glyphs, positioned in layout space.
    pub glyphs: Vec<PositionedGlyph>,
    /// Layout-relative x where the run starts — the left end of a decoration.
    pub start_x: f32,
    /// Layout-relative x where the run ends (`start_x` + advance) — the right
    /// end of a decoration.
    pub end_x: f32,
    /// The run's underline, already resolved against its font metrics.
    pub underline: Option<RunDecoration>,
    /// The run's strikethrough, already resolved against its font metrics.
    pub strikethrough: Option<RunDecoration>,
}

/// R1531 §5.36 — derive the draw list for a shaped layout.
///
/// The walk this exists to run exactly once per shaped layout. Kept beside the
/// types rather than inside `LayoutCache` so the derivation is testable against
/// a `Layout` a caller shaped itself.
#[must_use]
pub fn positioned_runs(layout: &crate::Layout) -> Vec<PositionedRun> {
    use parley::PositionedLayoutItem;

    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let parley_run = run.run();
            let metrics = parley_run.metrics();
            let baseline = run.baseline();
            // parley's decoration offset is measured upward from the baseline;
            // screen y grows downward, so the rule sits at `baseline - offset`.
            let rule = |deco: &parley::Decoration<Color>, offset: f32, size: f32| RunDecoration {
                y: baseline - deco.offset.unwrap_or(offset),
                size: deco.size.unwrap_or(size),
                brush: deco.brush,
            };
            let style = run.style();
            out.push(PositionedRun {
                font: parley_run.font().clone(),
                font_size: parley_run.font_size(),
                brush: style.brush,
                start_x: run.offset(),
                end_x: run.offset() + run.advance(),
                underline: style
                    .underline
                    .as_ref()
                    .map(|d| rule(d, metrics.underline_offset, metrics.underline_size)),
                strikethrough: style
                    .strikethrough
                    .as_ref()
                    .map(|d| rule(d, metrics.strikethrough_offset, metrics.strikethrough_size)),
                glyphs: run
                    .positioned_glyphs()
                    .map(|g| PositionedGlyph {
                        id: g.id,
                        x: g.x,
                        y: g.y,
                    })
                    .collect(),
            });
        }
    }
    out
}
