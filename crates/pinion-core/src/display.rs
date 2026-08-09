//! R1576 §5.16 §5.41 §2 #7 — the **desk** the windows sit on.
//!
//! Every window axis this framework had before R1576 described a window: its
//! size, its place (`WindowSpec::position`), its chrome, its title. None of them
//! described the *space those places are in*. A binding could say "put the
//! torn-off panel at `(2600, 40)`" and had no way to ask whether any display
//! reaches that far — so the commonest multi-monitor bug in desktop software
//! (restore a saved layout after unplugging a monitor, and the window opens
//! where no pixel is) was not merely possible here, it was **unaskable**.
//!
//! This module is the answer, and it is deliberately split in two:
//!
//! * **the value** — [`DisplayTopology`], a plain ordered set of [`Display`]s.
//!   Everything interesting is a *pure function* of it: which display holds a
//!   point, which displays a rectangle straddles, how much of that rectangle is
//!   on no display at all, whether the arrangement has holes, and where a
//!   window would have to move to become wholly visible.
//! * **the supply** — one platform read, in `pinion-shell`, that turns the
//!   window system's monitor list into that value.
//!
//! The split is the point. A monitor farm is not a thing CI has, so a design in
//! which the arithmetic lives inside the platform layer is a design whose
//! multi-monitor behaviour is asserted by nobody. Here the arrangement is an
//! argument: an L-shaped desk, a mirrored pair, a 4K panel beside a high-DPI
//! laptop are all values a test constructs, and the only thing needing a second
//! monitor to exercise is the handful of lines that read winit.
//!
//! # Coordinate space
//!
//! [`DisplayRect`] is in **physical device pixels**, with a **signed** origin —
//! a display to the left of or above the primary one genuinely sits at negative
//! coordinates, which is why [`crate::scene::Rect`] (unsigned, the layout and
//! paint box) is not reused here.
//!
//! Physical rather than logical, because physical is the one space in which the
//! window system itself lays displays out and therefore the only space in which
//! the arrangement is unambiguous. Under mixed DPI a "logical desktop" is a
//! per-display conversion, and conversions do not compose: two displays with
//! different scale factors have logical rectangles that overlap, or leave a gap
//! where the hardware does neither. Each [`Display`] publishes its own
//! [`scale_factor`](Display::scale_factor), so logical is derivable *per
//! display* — the direction that is well defined.
//!
//! # Where this is past the toolkit 6.11
//!
//! The toolkit's screen is the floor: it enumerates, and it reports geometry,
//! scale and refresh rate. Six things here are not parity, each checked
//! against `qscreen.h` / `qguiapplication.h` / `qwidget.h`:
//!
//! 1. **A display has a stable address.** screen has no id accessor at all —
//!    `name()` is platform text with no uniqueness guarantee (two identical
//!    panels commonly report one string), so the only handle the toolkit gives you is
//!    the `screen *` itself, which is `Q_DISABLE_COPY`, privately constructed,
//!    and destroyed on `screenRemoved`. A toolkit layout preset therefore cannot
//!    *name* a display. [`DisplayId`] is unique inside its topology by
//!    construction.
//! 2. **The holes in the desktop are stated.** `virtualGeometry()` is
//!    the *bounding rectangle* of the screens; for any arrangement that is not
//!    itself a rectangle it contains points that are on no screen, and the toolkit has
//!    no accessor that says so. [`DisplayTopology::is_gap_free`] does, off the
//!    same union computation that answers everything else here.
//! 3. **Placement is answerable before it happens.** Every the toolkit screen query
//!    takes a *point* (`screenAt`,
//!    `virtualSiblingAt`) — there is no rectangle-level question in
//!    the API, so each the toolkit application that restores a window geometry
//!    hand-rolls its own clamp, which is precisely why so many of them come
//!    back off-screen. [`DisplayTopology::resolve`] answers with the home
//!    display, every display the rectangle touches, how many of its pixels are
//!    visible, and where it would have to move.
//! 4. **A saved layout is data, and it survives the desk changing.**
//!    `saveGeometry()` answers a byte array — an opaque, versioned,
//!    absolute blob that cannot be read, diffed or edited. An [`Anchor`] is a
//!    display id plus an offset *within* that display, so one preset means the
//!    same thing after the monitors are rearranged, and when the named display
//!    is gone the substitution is **named** rather than silently performed.
//! 5. **Absence is stated, not zeroed.** `refreshRate()` returns
//!    `qreal`, so a platform that does not know reports `0` —
//!    indistinguishable from an answer. [`Display::refresh_mhz`] is an
//!    `Option`, which is also what the substrate underneath reports.
//! 6. **The desk reaches the wire.** `scene/displays` publishes all of it, so
//!    an agent driving the application headlessly knows what it is driving on.
//!    No the toolkit application can be asked about its screens from outside its
//!    process.
//!
//! # What this module deliberately does not model
//!
//! A display's **usable region** — the toolkit's `availableGeometry()`, the bounds minus panels
//! and docks. It is absent because nothing underneath reports it: winit's `MonitorHandle`
//! has no work-area accessor, and EWMH's `_NET_WORKAREA` is one rectangle for the whole
//! *desktop* rather than one per monitor (the toolkit's X11 plugin intersects
//! it with each screen's geometry and calls the result available). Modelling a
//! field the supply cannot fill would put a permanent `None` on every display on
//! every platform. It is the next slice of this axis, and it is a *platform
//! probe*, not geometry.

use std::collections::BTreeMap;

/// An axis-aligned rectangle in the virtual-desktop space: **physical device
/// pixels**, **signed** origin.
///
/// The extent is unsigned because a negative extent is not a rectangle; the
/// origin is signed because a display above or to the left of the primary one
/// really is at a negative coordinate. Edge arithmetic is `i64`, so an `i32`
/// origin plus a `u32` extent cannot overflow.
///
/// A zero-extent rectangle is the **empty set** — it contains no point,
/// intersects nothing, and contributes no area. That is the same convention
/// [`crate::scene::Rect::union`] uses, and it is what makes a monitor the
/// platform reports as `0x0` mid-hotplug harmless rather than a special case
/// every caller has to remember.
///
/// Areas are `u64` and that is provably enough: the largest representable
/// rectangle is `u32::MAX` x `u32::MAX` = `2^64 - 2^33 + 1` pixels, and every
/// area this module computes is bounded by some rectangle's own area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct DisplayRect {
    /// Left edge, physical pixels, may be negative.
    pub x: i32,
    /// Top edge, physical pixels, may be negative.
    pub y: i32,
    /// Width in physical pixels. `0` makes the rectangle empty.
    pub w: u32,
    /// Height in physical pixels. `0` makes the rectangle empty.
    pub h: u32,
}

impl DisplayRect {
    /// A rectangle at `(x, y)` extending `w` x `h` physical pixels.
    #[must_use]
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// Left edge — the arithmetic type every edge comparison uses.
    #[must_use]
    pub fn left(self) -> i64 {
        i64::from(self.x)
    }

    /// Top edge.
    #[must_use]
    pub fn top(self) -> i64 {
        i64::from(self.y)
    }

