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
//! positioned glyph list*: Skia's `SkTextBlob`, the toolkit's glyph run (and the static
//! text that caches one). The list is immutable, cheap to hold, and drawn many
//! times per build. This module is that type for pinion, and the cache that
//! already owns the shaped layout owns it too — same key, same lifetime, same
//! eviction — because it is the second half of one derivation rather than a
//! separate thing to keep in sync.
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
use pinion_core::scene::{StyleRun, effective_style_at};
use pinion_core::style::{Color, TextStyle, UnderlineStyle};

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

/// R1540 §5.36 — a run's underline: where the rule sits, and which FORM it
/// takes.
///
/// A separate type from [`RunDecoration`] rather than a field on it, because only the
/// underline has a form axis — a strikethrough is one straight rule in both
/// SGR (9) and the toolkit. Folding them together would put a field on the
/// strikethrough that no reader could act on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunUnderline {
    /// Where and how thick, resolved against the font metrics.
    pub rule: RunDecoration,
    /// Which form the rule takes. Never [`UnderlineStyle::None`]: an absent
    /// underline is the enclosing `Option`, so the two cannot disagree.
    pub style: UnderlineStyle,
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
    pub underline: Option<RunUnderline>,
    /// The run's strikethrough, already resolved against its font metrics.
    pub strikethrough: Option<RunDecoration>,
}

