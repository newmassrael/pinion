//! Value-to-pixel mapping — the arithmetic core every axis and series
//! share.
//!
//! A [`LinearScale`] maps a data `domain` (`f64` value range) onto a
//! pixel `range` (`f32` device coordinates) with an affine transform,
//! and inverts it. The range endpoints may be given in either order, so
//! a y-axis simply passes `(bottom_px, top_px)` — a descending pixel
//! range — to get the screen-space "larger value sits higher" mapping
//! without any special-casing at the call site.
//!
//! # Two layers (R1528)
//!
//! [`LinearScale`] is the **arithmetic**: a total affine transform, used
//! directly by the charts whose axis is linear by construction (the
//! categorical [`bar`](crate::BarChart) slot metric, the axis-less
//! [`Sparkline`](crate::Sparkline), the [`Timeline`](crate::Timeline)
//! ruler).
//!
//! [`ValueScale`] is the **axis** — the toolkit's abstract axis distinction. It is what
//! a numeric-x / numeric-y chart plots through, and it is the layer that knows
//! a mapping can be *undefined*: a logarithmic axis has no pixel for zero or
//! for a negative value. So [`ValueScale::map`] returns `Option<f32>` where [`LinearScale::map`] returns `f32`, and
//! every call site has to decide what a point with no pixel looks like (the
//! line chart breaks its polyline; the scatter chart omits the mark). That is
//! the whole reason the two layers are separate — a partial map is the wrong
//! contract for the affine core, and the right one for an axis.
//!
//! # An axis kind is more than its arithmetic (R1529)
//!
//! The time axis is what shows this. It is *affine* — equal durations
//! occupy equal pixel spans — so it reuses [`LinearScale`] with no
//! transform at all, and if an axis were only its arithmetic there would
//! be nothing to add. What makes it a distinct [`ValueScale`] arm is that
//! an axis also decides **which ticks it lands on** and **how they are
//! labelled**, and on those a time axis differs from a linear one
//! completely (see [`crate::ticks`]). Carrying the kind in the scale is
//! what keeps a tick set and the mapping that places it from being
//! resolved off different axes.
//!
//! # The log scale is the linear scale (d3's `transformLog`)
//!
//! [`LogScale`] holds a [`LinearScale`] over the *log-transformed*
//! domain and composes: `map(v) = linear.map(log_b(v))`, `invert(px) =
//! b^linear.invert(px)`. This is how d3 builds `scaleLog` — a continuous
//! scale plus a transform pair — and it means the log axis inherits the
//! degenerate-domain and descending-range handling already proved here
//! rather than restating that arithmetic.
//!
//! # An axis kind can carry data (R1545)
//!
//! [`CategoryScale`] is the fourth kind and the first whose identity is
//! not a scalar: it carries the [`Categories`] themselves, because a
//! categorical axis's labels are *data* and not derivable from a value the
//! way a number, a magnitude or an instant is. That is what made
//! [`AxisKind`] stop being `Copy` — a bound that was right while every kind
//! was a scalar, and outgrown at the arm that is a list. The same shape
//! R1529 recorded one level down, where the two-armed `is_log` boolean
//! stopped naming its arms at the third kind.
//!
//! Like [`ValueScale::Time`] it is *affine* — in the category **index** —
//! so it reuses [`LinearScale`] with no transform. Category `i` sits at
//! value `i`, and its **band** spans `i ± 0.5`, which is the toolkit's
//! bar category axis geometry and d3's `scaleBand`.

use std::sync::Arc;

/// Affine `f64` domain to `f32` pixel-range mapping (and its inverse).
///
/// The transform is `pixel = range_lo + t * (range_hi - range_lo)` where
/// `t = (value - domain_lo) / (domain_hi - domain_lo)`. A degenerate
/// domain (`domain_lo == domain_hi`) maps every value to the pixel-range
/// midpoint rather than dividing by zero; a degenerate pixel range makes
/// [`LinearScale::invert`] return `domain_lo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearScale {
    domain_lo: f64,
    domain_hi: f64,
    range_lo: f32,
    range_hi: f32,
}

impl LinearScale {
    /// Build a scale from a data `domain` (`(lo, hi)` values) onto a
    /// pixel `range` (`(lo, hi)` device coordinates). The range may be
    /// descending (`lo > hi`) for a y-axis.
    #[must_use]
    pub const fn new(domain: (f64, f64), range: (f32, f32)) -> Self {
        Self {
            domain_lo: domain.0,
            domain_hi: domain.1,
            range_lo: range.0,
            range_hi: range.1,
        }
    }

    /// Map a data `value` to its pixel coordinate. A degenerate domain
    /// returns the pixel-range midpoint.
    #[must_use]
    pub fn map(&self, value: f64) -> f32 {
        let span = self.domain_hi - self.domain_lo;
        let t = if span.abs() < f64::EPSILON {
            0.5
        } else {
            (value - self.domain_lo) / span
        };
        let lo = f64::from(self.range_lo);
        let hi = f64::from(self.range_hi);
        to_f32(lo + t * (hi - lo))
    }

    /// Invert a pixel coordinate back to a data value. A degenerate
    /// pixel range returns `domain_lo`.
    #[must_use]
    pub fn invert(&self, pixel: f32) -> f64 {
        let span = f64::from(self.range_hi) - f64::from(self.range_lo);
        if span.abs() < f64::EPSILON {
            return self.domain_lo;
        }
        let t = (f64::from(pixel) - f64::from(self.range_lo)) / span;
        self.domain_lo + t * (self.domain_hi - self.domain_lo)
    }

    /// The data domain `(lo, hi)` this scale was built with.
    #[must_use]
    pub const fn domain(&self) -> (f64, f64) {
        (self.domain_lo, self.domain_hi)
    }

    /// The pixel range `(lo, hi)` this scale was built with.
    #[must_use]
    pub const fn range(&self) -> (f32, f32) {
        (self.range_lo, self.range_hi)
    }
}

/// The default logarithm base — decades, what every log axis in a
/// monitoring or profiling tool uses (the toolkit's `base` default).
pub const DEFAULT_LOG_BASE: f64 = 10.0;