    /// One past the right edge. `i64`, so an `i32::MAX` origin cannot wrap.
    #[must_use]
    pub fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.w)
    }

    /// One past the bottom edge.
    #[must_use]
    pub fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.h)
    }

    /// Does this rectangle enclose no pixels?
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Pixel count.
    #[must_use]
    pub fn area(self) -> u64 {
        u64::from(self.w) * u64::from(self.h)
    }

    /// Is `(x, y)` inside? Half-open: the left and top edges are inside, the
    /// right and bottom edges are not — so two displays that abut share no
    /// pixel, and a point on the seam belongs to exactly one of them.
    #[must_use]
    pub fn contains(self, x: i32, y: i32) -> bool {
        let (px, py) = (i64::from(x), i64::from(y));
        !self.is_empty()
            && px >= self.left()
            && px < self.right()
            && py >= self.top()
            && py < self.bottom()
    }

    /// The overlapping region, or `None` when they do not meet.
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        if self.is_empty() || other.is_empty() {
            return None;
        }
        let x0 = self.left().max(other.left());
        let y0 = self.top().max(other.top());
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some(Self {
            x: i32::try_from(x0).ok()?,
            y: i32::try_from(y0).ok()?,
            w: u32::try_from(x1 - x0).ok()?,
            h: u32::try_from(y1 - y0).ok()?,
        })
    }

    /// The smallest rectangle containing both. An empty operand contributes
    /// nothing, so the union of an empty rectangle with a real one is the real
    /// one verbatim.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = self.left().min(other.left());
        let y0 = self.top().min(other.top());
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Self {
            x: i32::try_from(x0).unwrap_or(i32::MIN),
            y: i32::try_from(y0).unwrap_or(i32::MIN),
            w: u32::try_from(x1 - x0).unwrap_or(u32::MAX),
            h: u32::try_from(y1 - y0).unwrap_or(u32::MAX),
        }
    }
}

/// The area covered by **at least one** of `rects`, counting overlap once.
///
/// Coordinate compression: every distinct x edge and y edge of the input cuts
/// the plane into cells, and because those edges are exactly the rectangle
/// boundaries, each cell is wholly inside or wholly outside each rectangle. So
/// the union area is the sum of the covered cells — exact integer arithmetic,
/// no sampling, no inclusion-exclusion blow-up.
///
/// This one function answers three separate questions in this module: how much
/// of the desktop is real ([`DisplayTopology::covered_px`]), whether the
/// arrangement has holes ([`DisplayTopology::is_gap_free`]), and how much of a
/// window is on screen ([`Placement::visible_px`]). Mirrored displays — two
/// monitors reporting *identical* bounds — are the case that makes a naive
/// sum-of-areas wrong, and they are ordinary here.
#[must_use]
pub fn union_px(rects: &[DisplayRect]) -> u64 {
    let live: Vec<DisplayRect> = rects.iter().copied().filter(|r| !r.is_empty()).collect();
    if live.is_empty() {
        return 0;
    }
    let mut xs: Vec<i64> = live.iter().flat_map(|r| [r.left(), r.right()]).collect();
    let mut ys: Vec<i64> = live.iter().flat_map(|r| [r.top(), r.bottom()]).collect();
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();
    let mut total: u64 = 0;
    for xw in xs.windows(2) {
        let (x0, x1) = (xw[0], xw[1]);
        for yw in ys.windows(2) {
            let (y0, y1) = (yw[0], yw[1]);
            let covered = live
                .iter()
                .any(|r| r.left() <= x0 && x1 <= r.right() && r.top() <= y0 && y1 <= r.bottom());
            if covered {
                let w = u64::try_from(x1 - x0).unwrap_or(0);
                let h = u64::try_from(y1 - y0).unwrap_or(0);
                // Every covered cell lies inside the inputs' bounding box, so
                // the running total is bounded by that box's own area, which a
                // `u64` holds by construction.
                total = total.saturating_add(w.saturating_mul(h));
            }
        }
    }
    total
}

/// A display's address inside its [`DisplayTopology`] — unique by construction.
///
/// This is what a saved layout names, so it has to be *stable* (the same
/// physical monitor answers to the same id next session) and *unique* (two
/// monitors never answer to one id). The platform gives neither: a monitor's
/// reported name is optional, may be empty, and two identical panels routinely
/// report the same string. So the id is **derived** by
/// [`DisplayTopology::new`] — the reported name slugified, disambiguated by a
/// `#n` ordinal when it repeats, synthesized from the enumeration position when
/// there is no usable name at all. The platform's own string survives untouched
/// as [`Display::label`], because that is the one a human reads.
///
/// Stability is therefore exactly as good as the platform's enumeration is and
/// no better — worth stating rather than implying, because it is the property a
/// layout preset rests on.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct DisplayId(String);

impl DisplayId {
    /// Wrap a string that is already an id — a preset being read back, a test
    /// fixture, a wire argument. Nothing is checked here: uniqueness is a
    /// property of a *topology*, not of a string, and [`DisplayTopology::new`]
    /// is where it is established.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as it appears on the wire and in a preset.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DisplayId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a platform reports about one monitor — the *input* to
/// [`DisplayTopology::new`], before ids are made unique.
///
/// Deliberately the exact shape winit's `MonitorHandle` can answer, so the
/// supply is a field-for-field move with nothing invented: an optional name, a
/// physical position and size, a scale factor, an optional refresh rate.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    /// The platform's own name for the monitor, if it has one. May be empty,
    /// may repeat across monitors — see [`DisplayId`].
    pub label: Option<String>,
    /// Physical bounds in the virtual-desktop space.
    pub bounds: DisplayRect,
    /// Physical pixels per logical pixel. Must be finite and positive;
    /// [`DisplayTopology::new`] substitutes `1.0` for anything else, because a
    /// zero or `NaN` scale poisons every logical conversion downstream instead
    /// of failing at the one place it is wrong.
    pub scale_factor: f64,
    /// Refresh rate in millihertz, or `None` when the platform did not report
    /// one — never a `0` standing in for "unknown".
    pub refresh_mhz: Option<u32>,
    /// Did the platform call this the primary monitor?
    pub primary: bool,
}

impl DisplayInfo {
    /// A monitor with this name and bounds, at scale `1.0`, no reported refresh
    /// rate, not primary. The rest is set with the builders, so a test
    /// arrangement reads as the thing it describes.
    #[must_use]
    pub fn new(label: impl Into<String>, bounds: DisplayRect) -> Self {
        Self {
            label: Some(label.into()),
            bounds,
            scale_factor: 1.0,
            refresh_mhz: None,
            primary: false,
        }
    }

    /// An unnamed monitor — what a platform that reports no name gives.
    #[must_use]
    pub const fn unnamed(bounds: DisplayRect) -> Self {
        Self {
            label: None,
            bounds,
            scale_factor: 1.0,
            refresh_mhz: None,
            primary: false,
        }
    }

    /// Mark this the primary monitor.
    #[must_use]
    pub const fn as_primary(mut self) -> Self {
        self.primary = true;
        self
    }

