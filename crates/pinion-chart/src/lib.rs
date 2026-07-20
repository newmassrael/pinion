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
//! a **layout-native** entry point, [`LineChart::build_fill`] (R1360); a
//! categorical [`BarChart`] with per-bar colours (R1374), sharing the same
//! `scale` / `ticks` / `palette` / [`draw`](crate) core; and (R1375) that bar
//! chart's own scrub [`inspect`](BarChart::inspect) overlay — a highlight ring
//! on the focused bar + a value tooltip, over the lifted `callout` tooltip core
//! the chart types now share; and (R1376) a [`DonutChart`] — the crate's first
//! PART-OF-WHOLE form (filled Bézier-arc sectors, a centre hole, a legend, and
//! the same scrub inspect showing each slice's percent share); and (R1377) a
//! [`ScatterChart`] — the crate's third cartesian chart (numeric x AND y),
//! plotting each series as filled point marks. Being the third axis chart, it
//! is what let the shared cartesian substrate finally factor out — each helper
//! collapsing to ONE definition at the round its consumer count reached: line,
//! bar, and scatter now CALL one set of axis furniture (`gridlines` / `axes` /
//! `y_tick_labels`, in [`draw`](crate)) rather than re-derive it; line and
//! scatter share one plot resolver (`crate::plot`); the legend row is one
//! definition across line, donut, and scatter; and the circle geometry one
//! across the line and scatter markers. On the line chart, R1379-R1381 then
//! added per-series visibility (`Series::visible` — a hidden series drops its
//! geometry but keeps its index / legend slot / domain), an interactive legend
//! ([`LineChart::interactive_legend`] — legend entries emit as focusable,
//! tagged containers a caller binds to a toggle, so the chart stays a pure scene
//! producer), and an opt-in [`LineChart::rescale_to_visible`] that re-domains
//! the axes to only the visible series. And (R1382) the [`Treemap`] — the
//! crate's SECOND part-of-whole form, area-encoded rather than angular: a
//! squarified (Bruls-Huizing-van Wijk) tile layout with contrast-aware in-tile
//! labels and the same scrub inspect (ring + `value (percent%)` tooltip). And
//! (R1384) the first CROSS-FILTER surface — [`BarChart::select`] mutes the bars
//! outside an active category set, [`BarChart::selectable`] overlays one
//! focusable, caller-tagged hit region per bar (the `interactive_legend` chip
//! mechanism applied to the bars), and `examples/hello-cross-filter` wires that
//! selection to filter a companion line chart: a click in one widget reshapes
//! another. And (R1385) the [`Sparkline`] — a compact, axis-less trend line (no
//! axes / ticks / gridlines / legend, an optional faint area fill + end / min /
//! max reference dots) for inline display in a table cell or a KPI stat tile;
//! `examples/hello-stat-tiles` composes a row of them. And (R1389) the
//! [`Timeline`] — the crate's first NON-value form: labelled time [`Span`]s
//! grouped into horizontal [`Lane`]s over a shared numeric time ruler, with a
//! draggable **playhead** scrubber whose readout names the span under it per
//! lane (the track view a sequencer / capture-replay tool needs).
//! `examples/hello-timeline` is the forcing consumer — a flame view of the real
//! per-frame render phases (`use_frame_timings`). And (R1391) the numeric
//! cross-filter leg — [`ScatterChart::select_x_range`] mutes the point marks
//! outside a numeric x-range (the mark-level, continuous-range twin of the
//! categorical [`BarChart::select`], sharing the one lifted `MUTED_ALPHA`);
//! `examples/hello-scatter`'s brush strip drives it, so a numeric BRUSH in one
//! widget dims the corresponding points in another.
//!
//! Not yet: cross-filtering between two ARBITRARY chart types, and a y-rescale
//! to a brush-zoomed x-window (distinct from R1381's rescale-to-VISIBLE-series)
//! — follow-up slices on that same core. (A frequency *histogram* is a consumer
//! pattern over [`BarChart`], not a distinct type — `hello-frame-profiler` bins
//! its frame times into one, and R1375 lets a reader scrub it for each bin's
//! frame count.)
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
mod brush;
mod donut;
mod draw;
mod line;
mod palette;
mod plot;
mod scale;
mod scatter;
mod series;
mod sparkline;
mod style;
mod ticks;
mod timeline;
mod treemap;

pub use bar::{Bar, BarChart};
pub use brush::{Brush, BrushStripColors};
pub use donut::{DonutChart, Slice};
pub use line::LineChart;
pub use palette::CategoricalPalette;
pub use scale::LinearScale;
pub use scatter::ScatterChart;
pub use series::{Bounds, DataPoint, Series, bounds_in_x_window, data_bounds, visible_data_bounds};
pub use sparkline::Sparkline;
pub use style::{ChartStyle, Margin};
pub use ticks::{format_axis_tick, format_si, format_tick, nice_ticks, tick_decimals};
pub use timeline::{Lane, Span, Timeline};
pub use treemap::{Tile, Treemap};