/// Logarithmic `f64` domain to `f32` pixel-range mapping (and its inverse).
///
/// Built as a [`LinearScale`] over the log-transformed domain, so
/// `map(v) = linear.map(log_b(v))` and `invert(px) = b^linear.invert(px)`
/// — d3's `scaleLog` composition. Equal *ratios* therefore occupy equal
/// pixel distances, which is the whole point: a latency series spanning
/// `0.1 ms .. 1000 ms` shows its sub-millisecond structure instead of
/// collapsing onto the baseline.
///
/// # Partial by nature
///
/// `log_b(v)` is undefined for `v <= 0`, so [`LogScale::map`] returns
/// `None` there rather than inventing a pixel. Callers reach this through
/// [`ValueScale`], whose contract makes the possibility explicit at every
/// use site.
///
/// # Total construction
///
/// The domain must be strictly positive and the base greater than 1. Both
/// are *sanitised* rather than rejected, so the type has no failing
/// constructor: an out-of-contract base falls back to
/// [`DEFAULT_LOG_BASE`], and a non-positive or non-finite domain endpoint
/// falls back to the unit decade `(1, base)`. [`LogScale::domain`] then
/// reports the domain actually in use, so a chart built on a sanitised
/// scale is inspectable rather than silently wrong. The one production
/// caller (`crate::plot`) derives its domain from the positive data, so
/// it does not rely on the fallback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogScale {
    /// The affine scale over `(log_b(domain_lo), log_b(domain_hi))`.
    inner: LinearScale,
    base: f64,
    domain_lo: f64,
    domain_hi: f64,
}

impl LogScale {
    /// Build a log scale over a strictly-positive data `domain` onto a
    /// pixel `range` (descending for a y-axis, exactly as
    /// [`LinearScale::new`]). See the type doc for how an out-of-contract
    /// `base` or `domain` is sanitised.
    #[must_use]
    pub fn new(domain: (f64, f64), range: (f32, f32), base: f64) -> Self {
        let base = if base.is_finite() && base > 1.0 {
            base
        } else {
            DEFAULT_LOG_BASE
        };
        let (lo, hi) = if positive(domain.0) && positive(domain.1) {
            domain
        } else {
            (1.0, base)
        };
        Self {
            inner: LinearScale::new((log_base(lo, base), log_base(hi, base)), range),
            base,
            domain_lo: lo,
            domain_hi: hi,
        }
    }

    /// Map a positive data `value` to its pixel coordinate, or `None` when
    /// the logarithm is undefined there (`value <= 0`, or non-finite).
    ///
    /// Values outside the domain still map, by extrapolation, exactly as
    /// [`LinearScale::map`] does — clipping to the visible range is the
    /// chart's decision, not the scale's.
    #[must_use]
    pub fn map(&self, value: f64) -> Option<f32> {
        positive(value).then(|| self.inner.map(log_base(value, self.base)))
    }

    /// Invert a pixel coordinate back to a data value.
    #[must_use]
    pub fn invert(&self, pixel: f32) -> f64 {
        self.base.powf(self.inner.invert(pixel))
    }

    /// The data domain `(lo, hi)` in use — the constructed one, or the
    /// sanitised fallback described on the type.
    #[must_use]
    pub const fn domain(&self) -> (f64, f64) {
        (self.domain_lo, self.domain_hi)
    }

    /// The pixel range `(lo, hi)` this scale was built with.
    #[must_use]
    pub const fn range(&self) -> (f32, f32) {
        self.inner.range()
    }

    /// The logarithm base in use.
    #[must_use]
    pub const fn base(&self) -> f64 {
        self.base
    }
}

/// The ordered category list one categorical axis carries — the toolkit's
/// `categories`, d3's `scaleBand().domain(names)`.
///
/// Shared (`Arc`) rather than cloned: an [`AxisKind`] is copied into every
/// chart builder, plot resolution and tick format that touches the axis, and
/// a category list is `n` heap strings. Cloning it is a refcount bump.
///
/// Category `i` sits at value `i` on the axis, so the whole list occupies the
/// **extent** `-0.5 ..= n - 0.5`: each category owns one unit-wide band
/// centred on its index. A value outside that extent names no category, and
/// [`spans`](Self::spans) is what every consumer asks — see
/// [`AxisKind::defines`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Categories(Arc<[String]>);

impl Default for Categories {
    fn default() -> Self {
        Self(Arc::from(Vec::new()))
    }
}

impl Categories {
    /// Build a category list from anything string-like, in axis order.
    #[must_use]
    pub fn new<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(names.into_iter().map(Into::into).collect())
    }

    /// How many categories the axis carries (the toolkit's `count`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the axis carries no category at all — an axis that defines no
    /// pixel anywhere.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The category at `index`, or `None` when the index names none.
    ///
    /// The toolkit's `at` answers an out-of-range index with an empty
    /// string, which is indistinguishable from a category whose name IS
    /// empty. An `Option` cannot be misread that way.
    #[must_use]
    pub fn at(&self, index: usize) -> Option<&str> {
        self.0.get(index).map(String::as_str)
    }

    /// The categories in axis order.
    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    /// The axis extent `(-0.5, n - 0.5)` — the numeric domain the whole
    /// category list occupies, each category owning one unit-wide band.
    ///
    /// An empty list answers the unit extent `(-0.5, 0.5)` rather than the
    /// degenerate `(-0.5, -0.5)`, so an axis with no categories still resolves
    /// a drawable (empty) plot instead of collapsing its gridline arithmetic.
    /// Nothing maps on it either way — [`spans`](Self::spans) is false for
    /// every value.
    #[must_use]
    pub fn extent(&self) -> (f64, f64) {
        if self.is_empty() {
            return (-0.5, 0.5);
        }
        (-0.5, index_value(self.len()) - 0.5)
    }

    /// Whether `value` falls inside the axis extent — i.e. whether it names a
    /// category slot (an integer) or a position **between** two of them (what
    /// a line interpolating across categories occupies).
    ///
    /// This is the categorical axis's definedness, and it is stricter than a
    /// linear axis's: a linear axis extrapolates outside its domain because
    /// clipping is the chart's decision, whereas index `99` of a five-category
    /// axis is not a position at all. Such a point is *reported*
    /// ([`crate::OffScale`]) rather than drawn somewhere invented — the R1528
    /// stance for a log axis's zero, applied to the arm where "no such slot"
    /// is the failure. The toolkit draws it off in space with no diagnostic.
    #[must_use]
    pub fn spans(&self, value: f64) -> bool {
        if self.is_empty() || !value.is_finite() {
            return false;
        }
        let (lo, hi) = self.extent();
        value >= lo && value <= hi
    }

    /// Where category `index` sits on the axis — `i` for category `i`, the
    /// centre of the band [`CategoryScale::band`] draws it in. `None` when the
    /// index names no category.
    ///
    /// The x a series takes to plot over this axis, so a consumer building
    /// points never casts an index itself. The toolkit has no peer: a bar
    /// category axis exposes the names and the count, and the position a
    /// category occupies is implicit in the series painter.
    #[must_use]
    pub fn position(&self, index: usize) -> Option<f64> {
        self.at(index).map(|_| index_value(index))
    }

    /// Every category's axis position, in order — the x values a series plots
    /// over this axis.
    pub fn positions(&self) -> impl Iterator<Item = f64> + '_ {
        (0..self.len()).map(index_value)
    }

    /// The index of the category named `name` — the toolkit's implicit
    /// `indexOf` behind `setMin` / `setMax`.
    ///
    /// # Errors
    ///
    /// [`CategoryLookup::Unknown`] when no category has the name, and [`CategoryLookup::Ambiguous`] when more than one does.
    /// The toolkit answers both silently: an unknown name makes `setMin` a no-op
    /// the caller cannot detect, and a duplicated one resolves to the first
    /// match, so `setRange("a", "a")` over `["a", "b", "a"]` collapses the axis to one slot with nothing
    /// said.
    pub fn index_of(&self, name: &str) -> Result<usize, CategoryLookup> {
        let mut found = None;
        let mut count = 0_usize;
        for (i, c) in self.0.iter().enumerate() {
            if c == name {
                count += 1;
                found.get_or_insert(i);
            }
        }
        match (found, count) {
            (Some(i), 1) => Ok(i),
            (Some(_), n) => Err(CategoryLookup::Ambiguous {
                name: name.to_string(),
                count: n,
            }),
            _ => Err(CategoryLookup::Unknown(name.to_string())),
        }
    }

    /// The index window between two category NAMES — the toolkit's
    /// `setRange(min, max)`, resolved rather than applied.
    ///
    /// The endpoints may be given in either order; the window is normalised.
    ///
    /// # Errors
    ///
    /// The first endpoint that does not resolve, per [`index_of`](Self::index_of).
    /// Returning the failure is the whole point: the toolkit's `setRange` is `void`, so
    /// a typo'd category name leaves the axis silently unwindowed, and here it
    /// cannot reach the chart without the caller deciding what to do with it.
    pub fn window(&self, from: &str, to: &str) -> Result<CategoryWindow, CategoryLookup> {
        let a = self.index_of(from)?;
        let b = self.index_of(to)?;
        Ok(CategoryWindow::new(a, b))
    }
}