    /// Set the physical-per-logical pixel ratio.
    #[must_use]
    pub const fn with_scale(mut self, scale: f64) -> Self {
        self.scale_factor = scale;
        self
    }

    /// Set the reported refresh rate in millihertz.
    #[must_use]
    pub const fn with_refresh_mhz(mut self, mhz: u32) -> Self {
        self.refresh_mhz = Some(mhz);
        self
    }
}

/// One monitor, as the framework and the wire see it.
///
/// Constructed only by [`DisplayTopology::new`], because two of its fields are
/// guarantees rather than data: the [`id`](Self::id) is unique within its
/// topology, and at most one display in a topology is
/// [`primary`](Self::primary).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Display {
    id: DisplayId,
    label: String,
    bounds: DisplayRect,
    scale_factor: f64,
    refresh_mhz: Option<u32>,
    primary: bool,
}

impl Display {
    /// The address a layout preset names. Unique within the topology.
    #[must_use]
    pub const fn id(&self) -> &DisplayId {
        &self.id
    }

    /// The platform's own name, verbatim — possibly empty, possibly shared with
    /// another display. For humans; [`id`](Self::id) is for machines.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Physical bounds in the virtual-desktop space.
    #[must_use]
    pub const fn bounds(&self) -> DisplayRect {
        self.bounds
    }

    /// Physical pixels per logical pixel on this display.
    #[must_use]
    pub const fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    /// Reported refresh rate in millihertz, or `None` when the platform does
    /// not know.
    #[must_use]
    pub const fn refresh_mhz(&self) -> Option<u32> {
        self.refresh_mhz
    }

    /// Is this the primary display? At most one display in a topology is.
    #[must_use]
    pub const fn primary(&self) -> bool {
        self.primary
    }

    /// This display's size in **logical** pixels — its own physical size over
    /// its own scale factor.
    ///
    /// Only the *size* converts cleanly. A logical *origin* is not a
    /// desktop-wide fact, because each display divides by a different number;
    /// that is the whole reason this module's space is physical.
    #[must_use]
    pub fn logical_size(&self) -> (f64, f64) {
        (
            f64::from(self.bounds.w) / self.scale_factor,
            f64::from(self.bounds.h) / self.scale_factor,
        )
    }

    /// Turn a logical offset *within this display* into an absolute physical
    /// point in the virtual-desktop space.
    ///
    /// This is what makes an [`Anchor`] display-relative: "40 logical pixels in
    /// from the corner of that monitor" is a fact about the monitor, and it
    /// means the same thing after the desk is rearranged and on a monitor with
    /// a different scale.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the product is clamped into i32's range immediately before the cast, so the truncating conversion is unreachable"
    )]
    #[must_use]
    pub fn physical_at(&self, logical_offset: (i32, i32)) -> (i32, i32) {
        let scale = |v: i32| -> i64 {
            let scaled = f64::from(v) * self.scale_factor;
            if scaled.is_finite() {
                scaled.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i64
            } else {
                0
            }
        };
        let x = self.bounds.left() + scale(logical_offset.0);
        let y = self.bounds.top() + scale(logical_offset.1);
        (
            i32::try_from(x).unwrap_or(i32::MAX),
            i32::try_from(y).unwrap_or(i32::MAX),
        )
    }
}

/// The whole desk: every display, in the platform's enumeration order.
///
/// Two invariants, established by [`new`](Self::new) and relied on everywhere
/// else:
///
/// 1. **Ids are unique.** Two displays never answer to one [`DisplayId`].
/// 2. **At most one display is primary.** The platform may report none (a
///    headless session) or, through a bug, several; the constructor keeps the
///    first and demotes the rest, so [`primary`](Self::primary) is a question
///    with one answer rather than a list a caller must reduce.
///
/// An **empty** topology is an ordinary value, not an error: a headless or
/// surfaceless session genuinely has no displays, and every derivation here is
/// total on it. The toolkit models the same state as `primaryScreen()`
/// answering `nullptr`, which is the shape that produces the crash rather than
/// the answer.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct DisplayTopology {
    displays: Vec<Display>,
}

impl DisplayTopology {
    /// Canonicalise a platform report into a topology.
    ///
    /// Order is preserved — it is the platform's enumeration order, and the
    /// ordinal an id falls back on is taken from it, so a topology built twice
    /// from one report is identical.
    #[must_use]
    pub fn new(infos: Vec<DisplayInfo>) -> Self {
        // Pass 1: how many displays want each slug? A slug used once needs no
        // ordinal; a slug used twice makes BOTH ambiguous, so neither may keep
        // the bare form. Deciding per display as we went would give the first
        // occurrence the bare id and the second `#2`, which reads as a
        // precedence that is not there — and would silently change every id
        // when the enumeration order shifted.
        let mut wanted: BTreeMap<String, usize> = BTreeMap::new();
        for info in &infos {
            *wanted.entry(slug(info.label.as_deref())).or_insert(0) += 1;
        }
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let mut primary_taken = false;
        let displays = infos
            .into_iter()
            .enumerate()
            .map(|(index, info)| {
                let base = slug(info.label.as_deref());
                let id = if base.is_empty() {
                    // No usable name: the enumeration position is the only
                    // stable thing left, and it cannot collide with a slug
                    // because a slug never starts with `display-` unless the
                    // platform named a monitor that, in which case the ordinal
                    // branch above already owns it.
                    DisplayId(format!("display-{index}"))
                } else if wanted.get(&base).copied().unwrap_or(0) > 1 {
                    let ordinal = seen.entry(base.clone()).or_insert(0);
                    *ordinal += 1;
                    DisplayId(format!("{base}#{ordinal}"))
                } else {
                    DisplayId(base)
                };
                let primary = info.primary && !primary_taken;
                primary_taken |= primary;
                Display {
                    id,
                    label: info.label.unwrap_or_default(),
                    bounds: info.bounds,
                    scale_factor: if info.scale_factor.is_finite() && info.scale_factor > 0.0 {
                        info.scale_factor
                    } else {
                        1.0
                    },
                    refresh_mhz: info.refresh_mhz,
                    primary,
                }
            })
            .collect();
        Self { displays }
    }

