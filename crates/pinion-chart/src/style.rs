//! Shared chart style — the resolved colours, sizes, and layout knobs every
//! chart type reads. Lived in `line.rs` until R1374 gave the crate a second
//! chart type (`bar`); at two consumers a `crate::line::ChartStyle` import
//! reads as "the bar chart borrows the line chart's style", so the style is its
//! own neutral module (both `line` and `bar` `use crate::style::…`). The
//! struct still carries a few line-only knobs (`series_width`, `area_alpha`,
//! the `crosshair`/`marker_radius`/`tooltip_*` inspect fields); splitting a
//! shared axis core from per-chart extras is deferred until a third chart type
//! shows which knobs are truly common (YAGNI on a two-consumer split).

use pinion_core::style::{Color, TextStyle};

use crate::fit::{Along, Room};

/// Pixel insets between the chart `rect` and its plotting area, leaving
/// room for the axis tick labels and (top) the legend row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Margin {
    /// Left inset — the y-axis label gutter.
    pub left: u32,
    /// Top inset — the legend row.
    pub top: u32,
    /// Right inset.
    pub right: u32,
    /// Bottom inset — the x-axis label row.
    pub bottom: u32,
}

impl Margin {
    /// Explicit per-side margins.
    #[must_use]
    pub const fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The same inset on every side.
    #[must_use]
    pub const fn uniform(value: u32) -> Self {
        Self::new(value, value, value, value)
    }
}

impl Default for Margin {
    fn default() -> Self {
        Self::new(52, 28, 16, 28)
    }
}

/// Resolved colours, sizes, and layout knobs for a chart render. The
/// colour fields are plain [`Color`]s so this crate stays decoupled from
/// the theme system — the consumer resolves its theme roles (e.g.
/// `ColorRole::Outline` for the grid) into these fields.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartStyle {
    /// Axis line colour.
    pub axis: Color,
    /// Gridline colour (usually a low-alpha outline).
    pub grid: Color,
    /// Tick-label and legend-label text colour.
    pub label: Color,
    /// Optional plot background fill.
    pub background: Option<Color>,
    /// Tick-label / legend font size in px.
    pub label_size_px: u32,
    /// R1633 — the advance in pixels of the **widest** character a tick label
    /// can hold, at [`Self::label_size_px`].
    ///
    /// The one font fact this crate needs and the one it cannot get: it is pure
    /// data with no text engine, so a consumer that has measured its own face
    /// writes this and a consumer that has not takes the default, which is a
    /// conservative 0.6 em. Over-estimating is the safe direction — it thins one
    /// label more than strictly necessary, where under-estimating draws two on
    /// top of each other.
    ///
    /// Set together with [`Self::label_size_px`] through
    /// [`Self::with_label_size`], because a size changed without this is a
    /// silently wrong fit.
    pub label_advance_px: u32,
    /// Series polyline stroke width in px.
    pub series_width: u32,
    /// Alpha (0-255) of the translucent area fill under a filled series.
    pub area_alpha: u8,
    /// Plot insets.
    pub margin: Margin,
    /// Target x-axis tick count (nice-number snapped).
    pub x_ticks: usize,
    /// Target y-axis tick count (nice-number snapped).
    pub y_ticks: usize,
    /// Whether to render the legend row.
    pub legend: bool,
    /// Inspect crosshair line colour (the vertical scrub guide).
    pub crosshair: Color,
    /// Radius (px) of the per-series inspect marker dots.
    pub marker_radius: u32,
    /// Inspect tooltip background fill.
    pub tooltip_bg: Color,
    /// Inspect tooltip header / text colour (series values use the
    /// series colour; this is the `x = …` header).
    pub tooltip_fg: Color,
}

/// A conservative advance for the widest character at `size_px`.
///
/// Three fifths of the em: wider than the digits of every UI face this project
/// has measured, and narrower than a full-width glyph no tick label holds. It
/// is a *model*, which is why [`ChartStyle::label_advance_px`] can be
/// overwritten by a consumer that has a real measurement.
const fn advance_for(size_px: u32) -> u32 {
    size_px * 3 / 5
}

/// The height of one line at `size_px`.
///
/// Not a field, unlike the advance, because a line's height is ~1.4 em in every
/// UI face while a character's width is not — so this is a constant of
/// typography where that one is a property of the font.
const fn line_for(size_px: u32) -> u32 {
    size_px * 7 / 5
}

impl ChartStyle {
    /// The default chart style (neutral greys that read on a mid
    /// surface). Alias for [`Default::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The same style at a different label size, with the advance moved with
    /// it (R1633).
    ///
    /// The pair exists so the common case cannot get the fit wrong: a caller
    /// that only changes `label_size_px` leaves an advance measured for another
    /// size behind, and nothing would say so.
    #[must_use]
    pub const fn with_label_size(mut self, size_px: u32) -> Self {
        self.label_size_px = size_px;
        self.label_advance_px = advance_for(size_px);
        self
    }

    /// The [`TextStyle`] a tick label is measured in (R1633).
    ///
    /// The face and the size are what decide an advance, and both are this
    /// struct's — so measuring with this and painting with the label nodes'
    /// own style cannot disagree about how wide a label is. The alignment and
    /// the overflow the paint additionally sets are deliberately absent: they
    /// place a laid-out string, they do not change its width.
    #[must_use]
    pub fn label_text_style(&self) -> TextStyle {
        TextStyle::new()
            .with_size_px(self.label_size_px)
            .with_fg(self.label)
    }

    /// The room a label has on the **horizontal** axis (R1633).
    #[must_use]
    pub fn room_x(&self) -> Room {
        Room::new(
            Along::Width {
                advance_px: self.label_advance_px,
            },
            self.label_text_style(),
        )
    }

    /// The room a label has on the **vertical** axis (R1633).
    #[must_use]
    pub fn room_y(&self) -> Room {
        Room::new(
            Along::Height {
                line_px: line_for(self.label_size_px),
            },
            self.label_text_style(),
        )
    }
}

impl Default for ChartStyle {
    fn default() -> Self {
        let neutral = Color::rgb(0x8A, 0x92, 0x9E);
        Self {
            axis: neutral,
            grid: neutral.with_alpha(0x30),
            label: neutral,
            background: None,
            label_size_px: 11,
            label_advance_px: advance_for(11),
            series_width: 2,
            area_alpha: 40,
            margin: Margin::default(),
            x_ticks: 6,
            y_ticks: 5,
            legend: true,
            crosshair: neutral.with_alpha(0xB0),
            marker_radius: 4,
            tooltip_bg: Color::rgb(0x25, 0x2A, 0x33),
            tooltip_fg: Color::rgb(0xE8, 0xEB, 0xEF),
        }
    }
}