/// Why a category name did not resolve to a slot (R1545).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryLookup {
    /// No category carries this name.
    Unknown(String),
    /// More than one category carries this name, so it names no single slot.
    Ambiguous {
        /// The ambiguous name.
        name: String,
        /// How many categories carry it.
        count: usize,
    },
}

impl std::fmt::Display for CategoryLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(name) => write!(f, "no category named {name:?}"),
            Self::Ambiguous { name, count } => {
                write!(f, "{count} categories named {name:?}")
            }
        }
    }
}

impl std::error::Error for CategoryLookup {}

/// An inclusive index window over a [`Categories`] list — the resolved form
/// of the toolkit's `setRange`.
///
/// Normalised at construction (reversed endpoints swap), so a window is never
/// in an invalid state to be repaired at read time — the [`crate::PlotWindow`]
/// stance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategoryWindow {
    lo: usize,
    hi: usize,
}

impl CategoryWindow {
    /// The window between two indices, in either order.
    #[must_use]
    pub const fn new(a: usize, b: usize) -> Self {
        if a <= b {
            Self { lo: a, hi: b }
        } else {
            Self { lo: b, hi: a }
        }
    }

    /// The first category index in the window.
    #[must_use]
    pub const fn lo(self) -> usize {
        self.lo
    }

    /// The last category index in the window (inclusive).
    #[must_use]
    pub const fn hi(self) -> usize {
        self.hi
    }

    /// How many categories the window holds.
    #[must_use]
    pub const fn len(self) -> usize {
        self.hi - self.lo + 1
    }

    /// Always false — a window holds at least the one slot it was built from.
    /// Present because clippy asks any type with a `len` for it, and answering
    /// honestly is better than suppressing the question.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        false
    }

    /// The numeric axis domain this window pins — `(lo - 0.5, hi + 0.5)`, the
    /// band edges of its end categories. This is what a chart pins as its
    /// `x_domain`, so a category window and a [`crate::PlotWindow`] zoom are
    /// the same representation and cannot disagree.
    #[must_use]
    pub fn domain(self) -> (f64, f64) {
        (index_value(self.lo) - 0.5, index_value(self.hi) + 0.5)
    }
}

/// Categorical `f64` index to `f32` pixel-range mapping — the toolkit's
/// bar category axis, d3's `scaleBand`.
///
/// Affine in the category index (see the module doc), so it holds a
/// [`LinearScale`] over the numeric domain and adds the two things a
/// categorical axis has that a numeric one does not: the category **names**,
/// and the per-category **band**.
///
/// # The band is published (past the toolkit 6.11)
///
/// [`band`](Self::band) answers "which pixels does category `i` occupy". The
/// toolkit's bar category axis has no such accessor: a bar's rect is computed
/// inside the private series painter (bar series private), so a toolkit
/// application cannot ask where a category is drawn, and anything that needs
/// to align to one re-derives the arithmetic. That is exactly what happened
/// here before R1545 — `bar.rs` carried three copies of `left + i * slot`, one for the bar box,
/// one for the label, one for the click surface, and the crate's own doc
/// called that "the ONE definition of where bar `i` sits".
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryScale {
    /// The affine scale over the numeric index domain.
    inner: LinearScale,
    categories: Categories,
}

impl CategoryScale {
    /// Build a categorical scale over `domain` (a numeric index range — the
    /// whole [`Categories::extent`], or a [`CategoryWindow::domain`], or a
    /// [`crate::PlotWindow`] zoom) onto the pixel `range`, descending for a
    /// y-axis exactly as [`LinearScale::new`].
    #[must_use]
    pub fn new(categories: Categories, domain: (f64, f64), range: (f32, f32)) -> Self {
        Self {
            inner: LinearScale::new(domain, range),
            categories,
        }
    }

    /// Map a category index (or a fractional position between two) to its
    /// pixel, or `None` when the value names no position on this axis
    /// ([`Categories::spans`]).
    #[must_use]
    pub fn map(&self, value: f64) -> Option<f32> {
        self.categories.spans(value).then(|| self.inner.map(value))
    }

    /// Invert a pixel back to a fractional category index.
    #[must_use]
    pub fn invert(&self, pixel: f32) -> f64 {
        self.inner.invert(pixel)
    }