    /// A desk with no monitors — the headless case, and the identity every
    /// derivation here is total on.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            displays: Vec::new(),
        }
    }

    /// How many displays.
    #[must_use]
    pub fn len(&self) -> usize {
        self.displays.len()
    }

    /// Is this a headless desk?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.displays.is_empty()
    }

    /// Every display, in the platform's enumeration order.
    pub fn iter(&self) -> impl Iterator<Item = &Display> {
        self.displays.iter()
    }

    /// The display with this id, or `None`.
    #[must_use]
    pub fn get(&self, id: &DisplayId) -> Option<&Display> {
        self.displays.iter().find(|d| d.id == *id)
    }

    /// The primary display, or `None` on a headless desk. The toolkit
    /// `primaryScreen`.
    #[must_use]
    pub fn primary(&self) -> Option<&Display> {
        self.displays.iter().find(|d| d.primary)
    }

    /// The display the topology falls back on when a named one is gone: the
    /// primary if there is one, else the first enumerated.
    ///
    /// Separate from [`primary`](Self::primary) because "which monitor is the
    /// desktop's main one" and "where do I put a window I have nowhere else to
    /// put" are different questions, and a platform reporting no primary at all
    /// still has to answer the second.
    #[must_use]
    pub fn fallback(&self) -> Option<&Display> {
        self.primary().or_else(|| self.displays.first())
    }

    /// The smallest rectangle containing every display. The toolkit
    /// `virtualGeometry`.
    ///
    /// A point inside it is **not** necessarily on a display — see
    /// [`is_gap_free`](Self::is_gap_free), the question the toolkit cannot ask.
    #[must_use]
    pub fn bounding_box(&self) -> Option<DisplayRect> {
        self.displays
            .iter()
            .map(Display::bounds)
            .filter(|b| !b.is_empty())
            .reduce(DisplayRect::union)
    }

    /// Pixels that are on at least one display, counting an overlap once.
    #[must_use]
    pub fn covered_px(&self) -> u64 {
        union_px(&self.rects())
    }

    /// Does the arrangement fill its own bounding box?
    ///
    /// `true` for a single display, for a row of abutting displays, and for a
    /// mirrored pair; `false` for an L, for a diagonal pair, and for anything
    /// with a deliberate gap. Vacuously `true` for a headless desk — an empty
    /// union equals an empty bounding box.
    ///
    /// This is what makes "is `(x, y)` on a display?" and "is `(x, y)` inside the
    /// virtual desktop?" *different questions*, a distinction the toolkit's
    /// API has no way to express — which is why the toolkit code routinely
    /// uses `virtualGeometry()` containment as a visibility test and is wrong on every
    /// L-shaped desk.
    #[must_use]
    pub fn is_gap_free(&self) -> bool {
        self.bounding_box()
            .is_none_or(|bb| bb.area() == self.covered_px())
    }

    /// The display containing `(x, y)`, or `None`. The toolkit
    /// `screenAt`.
    ///
    /// Overlapping displays (a mirrored pair) resolve to the first in
    /// enumeration order, deterministically.
    #[must_use]
    pub fn display_at(&self, x: i32, y: i32) -> Option<&Display> {
        self.displays.iter().find(|d| d.bounds.contains(x, y))
    }

    /// Where would a window with these physical bounds actually be?
    ///
    /// The question the toolkit's API cannot be asked — see the module doc.
    /// Total: a rectangle on no display, an empty one, or a headless desk all
    /// produce a [`Placement`] rather than an error.
    #[must_use]
    pub fn resolve(&self, rect: DisplayRect) -> Placement {
        let mut covering = Vec::new();
        let mut best: Option<(u64, &Display)> = None;
        let mut shares = Vec::new();
        for display in &self.displays {
            let Some(overlap) = display.bounds.intersection(rect) else {
                continue;
            };
            let px = overlap.area();
            shares.push(overlap);
            covering.push(Coverage {
                id: display.id.clone(),
                px,
            });
            // Strictly greater, so a tie keeps the earlier display: enumeration
            // order is the tie-break everywhere in this module.
            if best.is_none_or(|(bpx, _)| px > bpx) {
                best = Some((px, display));
            }
        }
        Placement {
            home: best.map(|(_, d)| d.id.clone()),
            covering,
            visible_px: union_px(&shares),
            total_px: rect.area(),
            suggestion: self.nearest_fitting(rect),
        }
    }

    /// The origin nearest `rect`'s that would put it wholly inside a **single**
    /// display, or `None` when no display is large enough to hold it.
    ///
    /// A single display rather than the union: a window spanning an L's inner
    /// corner can be entirely covered by two displays and still be a poor place
    /// to put it — and, more to the point, a suggestion that depends on two
    /// monitors staying arranged is not one a preset can be built on.
    ///
    /// Ties break by enumeration order, so the answer is a function of the
    /// arrangement and of nothing else.
    fn nearest_fitting(&self, rect: DisplayRect) -> Option<(i32, i32)> {
        let mut best: Option<(u64, (i32, i32))> = None;
        for display in &self.displays {
            let b = display.bounds;
            if b.is_empty() || rect.w > b.w || rect.h > b.h {
                continue;
            }
            let x = rect.left().clamp(b.left(), b.right() - i64::from(rect.w));
            let y = rect.top().clamp(b.top(), b.bottom() - i64::from(rect.h));
            let dx = (x - rect.left()).unsigned_abs();
            let dy = (y - rect.top()).unsigned_abs();
            let cost = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
            let candidate = (
                i32::try_from(x).unwrap_or(i32::MAX),
                i32::try_from(y).unwrap_or(i32::MAX),
            );
            if best.is_none_or(|(bc, _)| cost < bc) {
                best = Some((cost, candidate));
            }
        }
        best.map(|(_, at)| at)
    }

    /// Resolve a display-relative [`Anchor`] into an absolute physical origin.
    ///
    /// This is the module's whole point for a layout preset: the preset says
    /// *which monitor* and *how far in*, and this says where that is today.
    /// When the named display is gone the answer is [`Anchored::Substituted`] — the window still
    /// opens somewhere a person can reach it, and the fact that it was moved
    /// is **in the answer**, which is the half the toolkit's `restoreGeometry` cannot
    /// report.
    #[must_use]
    pub fn anchor(&self, anchor: &Anchor) -> Anchored {
        if let Some(display) = self.get(&anchor.display) {
            return Anchored::OnDeclared {
                display: display.id.clone(),
                at: display.physical_at(anchor.offset),
            };
        }
        match self.fallback() {
            Some(display) => Anchored::Substituted {
                declared: anchor.display.clone(),
                display: display.id.clone(),
                at: display.physical_at(anchor.offset),
            },
            None => Anchored::NoDisplay {
                declared: anchor.display.clone(),
            },
        }
    }

    fn rects(&self) -> Vec<DisplayRect> {
        self.displays.iter().map(Display::bounds).collect()
    }
}

/// One display's share of a resolved rectangle.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Coverage {
    /// Which display.
    pub id: DisplayId,
    /// How many of the rectangle's pixels land on it. Counted **per display**,
    /// so summing these over-counts wherever two displays overlap — which is
    /// exactly the relation [`Placement::visible_px`] is checked against, and
    /// the reason both numbers are published rather than one.
    pub px: u64,
}

/// Where a rectangle actually is, against a [`DisplayTopology`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Placement {
    /// The display holding the largest share, or `None` when the rectangle is
    /// on no display at all — the state a restored preset lands in after a
    /// monitor is unplugged.
    pub home: Option<DisplayId>,
    /// Every display the rectangle touches, in enumeration order, with its
    /// share. A window straddling a seam has more than one.
    pub covering: Vec<Coverage>,
    /// Pixels of the rectangle that are on some display, counting overlap once.
    pub visible_px: u64,
    /// The rectangle's own pixel count.
    pub total_px: u64,
    /// The nearest origin that would make the rectangle wholly visible on one
    /// display, or `None` when none is big enough. Present even when the
    /// rectangle is already wholly visible, where it equals the rectangle's own
    /// origin — a suggestion that appeared only on failure would be one a
    /// caller could not check its own arithmetic against.
    pub suggestion: Option<(i32, i32)>,
}

