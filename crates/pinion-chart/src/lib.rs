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
//! (R1356) and the pinned-domain re-scaling a brush zoom drives (R1357);
//! a **layout-native** entry point, [`LineChart::build_fill`] (R1360); and a
//! categorical [`BarChart`] with per-bar colours (R1374), sharing the same
//! `scale` / `ticks` / `palette` / [`draw`](crate) core.
//!
//! Not yet: donut / treemap / scatter, legend-toggle, cross-filtering, and a
//! y-rescale on zoom — follow-up slices on that same core. (A frequency
//! *histogram* is a consumer pattern over [`BarChart`], not a distinct type —
//! `hello-frame-profiler` bins its frame times into one.)
//!
//! # Two entry points — pick by who places the chart
//!
//! * [`LineChart::build_fill(size)`](LineChart::build_fill) — **layout
//!   places it.** The root fills its slot; every child is authored in the
//!   chart's own `(0, 0)..(w, h)` frame. Dock it, flex it, resize it. The
//!   consumer feeds the slot's measured size back with
//!   [`use_pane_viewport_size`](pinion_core::use_pane_viewport_size) keyed
//!   on the chart's tag; `examples/hello-chart-fill` is the worked example.
//! * [`LineChart::build(rect)`](LineChart::build) — **the caller pins it**
//!   to a window-absolute rect known before layout runs. Its children carry
//!   window-absolute positions, so it is only correct under a root at the
//!   window origin. Prefer `build_fill` for anything new.
//!
//! Two rounds got here: R1358 made `Scene::Path` commands relative to the
//! node's own rect (the *primitive* blocker — nothing chart-side could work
//! around it), and R1360 built `build_fill` on top.
//!
//! # Known limitations (do not build on these without reading)
//!
//! * **`build_fill`'s reactive seam is Vello-only, so on TUI the chart is
//!   EMPTY (§2 #6).** The slot's measured size is published only by the
//!   live Vello paint (`ShellCore::compute_paint_scene_internal`);
//!   `pinion-tui` never publishes, so `use_pane_viewport_size` stays
//!   `(0, 0)` forever and `build_fill` returns its empty sentinel. Note the
//!   degradation differs *in kind* from `build`, which still emits its
//!   background, legend and labels on TUI (only `Scene::Path` is dropped,
//!   below) — `build_fill` emits nothing at all, including for
//!   `scene/snapshot`. Latent today (no TUI chart consumer exists); it is
//!   the price of the seam and it is recorded here rather than discovered.
//! * **A hypothetical-viewport RPC query is incoherent for `build_fill`
//!   (§2 #2).** `scene/layout {viewport}` runs the non-publishing producer,
//!   so it lays the chart *root* out at the hypothetical size while the
//!   *body* inside was built at the last live-published size. The common
//!   paths are fine (`scene/snapshot from: paint` reads the live scene, and
//!   `PINION_SCREENSHOT` goes through the publishing path), but the one
//!   method whose purpose is "how would this lay out at another size?"
//!   answers wrongly for the one widget R1360 made size-responsive.
//! * **§2 #6 GUI/TUI dual does not hold for this crate.** The TUI backend
//!   does not render `Scene::Path`, so in a terminal a chart loses its
//!   series, axes, gridlines, crosshair and markers — only the background,
//!   legend swatches, and text labels survive. The chart satisfies §2 #1
//!   (structured scene) and §2 #7 (scene-as-data); it does **not** satisfy
//!   §2 #6 today, and that gap is stated here rather than left silent.

mod bar;
mod draw;
mod line;
mod palette;
mod scale;
mod series;
mod style;
mod ticks;

pub use bar::{Bar, BarChart};
pub use line::LineChart;
pub use palette::CategoricalPalette;
pub use scale::LinearScale;
pub use series::{Bounds, DataPoint, Series, data_bounds};
pub use style::{ChartStyle, Margin};
pub use ticks::{format_axis_tick, format_si, format_tick, nice_ticks, tick_decimals};