    /// The numeric index domain in view.
    #[must_use]
    pub const fn domain(&self) -> (f64, f64) {
        self.inner.domain()
    }

    /// The pixel range this scale was built with.
    #[must_use]
    pub const fn range(&self) -> (f32, f32) {
        self.inner.range()
    }

    /// The categories this axis carries.
    #[must_use]
    pub const fn categories(&self) -> &Categories {
        &self.categories
    }

    /// The pixel span `(lo, hi)` category `index` occupies — its band. `None`
    /// when the index names no category.
    ///
    /// Reported for any category the axis carries, in view or not: the band is
    /// where the category *is*, and whether it is on screen is the chart's
    /// clip decision. Always ascending, so a descending (y-axis) pixel range
    /// still answers a span rather than a reversed pair.
    #[must_use]
    pub fn band(&self, index: usize) -> Option<(f32, f32)> {
        self.categories.at(index)?;
        let centre = index_value(index);
        let a = self.inner.map(centre - 0.5);
        let b = self.inner.map(centre + 0.5);
        Some((a.min(b), a.max(b)))
    }

    /// The width (px) of one category band — d3's `scaleBand().bandwidth()`.
    #[must_use]
    pub fn band_width(&self) -> f32 {
        (self.inner.map(1.0) - self.inner.map(0.0)).abs()
    }

    /// The inclusive index range whose categories are **in view**: those whose
    /// centre falls inside the domain. `None` when the axis carries no
    /// category, or when the window has closed past every one of them.
    ///
    /// Centre-in-domain rather than band-intersects-domain so that a window
    /// pinned to exactly `(lo - 0.5, hi + 0.5)` — what
    /// [`CategoryWindow::domain`] produces — holds exactly `lo..=hi` instead
    /// of touching its neighbours at the shared band edge.
    ///
    /// The toolkit cannot answer this. bar category axis reports `count()` (every
    /// category) and `min()` / `max()` (the names it was *set* to), never which
    /// categories a live window is showing — and since R1534 the window here
    /// moves under a wheel gesture, so the question has an answer that changes
    /// per frame.
    #[must_use]
    pub fn visible(&self) -> Option<CategoryWindow> {
        let n = self.categories.len();
        if n == 0 {
            return None;
        }
        let (d0, d1) = self.inner.domain();
        let (lo_v, hi_v) = if d0 <= d1 { (d0, d1) } else { (d1, d0) };
        let last = index_value(n - 1);
        let lo = lo_v.ceil().clamp(0.0, last);
        let hi = hi_v.floor().clamp(0.0, last);
        // A domain entirely outside `0..=last` clamps both ends onto the same
        // edge, which would report one visible category where none is.
        if hi < lo || hi_v < 0.0 || lo_v > last {
            return None;
        }
        Some(CategoryWindow::new(to_usize(lo), to_usize(hi)))
    }

    /// The index of the category nearest `pixel`, clamped into the visible
    /// window — the categorical hit-test. `None` when nothing is in view.
    #[must_use]
    pub fn nearest(&self, pixel: f32) -> Option<usize> {
        let window = self.visible()?;
        let v = self.inner.invert(pixel);
        if !v.is_finite() {
            return Some(window.lo());
        }
        let lo = index_value(window.lo());
        let hi = index_value(window.hi());
        Some(to_usize(v.round().clamp(lo, hi)))
    }
}

/// Which KIND one chart axis is — the toolkit's value axis / log value axis /
/// date time axis / bar category axis choice, without a domain or a pixel
/// range attached.
///
/// A chart knows its axis kinds before a plot is resolved (it needs them
/// to report its off-scale points), and everything that depends only on
/// the kind — definedness, which tick generator runs, which label format
/// applies — is decided from here so those rules have one spelling.
///
/// R1529 made this a type. It was a `bool` (`is_log`) plus an
/// `Option<f64>` base while there were two kinds, which is what a boolean
/// discriminator always is: an encoding that fits exactly two arms. The
/// third arm is where it stops fitting — `is_log == false` would have had
/// to mean "linear or time", and every site that branched on it would
/// have silently taken the linear path for a time axis.
///
/// R1545 dropped `Copy` for the same class of reason one level up. Every
/// kind up to the third was a scalar — nothing, a base, nothing — so a kind
/// was a machine word and copying it was free. A categorical axis's identity
/// IS its category list, so the fourth arm carries one (shared, so a clone is
/// a refcount bump). Keeping `Copy` would have meant keeping the categories
/// *outside* the kind, where the one thing that decides how a tick is
/// labelled would not be reachable from the thing that decides which label
/// format applies.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AxisKind {
    /// An affine axis over a plain number.
    #[default]
    Linear,
    /// A logarithmic axis in the given base.
    Log(f64),
    /// A time axis over epoch **milliseconds**, UTC — the unit the toolkit's
    /// date time axis and d3's `scaleUtc` both carry. Affine in the
    /// instant, so it maps through the same [`LinearScale`] arithmetic a
    /// [`Linear`](Self::Linear) axis does; what differs is entirely which
    /// ticks it lands on and how they are labelled.
    Time,
    /// A categorical axis over discrete, named slots — the toolkit's
    /// bar category axis, d3's `scaleBand`. Affine in the category *index*,
    /// so like [`Time`](Self::Time) it reuses [`LinearScale`] untouched; what
    /// differs is that its ticks are the indices, its labels are the names it
    /// carries, and a value naming no slot has no pixel.
    Category(Categories),
}

impl AxisKind {
    /// Whether an axis of this kind defines a pixel for `value`.
    #[must_use]
    pub fn defines(&self, value: f64) -> bool {
        match self {
            Self::Log(_) => positive(value),
            Self::Category(c) => c.spans(value),
            Self::Linear | Self::Time => value.is_finite(),
        }
    }

    /// The logarithm base, for the one kind that has one.
    #[must_use]
    pub const fn log_base(&self) -> Option<f64> {
        match self {
            Self::Log(base) => Some(*base),
            _ => None,
        }
    }

    /// The category list, for the one kind that has one.
    #[must_use]
    pub const fn categories(&self) -> Option<&Categories> {
        match self {
            Self::Category(c) => Some(c),
            _ => None,
        }
    }
}