impl Placement {
    /// Is every pixel of the rectangle on some display?
    ///
    /// An empty rectangle is **not** wholly visible: it has no pixels anywhere,
    /// and answering `true` would let a zero-size window through a visibility
    /// gate it never satisfied.
    #[must_use]
    pub const fn is_fully_visible(&self) -> bool {
        self.total_px > 0 && self.visible_px == self.total_px
    }

    /// Pixels of the rectangle that are on no display.
    #[must_use]
    pub const fn offscreen_px(&self) -> u64 {
        self.total_px.saturating_sub(self.visible_px)
    }

    /// The visible share; `0.0` for an empty rectangle.
    #[must_use]
    pub fn visible_fraction(&self) -> f64 {
        if self.total_px == 0 {
            return 0.0;
        }
        ratio(self.visible_px, self.total_px)
    }
}

/// A place stated the way a layout preset has to state it: **which display**,
/// and **how far into it**.
///
/// The offset is in that display's *logical* pixels, so a preset written on a
/// 1x monitor puts the window the same visible distance in when it is restored
/// onto a 2x one. An absolute virtual-desktop coordinate — which is all the
/// toolkit's `saveGeometry` blob holds — means something different the moment the
/// monitors are rearranged, and means nothing at all once one is unplugged.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    /// The display this place belongs to.
    pub display: DisplayId,
    /// Logical pixels in from that display's top-left corner. Signed, so a
    /// preset can deliberately hang a window off an edge.
    pub offset: (i32, i32),
}

impl Anchor {
    /// An offset within a named display.
    #[must_use]
    pub const fn new(display: DisplayId, offset: (i32, i32)) -> Self {
        Self { display, offset }
    }
}

/// What became of an [`Anchor`] when it met today's desk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Anchored {
    /// The named display is here; this is where that offset lands.
    OnDeclared {
        /// The display used — equal to the one declared.
        display: DisplayId,
        /// Absolute physical origin.
        at: (i32, i32),
    },
    /// The named display is gone. The offset was applied to the fallback
    /// display instead, and **both** names are reported, so a caller can say so
    /// rather than silently drift.
    Substituted {
        /// The display the preset named, which is not here.
        declared: DisplayId,
        /// The display used instead.
        display: DisplayId,
        /// Absolute physical origin on that display.
        at: (i32, i32),
    },
    /// There are no displays at all, so there is no such thing as a position.
    NoDisplay {
        /// The display the preset named.
        declared: DisplayId,
    },
}

impl Anchored {
    /// The absolute physical origin, or `None` on a headless desk.
    #[must_use]
    pub const fn at(&self) -> Option<(i32, i32)> {
        match self {
            Self::OnDeclared { at, .. } | Self::Substituted { at, .. } => Some(*at),
            Self::NoDisplay { .. } => None,
        }
    }

    /// Was the declared display honoured?
    #[must_use]
    pub const fn is_declared(&self) -> bool {
        matches!(self, Self::OnDeclared { .. })
    }

    /// The display actually used, or `None` on a headless desk.
    #[must_use]
    pub const fn display(&self) -> Option<&DisplayId> {
        match self {
            Self::OnDeclared { display, .. } | Self::Substituted { display, .. } => Some(display),
            Self::NoDisplay { .. } => None,
        }
    }

    /// The one word this outcome is called on the wire. A closed vocabulary
    /// this crate owns, so a client may match on it (R1565's `data_is_prose`
    /// distinction).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::OnDeclared { .. } => "on_declared",
            Self::Substituted { .. } => "substituted",
            Self::NoDisplay { .. } => "no_display",
        }
    }
}

/// `u64 / u64` as a ratio, without repeating the lint exemption at each call.
#[allow(
    clippy::cast_precision_loss,
    reason = "both operands are pixel counts of one rectangle; the result is a ratio, and a desktop-sized pixel count is far below 2^53"
)]
fn ratio(num: u64, den: u64) -> f64 {
    num as f64 / den as f64
}

