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
//! And (R1528) the crate's first non-linear **axis** — [`LineChart::y_log`] / [`ScatterChart::x_log`] and their
//! `_base` twins (the toolkit's log value axis). Until then every axis was affine:
//! [`LinearScale`] was the only mapping in the crate, so a latency series over `0.1 ms .. 1000 ms` put
//! everything below 100 ms on the baseline. A log axis puts equal *ratios* on
//! equal pixel spans. Three things follow from that and are worth knowing
//! before using it: the auto-domain measures only the strictly positive
//! samples and snaps to whole **decades**; each decade carries fainter
//! unlabelled subdivisions (`.grid.minor.*`), without which evenly-spaced decade lines
//! read as a linear axis; and a sample at or below zero has **no pixel**, so
//! it draws no mark and is reported by [`LineChart::off_scale`] — the [`Mapped::unreadable`] stance applied to the
//! axis rather than the input. The bar chart is deliberately excluded: a bar
//! encodes magnitude by length from a zero baseline, and a log axis has no
//! zero.
//!
//! And (R1529) the third axis kind — [`LineChart::x_time`] / [`ScatterChart::x_time`] and their `y_` twins, plus
//! [`Timeline::time_axis`] (the toolkit's date time axis, d3's `scaleUtc`). Values on it are epoch
//! **milliseconds**, UTC. Unlike the log axis this one is *affine* — equal
//! durations occupy equal pixel spans, so it reuses [`LinearScale`] untouched — and
//! everything that makes it a distinct kind is in its ticks and labels. Both
//! were wrong before: [`nice_ticks`]'s `1 / 2 / 5 x 10^n` step is a **decimal** quantiser, and above a
//! second time is mixed-radix, so a one-hour domain got gridlines at `00:06:40`; and
//! the label compacted by magnitude, which at the giga scale has 27-hour
//! resolution, so every tick of a sub-day axis rendered as the same string.
//! [`time_ticks`] lands on clock and calendar boundaries and [`format_time_tick`] names each one by
//! the finest field that distinguishes it, so the date appears where the axis
//! crosses into a new day rather than on every label. Two things are worth
//! knowing before using it: a lone value — a scrub header, an a11y readout —
//! takes [`TickFormat::readout`] rather than the tick label, because a relative label out of
//! its row is ambiguous; and the axis is UTC only, a local one needing a
//! timezone database that would also make every axis test read the host's
//! configuration.
//!
//! And (R1545) the fourth axis kind, and the first that was already *drawn*
//! before it was an axis — [`LineChart::x_category`] / [`ScatterChart::x_category`] and their `y_` twins, plus [`BarChart::x_window`] (the
//! toolkit's bar category axis, d3's `scaleBand`). The bar chart has had a categorical
//! x since R1374, but as a private slot metric: `left + i * slot`, written out three times
//! inside `bar.rs` for the bar box, its label and its click surface, reachable by
//! no other chart and by no consumer. Making it an [`AxisKind`] arm gives it what the
//! other three have — the shared tick set, the shared label format, and the
//! domain pinning through which a [`PlotWindow`] wheel zoom and a category window both
//! reach it — and gives every cartesian chart the ability to plot over named
//! slots.
//!
//! Three things follow and are worth knowing before using it. Sample `x`
//! values are category **indices** (category `i` sits at `x = i`, occupying the
//! band `i ± 0.5`), and one that names no slot has no pixel, so it is reported by
//! [`LineChart::off_scale`] rather than drawn somewhere invented. [`CategoryScale::band`] publishes where a
//! category is drawn, which the toolkit's axis cannot answer at all — a
//! toolkit bar's rect is computed inside the private series painter. And a
//! window is resolved from names *before* it reaches a chart ([`Categories::window`] → [`CategoryWindow`]),
//! where the toolkit's `setRange(string, string)` returns `void` and silently ignores a name that is not
//! a category.
//!
//! And (R1553) the crate's first datum that is **not a point** — a [`Distribution`],
//! drawn by [`BoxPlotChart`] (the toolkit's box plot series). Every value above resolves
//! to one position; a distribution occupies a span of the value axis and
//! carries interior landmarks, so one datum emits a box, a median, two
//! whiskers, two caps and a mark per outlier — Tukey's schema (*Exploratory
//! Data Analysis*, 1977), optionally notched after `McGill`, Tukey & Larsen (1978).
//!
//! The toolkit's box set is five doubles and `the toolkit's charting module` computes none of them (its
//! own box-plot example ships a `findMedian()` helper in the *example*). Four things
//! follow and are worth knowing before using this:
//!
//! * The summary is **derived**, and by a *named* definition —
//!   [`QuantileMethod`] carries three standard ones (Tukey's hinges, Hyndman
//!   & Fan types 7 and 6) that disagree for small `n`, so the method is part
//!   of the value rather than a caller's comment. Handing in five
//!   pre-computed numbers is still supported
//!   ([`Distribution::from_summary`], the toolkit's contract) — with the ordering
//!   *checked*, where `setValue` accepts an upper quartile below the
//!   lower one and paints an inverted box in silence.
//! * **Outliers exist.** The whiskers stop at Tukey's `k * IQR` fence and
//!   each sample beyond it is its own addressable mark. box set has five
//!   slots and no per-outlier geometry, so a toolkit box plot cannot draw one at
//!   any setting — and that fence is the defining half of the form.
//! * The **notch** ([`BoxPlotChart::notched`]) is drawn only where the
//!   statistic exists: it is a function of the sample count, which a
//!   pre-computed summary does not carry, so such a box keeps its plain
//!   rectangle in the same chart. box set carries no `n` at all.
//! * The value axis may be **logarithmic** ([`BoxPlotChart::y_log`]) —
//!   legitimately, unlike [`BarChart`]'s, because a box plot encodes five
//!   positions rather than a length from zero. A landmark the axis cannot
//!   place draws nothing and is reported by [`BoxPlotChart::off_scale`].
//!
//! And (R1567) the second series type with extent — a [`Candle`], drawn by [`CandlestickChart`]
//! (the toolkit's candlestick series). The paragraph that stood here said a
//! candlestick would be the box plot's *second consumer*, "the same interval
//! geometry over open / high / low / close". **Building one showed that was
//! wrong, and the reason is the round.**
//!
//! A [`Distribution`]'s five landmarks are totally ordered by construction.
//! A candle has four, and only three order relations among them: `low` is
//! below both middle values and `high` above both, while `open` and `close`
//! are **not ordered against each other at all**. That absence is not a
//! weaker invariant, it is the datum — which of the two is larger is the fact
//! a candlestick chart exists to show, and two sessions with the same four
//! numbers, the same extent and the same box mean opposite things. A type
//! whose invariant is "non-decreasing" cannot hold it. What the two really
//! share is the *paint*, and that is where the sharing now is (the per-slot
//! x-label lift in [`draw`](crate)).
//!
//! Six things follow and are worth knowing before using it:
//!
//! * **The relations are checked and the refusal names the slot.**
//!   candlestick set is five `qreal`s with `void` setters, so a transposed
//!   high and low, or an open outside the extremes, paints whatever geometry
//!   it implies. [`Candle::new`] refuses each ([`CandleError`]).
//! * **The direction is a value with three arms.** the toolkit has `increasingColor`
//!   and `decreasingColor` — paint properties on the *series* — and no
//!   accessor on the set, and its documented rule is a strict `close > open`,
//!   so a **doji** (the session that closed where it opened, the single
//!   most-read signal in the form) is silently painted as a losing one.
//!   [`Direction::Doji`] is its own arm.
//! * **The direction survives the loss of hue.** It is also the body's
//!   [`BodyFill`] — hollow for a rise, solid for a fall, the traditional
//!   Japanese form, which predates colour — so a colour-blind reader or a
//!   grayscale print keeps it. The default hues were chosen by *measurement*:
//!   the conventional mid-green / mid-red pair is all but isoluminant at
//!   1.11:1, which is exactly why that palette fails, and
//!   [`CandlestickChart::direction_contrast`] publishes the ratio.
//! * **One datum, two readings of the x-axis** ([`SessionAxis`]). A candle
//!   carries its own instant, so whether the axis is equal ordinal slots
//!   (the toolkit's bar category axis — how a price chart is read, and the weekend is
//!   invisible) or real UTC time (the toolkit's date time axis — where the weekend is
//!   a gap) is a property of the chart. The toolkit reaches the two by attaching
//!   different axis objects and supplying the category names as a **second
//!   data source** unrelated to the sets' timestamps; here the slot names are
//!   *derived* from the instants and the sessions sorted by them once, so the
//!   readings cannot disagree. One [`CategoryWindow`] narrows both.
//! * **The derived readings exist.** Body, range, the two shadows, the
//!   signed change and the percent change (`None` where it does not exist)
//!   are on the datum; candlestick set exposes none, so a toolkit consumer
//!   recomputes them.
//! * **A slot the axis cannot place is REPORTED**
//!   ([`CandlestickChart::off_scale`]) rather than pinned to a domain floor —
//!   R1528's stance, on the axis a long price history actually wants.
//!
//! Not yet: cross-filtering between two ARBITRARY chart types, and a y-rescale
//! to a brush-zoomed x-window (distinct from R1381's rescale-to-VISIBLE-series)
//! — follow-up slices on that same core. (A frequency *histogram* is a consumer
//! pattern over [`BarChart`], not a distinct type — `hello-frame-profiler` bins
//! its frame times into one, and R1375 lets a reader scrub it for each bin's
//! frame count.)
//!
//! # Where the data comes from (R1446)
//!
//! Every chart above takes a `Vec<Series>` the caller built. [`ModelMapper`] adds the other
//! source: a [`CellTable`] — pinion's row-major [`CellValue`](pinion_core::CellValue) block,
//! what every editable grid in the tree holds — projected into series by
//! naming which [`Field`] is x and which are y (the `the toolkit's charting module` `Q*XYModelMapper` contract). That makes
//! *which field is plotted* a runtime choice, and makes what the user edits
//! the thing the chart draws. Unlike the toolkit's `toReal()`, a cell that holds no
//! number is reported ([`Mapped::unreadable`]) rather than plotted as zero. `examples/hello-model-chart` is the forcing
//! consumer.
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
mod boxplot;
mod brush;
mod candle;
mod candlestick;
mod civil;
mod color_scale;
mod density;
mod distribution;
mod donut;
mod draw;
mod interpolate;
mod line;
mod model;
mod palette;
mod plot;
mod polar;
mod polar_chart;
mod scale;
mod scatter;
#[cfg(test)]
mod scene_probe;
mod series;
mod sparkline;
mod style;
mod ticks;
mod timeline;
mod treemap;
mod window;

