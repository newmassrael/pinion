//! Shared chart style — the resolved colours, sizes, and layout knobs every
//! chart type reads. Lived in `line.rs` until R1374 gave the crate a second
//! chart type (`bar`); at two consumers a `crate::line::ChartStyle` import
//! reads as "the bar chart borrows the line chart's style", so the style is its
//! own neutral module (both `line` and `bar` `use crate::style::…`). The
//! struct still carries a few line-only knobs (`series_width`, `area_alpha`,
//! the `crosshair`/`marker_radius`/`tooltip_*` inspect fields); splitting a
//! shared axis core from per-chart extras is deferred until a third chart type
//! shows which knobs are truly common (YAGNI on a two-consumer split).

use pinion_core::style::Color;

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

impl ChartStyle {
    /// The default chart style (neutral greys that read on a mid
    /// surface). Alias for [`Default::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