/// Slugify a platform monitor name into an id fragment: lowercase, with every
/// run of non-alphanumerics collapsed to a single `-` and the ends trimmed.
///
/// Deterministic and total. `None`, an empty name, and a name made entirely of
/// punctuation all slugify to the empty string, which [`DisplayTopology::new`]
/// then replaces with an enumeration-derived id — so "the platform gave us
/// nothing usable" has one representation rather than three.
fn slug(label: Option<&str>) -> String {
    let mut out = String::new();
    for ch in label.unwrap_or_default().chars() {
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        Anchor, Anchored, DisplayId, DisplayInfo, DisplayRect, DisplayTopology, slug, union_px,
    };

    /// `(0, 0) 1920x1080`, the shape most of these arrangements are built from.
    fn r(x: i32, y: i32, w: u32, h: u32) -> DisplayRect {
        DisplayRect::new(x, y, w, h)
    }

    fn id(s: &str) -> DisplayId {
        DisplayId::new(s)
    }

    /// Two 1920x1080 panels side by side, the left one primary. The commonest
    /// desk there is, and gap-free.
    fn side_by_side() -> DisplayTopology {
        DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", r(0, 0, 1920, 1080)).as_primary(),
            DisplayInfo::new("DP-2", r(1920, 0, 1920, 1080)),
        ])
    }

    /// A wide panel with a shorter one to its right, tops aligned: the bounding
    /// box has a hole under the shorter one.
    fn l_shaped() -> DisplayTopology {
        DisplayTopology::new(vec![
            DisplayInfo::new("main", r(0, 0, 1000, 1000)).as_primary(),
            DisplayInfo::new("side", r(1000, 0, 500, 400)),
        ])
    }

    fn scale_eq(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 1e-9
    }

    // --- DisplayRect ------------------------------------------------------

    #[test]
    fn r1576_an_empty_rect_is_the_empty_set() {
        let empty = r(10, 10, 0, 50);
        assert!(empty.is_empty());
        assert_eq!(empty.area(), 0);
        assert!(!empty.contains(10, 10), "an empty rect contains no point");
        assert_eq!(empty.intersection(r(0, 0, 100, 100)), None);
        assert_eq!(r(0, 0, 100, 100).intersection(empty), None);
        // Union treats it as contributing nothing, so it cannot drag a
        // bounding box out to its degenerate origin.
        assert_eq!(empty.union(r(0, 0, 100, 100)), r(0, 0, 100, 100));
        assert_eq!(r(0, 0, 100, 100).union(empty), r(0, 0, 100, 100));
    }

    #[test]
    fn r1576_containment_is_half_open_so_abutting_displays_share_no_pixel() {
        let left = r(0, 0, 100, 100);
        let right = r(100, 0, 100, 100);
        assert!(left.contains(99, 0));
        assert!(!left.contains(100, 0), "the right edge is outside");
        assert!(right.contains(100, 0));
        assert_eq!(
            left.intersection(right),
            None,
            "abutting is not overlapping"
        );
        // And the seam therefore belongs to exactly one of them.
        let desk = side_by_side();
        assert_eq!(
            desk.display_at(1920, 10).map(|d| d.id().as_str()),
            Some("dp-2")
        );
        assert_eq!(
            desk.display_at(1919, 10).map(|d| d.id().as_str()),
            Some("dp-1")
        );
    }

    #[test]
    fn r1576_edges_are_i64_so_an_extreme_origin_cannot_wrap() {
        let far = r(i32::MAX - 10, i32::MIN, 100, 100);
        assert_eq!(far.right(), i64::from(i32::MAX) - 10 + 100);
        assert_eq!(far.bottom(), i64::from(i32::MIN) + 100);
        assert!(
            far.right() > far.left(),
            "the right edge stayed to the right"
        );
    }

    #[test]
    fn r1576_intersection_is_the_overlap() {
        assert_eq!(
            r(0, 0, 100, 100).intersection(r(50, 50, 100, 100)),
            Some(r(50, 50, 50, 50))
        );
        assert_eq!(r(0, 0, 10, 10).intersection(r(50, 50, 10, 10)), None);
        // Containment: the inner rect is its own intersection.
        assert_eq!(
            r(0, 0, 100, 100).intersection(r(10, 10, 10, 10)),
            Some(r(10, 10, 10, 10))
        );
    }

    // --- union_px ---------------------------------------------------------

    #[test]
    fn r1576_union_counts_an_overlap_once() {
        assert_eq!(union_px(&[]), 0);
        assert_eq!(union_px(&[r(0, 0, 10, 10)]), 100);
        // Disjoint: the sum.
        assert_eq!(union_px(&[r(0, 0, 10, 10), r(100, 100, 10, 10)]), 200);
        // Abutting: still the sum, because containment is half-open.
        assert_eq!(union_px(&[r(0, 0, 10, 10), r(10, 0, 10, 10)]), 200);
        // Overlapping by 5x10: 100 + 100 - 50.
        assert_eq!(union_px(&[r(0, 0, 10, 10), r(5, 0, 10, 10)]), 150);
        // Mirrored — identical bounds. A sum of areas would say 200.
        assert_eq!(union_px(&[r(0, 0, 10, 10), r(0, 0, 10, 10)]), 100);
        // Contained.
        assert_eq!(union_px(&[r(0, 0, 10, 10), r(2, 2, 3, 3)]), 100);
        // Empties contribute nothing.
        assert_eq!(union_px(&[r(0, 0, 10, 10), r(0, 0, 0, 99)]), 100);
    }

    #[test]
    fn r1576_union_is_exact_on_an_l_shape() {
        // Three-rect L where two of them overlap: the answer is not any
        // pairwise formula, which is why the cell decomposition is used.
        let rects = [r(0, 0, 10, 10), r(0, 5, 10, 10), r(10, 0, 5, 5)];
        // Column 0..10 covered y 0..15 = 150; column 10..15 covered y 0..5 = 25.
        assert_eq!(union_px(&rects), 175);
    }

    // --- ids --------------------------------------------------------------

    #[test]
    fn r1576_a_repeated_name_makes_both_ids_ordinal() {
        let desk = DisplayTopology::new(vec![
            DisplayInfo::new("Generic Monitor", r(0, 0, 100, 100)),
            DisplayInfo::new("Generic Monitor", r(100, 0, 100, 100)),
        ]);
        let ids: Vec<&str> = desk.iter().map(|d| d.id().as_str()).collect();
        assert_eq!(ids, vec!["generic-monitor#1", "generic-monitor#2"]);
        // NEITHER keeps the bare slug: a bare id beside an ordinal one reads as
        // a precedence that is not there.
        assert!(desk.get(&id("generic-monitor")).is_none());
        // The platform's own string is untouched on both.
        assert!(desk.iter().all(|d| d.label() == "Generic Monitor"));
    }

    #[test]
    fn r1576_a_unique_name_keeps_its_slug_and_a_missing_one_gets_its_place() {
        let desk = DisplayTopology::new(vec![
            DisplayInfo::new("DP-4", r(0, 0, 100, 100)),
            DisplayInfo::unnamed(r(100, 0, 100, 100)),
            DisplayInfo::new("!!!", r(200, 0, 100, 100)),
        ]);
        let ids: Vec<&str> = desk.iter().map(|d| d.id().as_str()).collect();
        // The unnamed one and the punctuation-only one both slugify to the
        // empty string, and both fall back on their enumeration position — so
        // "nothing usable" has ONE representation, and they still differ.
        assert_eq!(ids, vec!["dp-4", "display-1", "display-2"]);
        // The platform's own string is kept verbatim on both, however useless
        // it was as an address.
        assert_eq!(
            desk.get(&id("display-1")).map(super::Display::label),
            Some("")
        );
        assert_eq!(
            desk.get(&id("display-2")).map(super::Display::label),
            Some("!!!")
        );
    }

    #[test]
    fn r1576_ids_are_unique_across_every_arrangement_shape() {
        let desks = [
            side_by_side(),
            l_shaped(),
            DisplayTopology::new(vec![
                DisplayInfo::unnamed(r(0, 0, 10, 10)),
                DisplayInfo::unnamed(r(10, 0, 10, 10)),
                DisplayInfo::new("x", r(20, 0, 10, 10)),
                DisplayInfo::new("x", r(30, 0, 10, 10)),
                DisplayInfo::new("X", r(40, 0, 10, 10)),
            ]),
            DisplayTopology::empty(),
        ];
        for desk in &desks {
            let mut ids: Vec<&str> = desk.iter().map(|d| d.id().as_str()).collect();
            let total = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), total, "ids repeat in {desk:?}");
        }
    }

    #[test]
    fn r1576_slug_is_total() {
        assert_eq!(slug(None), "");
        assert_eq!(slug(Some("")), "");
        assert_eq!(slug(Some("---")), "");
        assert_eq!(slug(Some("DP-4")), "dp-4");
        assert_eq!(slug(Some("  Dell U2720Q  ")), "dell-u2720q");
        assert_eq!(slug(Some("a__b")), "a-b", "runs collapse to one dash");
    }

    // --- primary / scale sanitisation -------------------------------------

    #[test]
    fn r1576_at_most_one_display_is_primary() {
        let two = DisplayTopology::new(vec![
            DisplayInfo::new("a", r(0, 0, 10, 10)).as_primary(),
            DisplayInfo::new("b", r(10, 0, 10, 10)).as_primary(),
        ]);
        assert_eq!(two.iter().filter(|d| d.primary()).count(), 1);
        assert_eq!(two.primary().map(|d| d.id().as_str()), Some("a"));
        // None reported: `primary` is honestly absent, `fallback` still answers.
        let none = DisplayTopology::new(vec![
            DisplayInfo::new("a", r(0, 0, 10, 10)),
            DisplayInfo::new("b", r(10, 0, 10, 10)),
        ]);
        assert!(none.primary().is_none());
        assert_eq!(none.fallback().map(|d| d.id().as_str()), Some("a"));
        // Headless: neither.
        assert!(DisplayTopology::empty().primary().is_none());
        assert!(DisplayTopology::empty().fallback().is_none());
    }

    #[test]
    fn r1576_an_impossible_scale_becomes_one_rather_than_poisoning_the_conversion() {
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let desk = DisplayTopology::new(vec![
                DisplayInfo::new("a", r(0, 0, 100, 100)).with_scale(bad),
            ]);
            let d = desk.iter().next().expect("one display");
            assert!(scale_eq(d.scale_factor(), 1.0), "scale {bad} was kept");
            assert!(
                scale_eq(d.logical_size().0, 100.0),
                "a poisoned scale would make the logical size NaN or infinite"
            );
        }
        let good = DisplayTopology::new(vec![
            DisplayInfo::new("a", r(0, 0, 100, 100)).with_scale(2.0),
        ]);
        let d = good.iter().next().expect("one display");
        assert!(scale_eq(d.scale_factor(), 2.0));
        assert!(scale_eq(d.logical_size().0, 50.0));
    }

    #[test]
    fn r1576_an_unreported_refresh_rate_is_absent_rather_than_zero() {
        let desk = DisplayTopology::new(vec![
            DisplayInfo::new("a", r(0, 0, 10, 10)),
            DisplayInfo::new("b", r(10, 0, 10, 10)).with_refresh_mhz(59_940),
        ]);
        let rates: Vec<Option<u32>> = desk.iter().map(super::Display::refresh_mhz).collect();
        assert_eq!(rates, vec![None, Some(59_940)]);
    }

    // --- bounding box / gaps ----------------------------------------------

    #[test]
    fn r1576_a_row_of_abutting_displays_is_gap_free() {
        let desk = side_by_side();
        assert_eq!(desk.bounding_box(), Some(r(0, 0, 3840, 1080)));
        assert_eq!(desk.covered_px(), 3840 * 1080);
        assert!(desk.is_gap_free());
    }

    #[test]
    fn r1576_an_l_shape_has_a_hole_the_bounding_box_hides() {
        let desk = l_shaped();
        let bb = desk.bounding_box().expect("two displays");
        assert_eq!(bb, r(0, 0, 1500, 1000));
        assert_eq!(desk.covered_px(), 1000 * 1000 + 500 * 400);
        assert!(!desk.is_gap_free());
        // The hole is a real place: inside the bounding box, on no display.
        // This is the exact case where the toolkit code using `virtualGeometry()` containment
        // as a visibility test is wrong.
        assert!(bb.contains(1200, 800));
        assert!(desk.display_at(1200, 800).is_none());
        assert_eq!(bb.area() - desk.covered_px(), 500 * 600, "the hole's area");
    }

    #[test]
    fn r1576_mirrored_displays_are_gap_free_and_resolve_in_order() {
        let desk = DisplayTopology::new(vec![
            DisplayInfo::new("built-in", r(0, 0, 100, 100)).as_primary(),
            DisplayInfo::new("projector", r(0, 0, 100, 100)),
        ]);
        assert_eq!(desk.covered_px(), 10_000, "the overlap counts once");
        assert!(desk.is_gap_free());
        assert_eq!(
            desk.display_at(50, 50).map(|d| d.id().as_str()),
            Some("built-in")
        );
    }

    #[test]
    fn r1576_a_diagonal_pair_is_not_gap_free_and_headless_vacuously_is() {
        let diagonal = DisplayTopology::new(vec![
            DisplayInfo::new("a", r(0, 0, 100, 100)),
            DisplayInfo::new("b", r(100, 100, 100, 100)),
        ]);
        assert!(!diagonal.is_gap_free());
        let headless = DisplayTopology::empty();
        assert_eq!(headless.bounding_box(), None);
        assert_eq!(headless.covered_px(), 0);
        assert!(headless.is_gap_free());
        assert!(headless.is_empty());
        assert_eq!(headless.len(), 0);
    }

    // --- resolve ----------------------------------------------------------

    #[test]
    fn r1576_a_window_wholly_on_one_display_names_that_display() {
        let p = side_by_side().resolve(r(100, 100, 800, 600));
        assert_eq!(p.home.as_ref().map(DisplayId::as_str), Some("dp-1"));
        assert_eq!(p.covering.len(), 1);
        assert!(p.is_fully_visible());
        assert_eq!(p.offscreen_px(), 0);
        assert!(scale_eq(p.visible_fraction(), 1.0));
        // Already visible, so the suggestion is where it already is — which is
        // what lets a caller test its own arithmetic against this field.
        assert_eq!(p.suggestion, Some((100, 100)));
    }

    #[test]
    fn r1576_a_window_on_a_seam_names_both_and_the_shares_sum_to_the_visible() {
        // 400 wide, straddling x = 1920 with 100px on the left panel.
        let p = side_by_side().resolve(r(1820, 0, 400, 100));
        assert_eq!(p.home.as_ref().map(DisplayId::as_str), Some("dp-2"));
        let names: Vec<&str> = p.covering.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(names, vec!["dp-1", "dp-2"], "enumeration order");
        assert_eq!(p.covering[0].px, 100 * 100);
        assert_eq!(p.covering[1].px, 300 * 100);
        assert!(p.is_fully_visible());
        // The two displays do not overlap, so the per-display shares sum to the
        // union exactly. This is the equality half of the relation `Coverage`
        // documents; the mirrored case below is the inequality half.
        let summed: u64 = p.covering.iter().map(|c| c.px).sum();
        assert_eq!(summed, p.visible_px);
        assert_eq!(p.visible_px, p.total_px);
    }

    #[test]
    fn r1576_over_a_mirrored_pair_the_shares_over_count_and_the_union_does_not() {
        let desk = DisplayTopology::new(vec![
            DisplayInfo::new("built-in", r(0, 0, 100, 100)).as_primary(),
            DisplayInfo::new("projector", r(0, 0, 100, 100)),
        ]);
        let p = desk.resolve(r(0, 0, 50, 50));
        assert_eq!(p.covering.len(), 2);
        let summed: u64 = p.covering.iter().map(|c| c.px).sum();
        assert_eq!(summed, 5_000, "each display claims the whole window");
        assert_eq!(p.visible_px, 2_500, "the window is 2,500 pixels, once");
        assert!(summed > p.visible_px);
        assert!(p.is_fully_visible());
    }

    #[test]
    fn r1576_a_window_hanging_off_the_edge_reports_exactly_what_is_lost() {
        // 200 wide at x = 3740 on a 3840-wide desk: half of it is nowhere.
        let p = side_by_side().resolve(r(3740, 0, 200, 100));
        assert_eq!(p.home.as_ref().map(DisplayId::as_str), Some("dp-2"));
        assert_eq!(p.visible_px, 100 * 100);
        assert_eq!(p.total_px, 200 * 100);
        assert_eq!(p.offscreen_px(), 100 * 100);
        assert!(!p.is_fully_visible());
        assert!(scale_eq(p.visible_fraction(), 0.5));
        // And the fix: slide it left until it fits on dp-2.
        assert_eq!(p.suggestion, Some((3640, 0)));
    }

    #[test]
    fn r1576_a_window_on_no_display_at_all_says_so() {
        // The unplugged-monitor case: the preset put it where the second panel
        // used to be, and today there is one panel.
        let one = DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", r(0, 0, 1920, 1080)).as_primary(),
        ]);
        let p = one.resolve(r(2600, 40, 800, 600));
        assert!(p.home.is_none());
        assert!(p.covering.is_empty());
        assert_eq!(p.visible_px, 0);
        assert_eq!(p.offscreen_px(), p.total_px);
        assert!(!p.is_fully_visible());
        assert!(scale_eq(p.visible_fraction(), 0.0));
        assert_eq!(p.suggestion, Some((1120, 40)), "slid back onto the panel");
    }

    #[test]
    fn r1576_a_window_in_an_l_shapes_hole_is_invisible_though_inside_the_bounds() {
        let desk = l_shaped();
        let rect = r(1100, 600, 200, 200);
        assert!(
            desk.bounding_box()
                .expect("two displays")
                .intersection(rect)
                .is_some(),
            "the bounding box says it is on the desktop"
        );
        let p = desk.resolve(rect);
        assert_eq!(p.visible_px, 0, "and not one pixel of it is on a display");
        assert!(p.home.is_none());
    }

    #[test]
    fn r1576_an_empty_window_is_not_fully_visible() {
        let p = side_by_side().resolve(r(10, 10, 0, 600));
        assert_eq!(p.total_px, 0);
        assert_eq!(p.visible_px, 0);
        assert!(!p.is_fully_visible(), "nothing is not everything");
        assert!(scale_eq(p.visible_fraction(), 0.0));
        assert!(p.covering.is_empty());
    }

    #[test]
    fn r1576_a_headless_desk_resolves_everything_to_nowhere() {
        let p = DisplayTopology::empty().resolve(r(0, 0, 800, 600));
        assert!(p.home.is_none());
        assert!(p.covering.is_empty());
        assert_eq!(p.visible_px, 0);
        assert_eq!(p.total_px, 800 * 600);
        assert_eq!(p.suggestion, None, "there is nowhere to suggest");
    }

    #[test]
    fn r1576_a_window_too_big_for_any_display_gets_no_suggestion() {
        let p = side_by_side().resolve(r(0, 0, 3840, 1080));
        // It spans both panels and is wholly visible...
        assert!(p.is_fully_visible());
        // ...but no SINGLE display can hold it, and a suggestion that depends
        // on two monitors staying arranged is not one a preset can rest on.
        assert_eq!(p.suggestion, None);
    }

    #[test]
    fn r1576_the_suggestion_picks_the_nearer_display_and_ties_go_to_enumeration_order() {
        let desk = side_by_side();
        // Sitting just off the right end: dp-2 is nearer than dp-1.
        assert_eq!(
            desk.resolve(r(3900, 0, 100, 100)).suggestion,
            Some((3740, 0))
        );
        // Sitting off the left end: dp-1 is nearer.
        assert_eq!(desk.resolve(r(-500, 0, 100, 100)).suggestion, Some((0, 0)));
        // Two displays equidistant from a window hanging above the seam — each
        // would have to move it by exactly (50, 50). The earlier one wins,
        // deterministically, so the answer is a function of the arrangement and
        // never of iteration luck.
        let ties = DisplayTopology::new(vec![
            DisplayInfo::new("a", r(0, 0, 200, 100)),
            DisplayInfo::new("b", r(200, 0, 200, 100)),
        ]);
        assert_eq!(
            ties.resolve(r(150, -50, 100, 100)).suggestion,
            Some((100, 0))
        );
    }

    // --- anchors ----------------------------------------------------------

    #[test]
    fn r1576_an_anchor_on_a_present_display_is_that_displays_corner_plus_the_offset() {
        let desk = side_by_side();
        let a = desk.anchor(&Anchor::new(id("dp-2"), (40, 30)));
        assert_eq!(a.name(), "on_declared");
        assert!(a.is_declared());
        assert_eq!(a.at(), Some((1960, 30)));
        assert_eq!(a.display().map(DisplayId::as_str), Some("dp-2"));
        // The round trip: the place an anchor names resolves back to its own
        // display, which is the property a preset actually depends on.
        let p = desk.resolve(r(1960, 30, 400, 300));
        assert_eq!(p.home.as_ref().map(DisplayId::as_str), Some("dp-2"));
        assert!(p.is_fully_visible());
    }

    #[test]
    fn r1576_an_anchor_offset_is_logical_so_it_scales_with_the_display() {
        let desk = DisplayTopology::new(vec![
            DisplayInfo::new("laptop", r(0, 0, 2560, 1600))
                .with_scale(2.0)
                .as_primary(),
            DisplayInfo::new("desk", r(2560, 0, 1920, 1080)),
        ]);
        // The same "40 logical pixels in" is 80 physical on the 2x panel and 40
        // on the 1x one — the property that makes one preset mean one visible
        // distance on both.
        assert_eq!(
            desk.anchor(&Anchor::new(id("laptop"), (40, 40))).at(),
            Some((80, 80))
        );
        assert_eq!(
            desk.anchor(&Anchor::new(id("desk"), (40, 40))).at(),
            Some((2600, 40))
        );
    }

    #[test]
    fn r1576_an_anchor_naming_a_vanished_display_is_substituted_and_says_so() {
        let one = DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", r(0, 0, 1920, 1080)).as_primary(),
        ]);
        let a = one.anchor(&Anchor::new(id("dp-2"), (40, 30)));
        assert_eq!(a.name(), "substituted");
        assert!(!a.is_declared());
        assert_eq!(a.display().map(DisplayId::as_str), Some("dp-1"));
        assert_eq!(a.at(), Some((40, 30)));
        assert!(
            matches!(&a, Anchored::Substituted { declared, .. } if declared.as_str() == "dp-2"),
            "the name the preset asked for is still in the answer"
        );
        // And the substituted place is somewhere a person can actually reach,
        // which is the whole reason to substitute rather than obey.
        assert!(one.resolve(r(40, 30, 800, 600)).is_fully_visible());
    }

    #[test]
    fn r1576_an_anchor_on_a_headless_desk_has_no_position_at_all() {
        let a = DisplayTopology::empty().anchor(&Anchor::new(id("dp-1"), (40, 30)));
        assert_eq!(a.name(), "no_display");
        assert_eq!(a.at(), None);
        assert_eq!(a.display(), None);
        assert!(!a.is_declared());
    }

    #[test]
    fn r1576_the_fallback_is_the_primary_when_there_is_one() {
        // Enumeration order says "a" first, but "b" is primary — so a vanished
        // display substitutes onto "b", not onto the first one listed.
        let desk = DisplayTopology::new(vec![
            DisplayInfo::new("a", r(0, 0, 100, 100)),
            DisplayInfo::new("b", r(100, 0, 100, 100)).as_primary(),
        ]);
        let a = desk.anchor(&Anchor::new(id("gone"), (5, 5)));
        assert_eq!(a.display().map(DisplayId::as_str), Some("b"));
        assert_eq!(a.at(), Some((105, 5)));
    }

    #[test]
    fn r1576_an_anchor_round_trips_through_serde() {
        let a = Anchor::new(id("dp-2"), (40, 30));
        let json = serde_json::to_string(&a).expect("serialize");
        let back: Anchor = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, a, "a preset is data a session can write and read");
    }
}