/// The scale of one chart axis: linear, logarithmic, time, or categorical.
///
/// This is the axis-level abstraction (the toolkit's abstract axis
/// distinction), distinct from the [`LinearScale`] arithmetic it is built on — see the
/// module doc. A closed enum rather than a trait object because the set of
/// axis scales is closed.
///
/// [`map`](Self::map) is **partial**: a log axis has no pixel for a
/// non-positive value, a categorical axis none for a value naming no slot,
/// and a linear or time axis none for a non-finite one. There is deliberately
/// no total variant — a caller that silently substituted a pixel would be
/// drawing a point the data does not contain, which is the failure
/// [`crate::Mapped::unreadable`] exists to avoid on the other input path.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueScale {
    /// An affine axis — equal value differences occupy equal pixel spans.
    Linear(LinearScale),
    /// A logarithmic axis — equal value *ratios* occupy equal pixel spans.
    Log(LogScale),
    /// A time axis over epoch milliseconds (R1529). Carries a
    /// [`LinearScale`] because time IS affine — equal *durations* occupy
    /// equal pixel spans — so unlike [`Log`](Self::Log) there is no
    /// transform to compose. That the arithmetic core is reused untouched
    /// by a third axis kind is the two-layer split of R1528 paying off.
    Time(LinearScale),
    /// A categorical axis over named slots (R1545) — the toolkit's
    /// bar category axis. Affine in the category index, and the first arm
    /// whose scale carries *data* (the names) rather than only a parameter.
    Category(CategoryScale),
}

impl ValueScale {
    /// Map a data `value` to its pixel coordinate, or `None` when this
    /// axis does not define one there.
    #[must_use]
    pub fn map(&self, value: f64) -> Option<f32> {
        match self {
            Self::Linear(s) | Self::Time(s) => value.is_finite().then(|| s.map(value)),
            Self::Log(s) => s.map(value),
            Self::Category(s) => s.map(value),
        }
    }

    /// Invert a pixel coordinate back to a data value.
    #[must_use]
    pub fn invert(&self, pixel: f32) -> f64 {
        match self {
            Self::Linear(s) | Self::Time(s) => s.invert(pixel),
            Self::Log(s) => s.invert(pixel),
            Self::Category(s) => s.invert(pixel),
        }
    }

    /// Whether this axis defines a pixel for `value` — the predicate form
    /// of [`map`](Self::map), for the call sites that partition data
    /// before mapping it.
    #[must_use]
    pub fn defines(&self, value: f64) -> bool {
        self.kind().defines(value)
    }

    /// The data domain `(lo, hi)` of this axis.
    #[must_use]
    pub const fn domain(&self) -> (f64, f64) {
        match self {
            Self::Linear(s) | Self::Time(s) => s.domain(),
            Self::Log(s) => s.domain(),
            Self::Category(s) => s.domain(),
        }
    }

    /// The pixel range `(lo, hi)` of this axis.
    #[must_use]
    pub const fn range(&self) -> (f32, f32) {
        match self {
            Self::Linear(s) | Self::Time(s) => s.range(),
            Self::Log(s) => s.range(),
            Self::Category(s) => s.range(),
        }
    }

    /// Which [`AxisKind`] this scale is — what the tick generator, the
    /// domain snapper and the label format branch on.
    #[must_use]
    pub fn kind(&self) -> AxisKind {
        match self {
            Self::Linear(_) => AxisKind::Linear,
            Self::Log(s) => AxisKind::Log(s.base()),
            Self::Time(_) => AxisKind::Time,
            Self::Category(s) => AxisKind::Category(s.categories().clone()),
        }
    }

    /// The categorical scale behind a [`Category`](Self::Category) axis —
    /// the accessor for the geometry only that kind has ([`band`], the
    /// visible window, the hit-test). `None` on every numeric kind.
    ///
    /// [`band`]: CategoryScale::band
    #[must_use]
    pub const fn category(&self) -> Option<&CategoryScale> {
        match self {
            Self::Category(s) => Some(s),
            _ => None,
        }
    }
}

/// Whether `v` is a value a logarithm is defined at: finite and `> 0`.
pub(crate) const fn positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// `log_base(v, b)` — the logarithm of `v` in base `b`. Callers guarantee
/// `v > 0` and `b > 1`.
pub(crate) fn log_base(v: f64, base: f64) -> f64 {
    v.ln() / base.ln()
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "f64 pixel arithmetic narrowed to the f32 PathPoint coordinate space; sub-pixel loss is expected and bounded by the device resolution"
)]
pub(crate) fn to_f32(v: f64) -> f32 {
    v as f32
}

/// A category index — or a count of them — as an `f64` axis value. Category
/// `i` sits at `i`, so the two conversions are the same one.
///
/// One definition rather than three (R1545): this reached the round's own
/// self-grep as a private copy in `scale`, `plot` and `bar`, which is the
/// mechanical-duplication shape the crate lifts on sight.
#[allow(
    clippy::cast_precision_loss,
    reason = "a category index or count is a display cardinality, far inside f64's exact integer range"
)]
pub(crate) fn index_value(index: usize) -> f64 {
    index as f64
}