/// R1531 §5.36 — derive the draw list for a shaped layout.
///
/// The walk this exists to run exactly once per shaped layout. Kept beside the
/// types rather than inside `LayoutCache` so the derivation is testable against
/// a `Layout` a caller shaped itself.
///
/// R1540 — `base` + `style_runs` are the SAME inputs the layout was shaped
/// from, and they are needed again here because parley's run style carries the
/// underline's brush and metrics but not its FORM (parley has no notion of an
/// undercurl). The form is resolved per run by byte offset, mirroring parley's
/// own last-push-wins overlap rule, so a run's form is the one the bytes it
/// covers were given.
#[must_use]
pub fn positioned_runs(
    layout: &crate::Layout,
    base: &TextStyle,
    style_runs: &[StyleRun],
) -> Vec<PositionedRun> {
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
            // A parley run is a maximal span of uniform style, so the form at
            // its first byte holds for all of it.
            let ul_style = underline_style_at(base, style_runs, parley_run.text_range().start);
            out.push(PositionedRun {
                font: parley_run.font().clone(),
                font_size: parley_run.font_size(),
                brush: style.brush,
                start_x: run.offset(),
                end_x: run.offset() + run.advance(),
                underline: style.underline.as_ref().map(|d| RunUnderline {
                    rule: rule(d, metrics.underline_offset, metrics.underline_size),
                    // parley was asked for a decoration iff the form is drawn
                    // (`is_on`), so a `Some` here cannot carry `None`. The
                    // fallback keeps that stated rather than assumed.
                    style: if ul_style.is_on() {
                        ul_style
                    } else {
                        UnderlineStyle::Single
                    },
                }),
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

/// The underline form in effect at `offset` (R1540).
///
/// R1546 — the "last run covering the offset, else the base" walk is
/// [`effective_style_at`], lifted to `pinion-core` when this became its third
/// copy. See there for why the rule is last-match.
fn underline_style_at(base: &TextStyle, runs: &[StyleRun], offset: usize) -> UnderlineStyle {
    effective_style_at(base, runs, offset).decoration.underline
}

/// R1546 §5.36 — one painted background band: a rectangle behind a stretch of
/// glyphs, in the layout's own coordinate frame.
///
/// A band is per **visual line**, so a range that soft-wraps produces one band
/// per row — the shape a highlighter pen leaves, with a ragged right edge that
/// stops at the text rather than at the box.
///
/// `start` / `end` are the UTF-8 byte range the band was derived for, carried
/// so a consumer can ask which bytes a painted band belongs to. Every band cut
/// from the same range repeats that range; the geometry is what differs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBackground {
    /// UTF-8 byte offset of the first byte in the band's range (inclusive).
    pub start: u32,
    /// UTF-8 byte offset one past the last byte in the band's range.
    pub end: u32,
    /// Layout-relative x of the band's left edge.
    pub x: f32,
    /// Layout-relative y of the band's top edge.
    pub y: f32,
    /// Band width.
    pub width: f32,
    /// Band height — the visual line's box, so consecutive lines tile with no
    /// seam and a band registers with a selection band over the same bytes.
    pub height: f32,
    /// The declared background colour ([`TextStyle::bg_color`]).
    pub color: Color,
}

/// R1546 §5.36 — derive the background bands for a shaped layout.
///
/// # Why the bands are cut by byte, not by parley run
///
/// parley has no notion of a background (its `StyleProperty` carries a
/// foreground `Brush` and decorations, and nothing else), so — exactly as with
/// R1540's underline FORM — the value is resolved here from the same `base` +
/// `style_runs` the layout was shaped from.
///
/// But unlike the underline form, a background must NOT be resolved per parley
/// run. A parley run is a maximal span of uniform *shaping* — it splits on font
/// fallback and on bidi level, neither of which a background cares about — so
/// one declared highlight can arrive as three runs, and three abutting rects at
/// f32 boundaries is three chances of a hairline seam through a solid band.
/// The bands are therefore cut where the *declaration* changes: the byte space
/// is segmented at every run boundary, each segment takes its effective
/// [`TextStyle::bg_color`] ([`effective_style_at`], the shaper's own
/// last-push-wins rule), adjacent segments agreeing on colour are merged, and
/// each surviving segment is measured.
///
/// # The geometry is the selection band's
///
/// Measuring is [`crate::selection_rects_for_range`] — parley's own per-line
/// range geometry, and the function the `TextField`'s selection, find-match and
/// preedit bands already call. So a highlight and a selection over the same
/// bytes cannot disagree: they are not two derivations that happen to match,
/// they are one function called twice.
///
/// # A run with no background punches a hole
///
/// [`StyleRun`] carries a FULLY RESOLVED style, so a run whose `bg_color` is
/// `None` states that its bytes have no background — even where the base style
/// declares one. That falls out of segmenting by effective value rather than
/// being a rule applied on top, which is the point: there is no ordering
/// question to get wrong, because a byte has exactly one background.
#[must_use]
pub fn backgrounds(
    layout: &crate::Layout,
    base: &TextStyle,
    style_runs: &[StyleRun],
    content_len: usize,
) -> Vec<TextBackground> {
    if content_len == 0 {
        return Vec::new();
    }
    // Segment the byte space at every declaration boundary. A run reaching past
    // the content (the shaper ignores out-of-range pushes) contributes no
    // boundary the content can see.
    let mut cuts: Vec<usize> = Vec::with_capacity(style_runs.len() * 2 + 2);
    cuts.push(0);
    cuts.push(content_len);
    for run in style_runs {
        cuts.push((run.start as usize).min(content_len));
        cuts.push((run.end as usize).min(content_len));
    }
    cuts.sort_unstable();
    cuts.dedup();

    // Merge adjacent segments that resolve to the same background, so one
    // declared highlight split by an unrelated run boundary (a bold word inside
    // it, say) is still one band.
    let mut spans: Vec<(usize, usize, Option<Color>)> = Vec::new();
    for pair in cuts.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let bg = effective_style_at(base, style_runs, a).bg_color;
        match spans.last_mut() {
            Some(last) if last.2 == bg => last.1 = b,
            _ => spans.push((a, b, bg)),
        }
    }

    let mut out = Vec::new();
    for (a, b, bg) in spans {
        let Some(color) = bg else { continue };
        for rect in crate::selection_rects_for_range(layout, a, b) {
            // A zero-width band is a range that measured to nothing (an empty
            // line's own range, say); it would paint no pixels and publishing
            // it would put a rect a consumer cannot locate into the answer.
            if rect.width <= 0.0 {
                continue;
            }
            out.push(TextBackground {
                start: u32::try_from(a).unwrap_or(u32::MAX),
                end: u32::try_from(b).unwrap_or(u32::MAX),
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color,
            });
        }
    }
    out
}

/// R1550 §5.36 — what a draw list is holding.
///
/// [`PositionedRun::font`] is deliberately not in the total. `FontData` is a
/// `peniko::Blob` — an `Arc` over a font file that the font collection owns
/// and every run over that face shares. Counting it per run would report one
/// 5 MB face five hundred times, which is exactly the double-count the
/// [`Footprint`](pinion_core::footprint::Footprint) contract's shared-interior
/// rule exists to prevent.
mod footprint {
    use super::{PositionedGlyph, PositionedRun, RunDecoration, RunUnderline, TextBackground};
    use pinion_core::footprint::Footprint;

    impl Footprint for TextBackground {
        fn footprint(&self) -> usize {
            let Self {
                start,
                end,
                x,
                y,
                width,
                height,
                color,
            } = self;
            start.footprint()
                + end.footprint()
                + x.footprint()
                + y.footprint()
                + width.footprint()
                + height.footprint()
                + color.footprint()
        }
    }

    impl Footprint for PositionedGlyph {
        fn footprint(&self) -> usize {
            let Self { id, x, y } = self;
            id.footprint() + x.footprint() + y.footprint()
        }
    }

    impl Footprint for RunDecoration {
        fn footprint(&self) -> usize {
            let Self { y, size, brush } = self;
            y.footprint() + size.footprint() + brush.footprint()
        }
    }

    impl Footprint for RunUnderline {
        fn footprint(&self) -> usize {
            let Self { rule, style } = self;
            rule.footprint() + style.footprint()
        }
    }

    impl Footprint for PositionedRun {
        fn footprint(&self) -> usize {
            let Self {
                // Shared with the font collection — see the module note.
                font: _,
                font_size,
                brush,
                glyphs,
                start_x,
                end_x,
                underline,
                strikethrough,
            } = self;
            font_size.footprint()
                + brush.footprint()
                + glyphs.footprint()
                + start_x.footprint()
                + end_x.footprint()
                + underline.footprint()
                + strikethrough.footprint()
        }
    }
}

/// R1551 §5.36 — one shaped line's box, in the layout's own coordinate frame.
///
/// This is where a line **landed**, as the painter has it: parley's own
/// `LineMetrics`, not a re-derivation. It is what makes a paragraph-level
/// declaration checkable — a first-line indent is a claim about where line 0
/// starts, and only the shaped layout can say whether it got there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLineBox {
    /// UTF-8 byte offset of the line's first byte (inclusive).
    pub start: u32,
    /// UTF-8 byte offset one past the line's last byte.
    pub end: u32,
    /// Layout-relative x of the line's first glyph — parley's
    /// `LineMetrics::offset`, which is where the CSS `text-indent` and the
    /// alignment both land.
    pub x: f32,
    /// Layout-relative y of the line's top edge (`block_min_coord`).
    pub y: f32,
    /// Full advance of the line, trailing whitespace included.
    pub advance: f32,
    /// Advance of the line's trailing whitespace, so a consumer can compute the
    /// inked width (`advance - trailing_whitespace`) the way alignment does.
    pub trailing_whitespace: f32,
    /// The line box's height (`block_max_coord - block_min_coord`).
    pub height: f32,
}

/// R1551 §5.36 — every shaped line's box, top to bottom.
///
/// One call over [`parley::Layout::lines`]; nothing is recomputed, so the boxes
/// published here are the ones the glyph walk uses.
#[must_use]
pub fn line_boxes(layout: &crate::Layout) -> Vec<TextLineBox> {
    layout
        .lines()
        .map(|line| {
            let m = line.metrics();
            let range = line.text_range();
            TextLineBox {
                start: u32::try_from(range.start).unwrap_or(u32::MAX),
                end: u32::try_from(range.end).unwrap_or(u32::MAX),
                x: m.offset,
                y: m.block_min_coord,
                advance: m.advance,
                trailing_whitespace: m.trailing_whitespace,
                height: m.block_max_coord - m.block_min_coord,
            }
        })
        .collect()
}
