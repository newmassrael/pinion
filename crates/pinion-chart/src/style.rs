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

/// A conservative advance for the widest character at `size_px` — the label
/// fit's **headless fallback** (R1633, made a derivation R1636).
///
/// Three fifths of the em: wider than the digits of every UI face this project
/// has measured, and narrower than a full-width glyph no tick label holds.
///
/// It was a FIELD for one round, so a consumer with a measured face could
/// overwrite it. That was the shape of R1633's first draft, where the model was
/// the primary answer; by the time that round landed the primary answer was
/// [`pinion_core::measured_text_extent`] and this was only what a headless
/// caller falls back to. The field outlived its reason and cost something for
/// it: **ten examples set `label_size_px` through `..ChartStyle::default()`**,
/// which left every one of them fitting 12-to-14-pixel labels against an
/// advance derived from 11, and **nothing anywhere set the field** — the
/// consumer it existed for never arrived. A `with_label_size` builder existed
/// to keep the pair together and had no callers either.
///
/// Derived, the two cannot disagree. A consumer that really has measured its
/// face states it the way every other measurement in this framework is stated:
/// by seeding a [`TextMetrics`](pinion_core::TextMetrics) provider, which the
/// fit consults first.
///
/// # No test pins the ratio, and that is not an omission
///
/// A counterfactual widening it to four fifths breaks nothing, which was
/// checked rather than assumed. The reason is structural: this constant is
/// consulted **only when nothing can measure**, so any assertion about how
/// close it is to a real advance would need the measurement whose absence is
/// the sole condition for reaching it. What IS pinned is everything that can
/// be: that the fallback follows the label size
/// (`r1636_the_fallback_advance_follows_the_label_size`), and that a seeded
/// provider wins over it whatever it says
/// (`r1633_the_fit_measures_where_it_can_and_models_where_it_cannot`). The
/// value itself is a judgement, and a tautological test asserting it equals
/// itself would read as coverage of one.
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
                advance_px: advance_for(self.label_size_px),
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