/// `v` as an index. Callers pass a value already clamped into `0..=len-1`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped by the caller into a valid category index range, so the truncation is exact and the value non-negative"
)]
fn to_usize(v: f64) -> usize {
    v as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }
    fn close64(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn maps_endpoints_and_midpoint() {
        let s = LinearScale::new((0.0, 100.0), (0.0, 200.0));
        assert!(close(s.map(0.0), 0.0));
        assert!(close(s.map(100.0), 200.0));
        assert!(close(s.map(50.0), 100.0));
    }

    #[test]
    fn descending_range_puts_larger_values_higher() {
        // y-axis: value 0 -> bottom (300px), value 10 -> top (20px).
        let s = LinearScale::new((0.0, 10.0), (300.0, 20.0));
        assert!(close(s.map(0.0), 300.0));
        assert!(close(s.map(10.0), 20.0));
        assert!(
            s.map(10.0) < s.map(0.0),
            "larger value maps higher (smaller px)"
        );
    }

    #[test]
    fn invert_round_trips() {
        let s = LinearScale::new((-5.0, 5.0), (10.0, 410.0));
        for v in [-5.0, -1.0, 0.0, 2.5, 5.0] {
            let px = s.map(v);
            assert!(close64(s.invert(px), v), "round-trip {v}");
        }
    }

    #[test]
    fn degenerate_domain_maps_to_range_midpoint() {
        let s = LinearScale::new((7.0, 7.0), (0.0, 100.0));
        assert!(close(s.map(7.0), 50.0));
        assert!(close(s.map(999.0), 50.0));
    }

    #[test]
    fn degenerate_range_inverts_to_domain_lo() {
        let s = LinearScale::new((3.0, 9.0), (50.0, 50.0));
        assert!(close64(s.invert(50.0), 3.0));
    }

    #[test]
    fn accessors_echo_construction() {
        let s = LinearScale::new((1.0, 2.0), (3.0, 4.0));
        assert_eq!(s.domain(), (1.0, 2.0));
        assert_eq!(s.range(), (3.0, 4.0));
    }

    // ---- R1528: the logarithmic axis -----------------------------------

    #[test]
    fn r1528_equal_ratios_occupy_equal_pixel_spans() {
        // Three decades over 300px: each decade is exactly 100px, which is
        // the defining property a linear axis cannot have.
        let s = LogScale::new((1.0, 1000.0), (0.0, 300.0), DEFAULT_LOG_BASE);
        assert!(close(s.map(1.0).unwrap(), 0.0));
        assert!(close(s.map(10.0).unwrap(), 100.0));
        assert!(close(s.map(100.0).unwrap(), 200.0));
        assert!(close(s.map(1000.0).unwrap(), 300.0));
        // The same RATIO anywhere in the domain spans the same pixels.
        let decade = s.map(20.0).unwrap() - s.map(2.0).unwrap();
        assert!(close(decade, 100.0), "2->20 spans a decade like 1->10");
    }

    #[test]
    fn r1528_descending_range_works_for_a_y_axis() {
        // y-axis: domain-lo -> bottom (300px), domain-hi -> top (20px).
        let s = LogScale::new((0.1, 100.0), (300.0, 20.0), DEFAULT_LOG_BASE);
        assert!(close(s.map(0.1).unwrap(), 300.0));
        assert!(close(s.map(100.0).unwrap(), 20.0));
        assert!(s.map(100.0).unwrap() < s.map(0.1).unwrap());
    }

    #[test]
    fn r1528_log_invert_round_trips() {
        let s = LogScale::new((0.01, 1000.0), (10.0, 410.0), DEFAULT_LOG_BASE);
        for v in [0.01, 0.5, 1.0, 42.0, 1000.0] {
            let px = s.map(v).expect("positive value maps");
            let back = s.invert(px);
            assert!(
                (back - v).abs() <= 1e-6 * v.abs().max(1.0),
                "round-trip {v} -> {px}px -> {back}"
            );
        }
    }

    /// ★ The reason [`ValueScale::map`] is partial. A log axis has no pixel
    /// for zero or a negative sample; returning one would draw a point the
    /// data does not contain.
    #[test]
    fn r1528_nonpositive_has_no_pixel_rather_than_the_baseline() {
        let s = LogScale::new((1.0, 1000.0), (300.0, 0.0), DEFAULT_LOG_BASE);
        for v in [0.0, -1.0, -0.0, f64::NEG_INFINITY, f64::NAN] {
            assert_eq!(s.map(v), None, "log has no pixel for {v}");
            assert!(!ValueScale::Log(s).defines(v));
        }
        // The counterfactual that makes the claim worth anything: the
        // baseline pixel IS a real, reachable pixel for a positive value,
        // so "map to the baseline" would have been indistinguishable from
        // a genuine sample at the domain floor.
        assert_eq!(s.map(1.0), Some(300.0));
    }

    #[test]
    fn r1528_linear_axis_is_partial_only_at_non_finite() {
        let s = ValueScale::Linear(LinearScale::new((0.0, 10.0), (0.0, 100.0)));
        assert_eq!(s.map(-5.0), Some(-50.0), "negatives are ordinary here");
        assert_eq!(s.map(f64::NAN), None);
        assert_eq!(s.map(f64::INFINITY), None);
        assert_eq!(s.kind(), AxisKind::Linear);
    }

    // ---- R1529: the time axis ------------------------------------------

    /// ★ A time axis reuses the affine core untouched — that is the claim
    /// the [`ValueScale::Time`] arm makes, and it is checkable: its mapping
    /// must agree with a [`LinearScale`] over the same domain everywhere.
    /// If it did not, the arm would be hiding arithmetic rather than policy.
    #[test]
    fn r1529_time_axis_maps_exactly_as_the_affine_core_does() {
        let domain = (1_772_582_400_000.0, 1_772_586_000_000.0); // one hour
        let inner = LinearScale::new(domain, (0.0, 300.0));
        let axis = ValueScale::Time(inner);
        for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let v = domain.0 + frac * (domain.1 - domain.0);
            assert_eq!(
                axis.map(v),
                Some(inner.map(v)),
                "time is affine in the instant"
            );
        }
        assert_eq!(axis.domain(), domain);
        assert_eq!(axis.range(), (0.0, 300.0));
        assert!(close64(axis.invert(150.0), inner.invert(150.0)));
        assert_eq!(axis.kind(), AxisKind::Time);
    }

    /// ★ The reason [`AxisKind`] is a type rather than the `is_log` boolean
    /// it replaced: with three kinds, `!is_log` no longer names one of them.
    /// A time axis carries negative values (any instant before 1970) and
    /// must not be mistaken for a log axis, which cannot.
    #[test]
    fn r1529_axis_kind_names_three_arms_where_a_bool_named_two() {
        assert!(AxisKind::Time.defines(-86_400_000.0), "1969 is an instant");
        assert!(AxisKind::Linear.defines(-86_400_000.0));
        assert!(!AxisKind::Log(10.0).defines(-86_400_000.0));
        for k in [AxisKind::Linear, AxisKind::Log(10.0), AxisKind::Time] {
            assert!(!k.defines(f64::NAN), "{k:?} defines no pixel for NaN");
            assert!(!k.defines(f64::INFINITY));
        }
        assert_eq!(AxisKind::Log(2.0).log_base(), Some(2.0));
        assert_eq!(AxisKind::Time.log_base(), None);
        assert_eq!(AxisKind::Linear.log_base(), None);
        assert_eq!(AxisKind::default(), AxisKind::Linear);
    }

    // ---- R1545: the categorical axis ------------------------------------

    fn months() -> Categories {
        Categories::new(["Jan", "Feb", "Mar", "Apr", "May", "Jun"])
    }

    /// The full-extent axis over `months()` on a 600px plot: six unit-wide
    /// bands, 100px each.
    fn month_axis() -> CategoryScale {
        let c = months();
        CategoryScale::new(c.clone(), c.extent(), (0.0, 600.0))
    }

    /// ★ The band is the slot, and it is the axis that says so. Adjacent
    /// bands must TOUCH and each be exactly one slot wide — the property
    /// `bar.rs` used to get by writing `left + i * slot` itself, three times.
    #[test]
    fn r1545_the_band_is_the_slot_and_the_bands_tile_the_axis() {
        let s = month_axis();
        assert!(close(s.band_width(), 100.0), "600px / 6 categories");
        for i in 0..6 {
            let (lo, hi) = s.band(i).expect("every category has a band");
            assert!(close(hi - lo, 100.0), "band {i} is one slot wide");
            // The category's own value sits at the band centre.
            let centre = s.map(f64::from(i32::try_from(i).unwrap())).unwrap();
            assert!(
                close(centre, f32::midpoint(lo, hi)),
                "index {i} is band-centred"
            );
        }
        // Tiling: band k's right edge IS band k+1's left edge, and the run
        // covers the whole pixel range with nothing left over.
        for i in 0..5 {
            assert!(close(s.band(i).unwrap().1, s.band(i + 1).unwrap().0));
        }
        assert!(close(s.band(0).unwrap().0, 0.0));
        assert!(close(s.band(5).unwrap().1, 600.0));
        assert_eq!(s.band(6), None, "no seventh category, so no seventh band");
    }

    /// ★ A descending (y-axis) pixel range still answers a SPAN, not a
    /// reversed pair — a horizontal-bar consumer reads `(lo, hi)` and would
    /// otherwise compute a negative height.
    #[test]
    fn r1545_a_descending_range_still_answers_an_ascending_band() {
        let c = months();
        let s = CategoryScale::new(c.clone(), c.extent(), (600.0, 0.0));
        for i in 0..6 {
            let (lo, hi) = s.band(i).unwrap();
            assert!(lo < hi, "band {i} is ({lo}, {hi})");
        }
        assert!(close(s.band_width(), 100.0), "width is unsigned");
        // Category 0 is at the BOTTOM of a descending range.
        assert!(s.map(0.0).unwrap() > s.map(5.0).unwrap());
    }

    /// ★ The reason [`CategoryScale::map`] is partial, and the counterfactual
    /// that makes the claim mean something: the boundary values ±0.5 and every
    /// fraction between two slots DO map (that is where a line segment
    /// crossing categories lives), so "reject anything non-integral" would
    /// pass a naive test and break the line chart.
    #[test]
    fn r1545_a_value_naming_no_slot_has_no_pixel() {
        let s = month_axis();
        let kind = AxisKind::Category(months());
        for v in [9.0, 6.0, -1.0, -0.51, 5.51, f64::NAN, f64::INFINITY] {
            assert_eq!(s.map(v), None, "{v} names no slot");
            assert!(!kind.defines(v), "{v} is off-scale for the axis kind");
        }
        // The counterfactual: the extent edges and the fractions between
        // slots are ordinary positions.
        for v in [-0.5, 0.0, 2.5, 3.25, 5.0, 5.5] {
            assert!(s.map(v).is_some(), "{v} is a position on the axis");
            assert!(kind.defines(v));
        }
        // And an empty axis defines nothing at all, including index 0.
        let empty = AxisKind::Category(Categories::default());
        assert!(!empty.defines(0.0));
        assert_eq!(empty.categories().map(Categories::len), Some(0));
    }

    /// ★ A window narrows what is in view AND widens the band — the two are
    /// the same fact, which is why the bars, their labels and their hit
    /// regions can only agree.
    #[test]
    fn r1545_a_window_narrows_the_view_and_widens_the_band() {
        let c = months();
        let window = c.window("Mar", "May").expect("three named months");
        assert_eq!((window.lo(), window.hi(), window.len()), (2, 4, 3));

        let s = CategoryScale::new(c.clone(), window.domain(), (0.0, 600.0));
        assert_eq!(s.visible(), Some(window), "exactly the windowed slots");
        assert!(close(s.band_width(), 200.0), "600px / 3 visible");
        assert!(close(s.band(2).unwrap().0, 0.0), "Mar starts at the axis");
        assert!(close(s.band(4).unwrap().1, 600.0), "May ends at it");
        // A category outside the window still HAS a band — where it would be
        // drawn — because whether it is on screen is the chart's clip
        // decision, not the axis's.
        let (feb_lo, feb_hi) = s.band(1).expect("Feb is still a category");
        assert!(
            feb_hi <= 0.0,
            "Feb sits off the left edge: {feb_lo}..{feb_hi}"
        );
        // Reversed endpoints normalise rather than collapse the axis.
        assert_eq!(c.window("May", "Mar").unwrap(), window);
    }

    /// ★ Windowing past the end of the list reports nothing visible rather
    /// than clamping onto one arbitrary slot — the failure a naive
    /// `clamp(0, last)` on both ends produces.
    #[test]
    fn r1545_a_window_off_the_list_shows_nothing() {
        let c = months();
        let past = CategoryScale::new(c.clone(), (7.5, 11.5), (0.0, 600.0));
        assert_eq!(past.visible(), None, "beyond the last category");
        assert_eq!(past.nearest(300.0), None, "and nothing to hit-test");
        let before = CategoryScale::new(c.clone(), (-9.5, -2.5), (0.0, 600.0));
        assert_eq!(before.visible(), None, "before the first category");
        // A window that merely touches the list still shows it.
        let touching = CategoryScale::new(c, (4.5, 9.5), (0.0, 600.0));
        assert_eq!(touching.visible(), Some(CategoryWindow::new(5, 5)));
    }

    /// ★ Past the toolkit 6.11: `setRange(string, string)` returns
    /// `void`, so a name that is not a category leaves the axis silently
    /// unwindowed, and a duplicated one resolves to the first match with
    /// nothing said. Here neither can reach a chart without being handled.
    #[test]
    fn r1545_a_name_that_does_not_resolve_says_why() {
        let c = months();
        assert_eq!(c.index_of("Mar"), Ok(2));
        assert_eq!(
            c.index_of("Smarch"),
            Err(CategoryLookup::Unknown("Smarch".to_string()))
        );
        // The window reports the FIRST endpoint that failed, so a caller is
        // told which of the two it typed wrong.
        assert_eq!(
            c.window("Smarch", "Jun"),
            Err(CategoryLookup::Unknown("Smarch".to_string()))
        );
        assert_eq!(
            c.window("Jan", "Nope"),
            Err(CategoryLookup::Unknown("Nope".to_string()))
        );

        let dupes = Categories::new(["a", "b", "a"]);
        assert_eq!(
            dupes.index_of("a"),
            Err(CategoryLookup::Ambiguous {
                name: "a".to_string(),
                count: 2,
            }),
            "a duplicated name names no single slot"
        );
        assert_eq!(dupes.index_of("b"), Ok(1), "the unique one still resolves");
        // Both failures render, so a consumer can put the reason on screen.
        assert_eq!(
            c.index_of("Smarch").unwrap_err().to_string(),
            "no category named \"Smarch\""
        );
        assert_eq!(
            dupes.index_of("a").unwrap_err().to_string(),
            "2 categories named \"a\""
        );
    }

    /// ★ The hit-test resolves inside the WINDOW, which is what makes a
    /// windowed chart's inspect land on the bar under the cursor rather than
    /// on a fraction of all sixty categories.
    #[test]
    fn r1545_nearest_resolves_inside_the_window() {
        let s = month_axis();
        assert_eq!(s.nearest(50.0), Some(0), "the middle of Jan's band");
        assert_eq!(s.nearest(149.0), Some(1), "just past Feb's left edge");
        assert_eq!(s.nearest(599.0), Some(5));
        // Outside the plot clamps to an end rather than inventing an index.
        assert_eq!(s.nearest(-400.0), Some(0));
        assert_eq!(s.nearest(4000.0), Some(5));

        let c = months();
        let windowed = CategoryScale::new(
            c.clone(),
            c.window("Mar", "May").unwrap().domain(),
            (0.0, 600.0),
        );
        assert_eq!(windowed.nearest(50.0), Some(2), "the window's first slot");
        assert_eq!(windowed.nearest(550.0), Some(4), "its last");
        // The counterfactual: on the un-windowed axis the same pixel is a
        // different category, so the window is doing the work.
        assert_ne!(windowed.nearest(550.0), s.nearest(550.0));
        assert_eq!(
            CategoryScale::new(Categories::default(), (-0.5, 0.5), (0.0, 9.0)).nearest(4.0),
            None
        );
    }

    /// ★ The kind round-trips through the scale: `ValueScale::kind()` has to
    /// hand back the categories, or a plot could not report its own
    /// off-scale points.
    #[test]
    fn r1545_the_scale_carries_its_categories_through_the_kind() {
        let s = ValueScale::Category(month_axis());
        assert_eq!(s.kind(), AxisKind::Category(months()));
        assert_eq!(s.domain(), (-0.5, 5.5));
        assert_eq!(s.range(), (0.0, 600.0));
        assert_eq!(s.map(1.0), Some(150.0));
        assert_eq!(s.map(6.0), None);
        assert!(close64(s.invert(150.0), 1.0));
        assert_eq!(
            s.category().map(|c| c.categories().at(3)),
            Some(Some("Apr"))
        );
        // The numeric kinds have no categorical geometry to offer.
        assert!(
            ValueScale::Linear(LinearScale::new((0.0, 1.0), (0.0, 1.0)))
                .category()
                .is_none()
        );
        assert_eq!(AxisKind::Time.categories(), None);
        assert_eq!(AxisKind::Category(months()).log_base(), None);
    }

    /// The list accessors, including the one the toolkit cannot express: an
    /// out-of-range `at` is `None` rather than an empty string that a
    /// legitimately-empty category name is indistinguishable from.
    #[test]
    fn r1545_the_category_list_reports_its_own_shape() {
        let c = months();
        assert_eq!(c.len(), 6);
        assert!(!c.is_empty());
        assert_eq!(c.at(0), Some("Jan"));
        assert_eq!(c.at(6), None);
        assert_eq!(c.extent(), (-0.5, 5.5));
        // The axis position of a slot, and the whole run of them — what a
        // series plots over, so no consumer casts an index itself.
        assert_eq!(c.position(0), Some(0.0));
        assert_eq!(c.position(5), Some(5.0));
        assert_eq!(c.position(6), None, "no sixth slot, so no sixth position");
        assert_eq!(
            c.positions().collect::<Vec<_>>(),
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]
        );
        // Each position is the centre of the band that draws it.
        let s = month_axis();
        for (i, x) in c.positions().enumerate() {
            let (lo, hi) = s.band(i).unwrap();
            assert!(close(s.map(x).unwrap(), f32::midpoint(lo, hi)));
        }
        assert_eq!(Categories::default().positions().count(), 0);
        assert_eq!(c.as_slice().len(), 6);

        let blank = Categories::new(["", "x"]);
        assert_eq!(blank.at(0), Some(""), "an empty NAME is a category");
        assert_eq!(blank.at(9), None, "an empty SLOT is not");

        let empty = Categories::default();
        assert!(empty.is_empty());
        assert_eq!(empty.extent(), (-0.5, 0.5), "still a drawable extent");
        assert!(!empty.spans(0.0));

        let w = CategoryWindow::new(4, 4);
        assert_eq!((w.lo(), w.hi(), w.len(), w.is_empty()), (4, 4, 1, false));
        assert_eq!(w.domain(), (3.5, 4.5));
    }

    #[test]
    fn r1528_out_of_contract_construction_is_sanitised_and_reported() {
        // A non-positive endpoint cannot be a log domain; the fallback is
        // the unit decade, and `domain()` reports what is actually in use
        // rather than echoing the impossible request.
        let s = LogScale::new((0.0, 100.0), (0.0, 100.0), DEFAULT_LOG_BASE);
        assert_eq!(s.domain(), (1.0, 10.0));
        assert!(close64(s.base(), 10.0));
        // A base of 1 has no logarithm; 0 and NaN neither.
        for bad in [1.0, 0.0, -2.0, f64::NAN] {
            let fixed = LogScale::new((1.0, 8.0), (0.0, 90.0), bad);
            assert!(
                close64(fixed.base(), 10.0),
                "base {bad} is not a logarithm base"
            );
        }
        // A legal non-decimal base is kept: base 2 puts 8 at three steps.
        let two = LogScale::new((1.0, 8.0), (0.0, 90.0), 2.0);
        assert!(close64(two.base(), 2.0));
        assert!(close(two.map(2.0).unwrap(), 30.0));
        assert!(close(two.map(4.0).unwrap(), 60.0));
    }

    #[test]
    fn r1528_value_scale_delegates_domain_and_range() {
        let lin = ValueScale::Linear(LinearScale::new((1.0, 2.0), (3.0, 4.0)));
        assert_eq!(lin.domain(), (1.0, 2.0));
        assert_eq!(lin.range(), (3.0, 4.0));
        let log = ValueScale::Log(LogScale::new((1.0, 100.0), (5.0, 6.0), 10.0));
        assert_eq!(log.domain(), (1.0, 100.0));
        assert_eq!(log.range(), (5.0, 6.0));
        assert_eq!(log.kind(), AxisKind::Log(10.0));
        assert!(close64(log.invert(5.0), 1.0));
    }
}
