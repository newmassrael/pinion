//! `pinion-chart` — a retained-mode data-visualization substrate for
//! pinion (R1354, Phase B).
//!
//! # Why this crate exists
//!
//! pinion already had the *primitive* for custom vector graphics —
//! [`Scene::Path`](pinion_core::Scene) (R721: `MoveTo` / `LineTo` /
//! `CurveTo` / `Close`, stroke + fill + gradient) — and the node editor
//! proved the "absolute-pixel vector overlay" technique at scale. What
//! was missing was the *library*: value-to-pixel scaling, human-friendly
//! axis ticks, a colour-blind-safe series palette, and a chart builder
//! that assembles those into a professional axes-gridlines-legend layout.
//! This crate is that library — the largest gap for a monitoring /
//! analysis dashboard consumer.
//!
//! # Design
//!
//! Charts are built from the **retained** primitives
//! ([`Scene::Path`](pinion_core::Scene) +
//! [`Scene::Text`](pinion_core::Scene)), never the immediate-mode painter
//! (which has no text and so cannot label an axis). The crate depends
//! only on `pinion-core`: it produces a [`Scene`](pinion_core::Scene)
//! that the ordinary paint / snapshot / a11y walks consume unchanged, so
//! every chart element is queryable as data (§2 #1 / #7) — an AI client
//! reads the series geometry by tag, without sampling pixels.
//!
//! Colours are passed in as resolved [`Color`](pinion_core::style::Color)
//! values, keeping the theme system a consumer concern: an app resolves
//! its `ColorRole`s into a [`ChartStyle`] and hands it to the chart, so
//! the same chart renders correctly in light and dark themes.
//!
//! # Example
//!
//! ```
//! use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
//! use pinion_core::scene::Rect;
//!
//! let chart = LineChart::new(vec![
//!     Series::new("latency", vec![DataPoint::new(0.0, 12.0), DataPoint::new(1.0, 18.0)]),
//! ])
//! .filled(true);
//!
//! let scene = chart.build(Rect::new(0, 0, 640, 360), &ChartStyle::default());
//! // `scene` is a tagged pinion_core::Scene ready to embed under a widget root.
//! ```
//!
//! # Scope
//!
//! Shipped: line and area charts with nice axes, gridlines, tick labels,
//! and a legend (R1354); a scrub [`inspect`](LineChart::inspect) overlay —
//! crosshair, per-series markers, value tooltip (R1355); x-domain clipping
//! (R1356) and the pinned-domain re-scaling a brush zoom drives (R1357).
//!
//! Not yet: histogram / bar / donut / treemap / scatter, legend-toggle,
//! cross-filtering, and a y-rescale on zoom — follow-up slices on the same
//! `scale` / `ticks` / `palette` core.
//!
//! # Known limitations (do not build on these without reading)
//!
//! * **The chart is not layout-native — but the primitive no longer stops
//!   it (R1358).** [`LineChart::build`] still takes the window-absolute
//!   rect it will occupy and must be handed that geometry before the
//!   layout pass runs, so it cannot yet flex, sit in a dock panel, or
//!   respond to a resize, and must be embedded under a root at the window
//!   origin. Until R1358 the *primitive* made this unfixable: `Scene::Path`
//!   commands painted at literal device pixels, so a chart's geometry was
//!   welded to a window position no matter what the chart did. R1358 made
//!   path commands relative to the node's own rect, removing that blocker.
//!   The remaining work is this crate's: emit children relative to a
//!   placed chart root, and discover the size at view time (the
//!   measured-rect reactive seam). See the `line` module's coordinate
//!   contract for the specifics. `build`'s signature changes with that
//!   follow-up.
//! * **§2 #6 GUI/TUI dual does not hold for this crate.** The TUI backend
//!   does not render `Scene::Path`, so in a terminal a chart loses its
//!   series, axes, gridlines, crosshair and markers — only the background,
//!   legend swatches, and text labels survive. The chart satisfies §2 #1
//!   (structured scene) and §2 #7 (scene-as-data); it does **not** satisfy
//!   §2 #6 today, and that gap is stated here rather than left silent.

mod line;
mod palette;
mod scale;
mod series;
mod ticks;

pub use line::{ChartStyle, LineChart, Margin};
pub use palette::CategoricalPalette;
pub use scale::LinearScale;
pub use series::{Bounds, DataPoint, Series, data_bounds};
pub use ticks::{format_axis_tick, format_si, format_tick, nice_ticks, tick_decimals};