pub use bar::{Bar, BarChart};
pub use boxplot::{BoxPlotChart, DistributionMark, LandmarkKind, OffScaleLandmark, ViolinScale};
pub use brush::{Brush, BrushStripColors};
pub use candle::{
    BodyFill, Candle, CandleError, CandlePosition, Direction, candle_bounds, positive_candle_bounds,
};
pub use candlestick::{CandlestickChart, OffScaleCandle, SessionAxis, SessionMark};
pub use color_scale::{ColorScale, contrast_ratio, readable_ink, relative_luminance};
pub use density::{Bandwidth, DEFAULT_RESOLUTION, Density, DensityError, Kernel};
pub use distribution::{
    DEFAULT_FENCE, Distribution, DistributionError, DistributionSource, QuantileMethod,
    SummaryPosition, distribution_bounds, positive_distribution_bounds,
};
pub use donut::{DonutChart, Slice};
pub use interpolate::{CurveSegment, Interpolation, Overshoot, curve, is_graph, overshoot};
pub use line::LineChart;
pub use model::{CellTable, Field, Mapped, ModelMapper, Orientation, UnreadableCell, numeric};
pub use palette::CategoricalPalette;
pub use plot::OffScale;
pub use polar::{AngularScale, Winding};
pub use polar_chart::PolarChart;
pub use scale::{
    AxisKind, Categories, CategoryLookup, CategoryScale, CategoryWindow, DEFAULT_LOG_BASE,
    LinearScale, LogScale, ValueScale,
};
pub use scatter::ScatterChart;
pub use series::{
    Bounds, DataPoint, Series, StackedBand, bounds_in_x_window, data_bounds, stack,
    stacked_value_bounds, value_bounds, visible_data_bounds,
};
pub use sparkline::Sparkline;
pub use style::{ChartStyle, Margin};
pub use ticks::{
    TickFormat, format_axis_tick, format_log_tick, format_si, format_tick, format_time_stamp,
    format_time_tick, log_minor_ticks, log_ticks, nice_log_domain, nice_ticks, nice_time_domain,
    tick_decimals, time_ticks,
};
pub use timeline::{Lane, Span, Timeline};
pub use treemap::{Tile, Treemap, color_value_bounds};
pub use window::{PlotWindow, map_window, plot_area};
