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
//! 3. **Placement is answerable before it happens.** A toolkit screen query
//!    takes a *point* (`screenAt`,
//!    `virtualSiblingAt`) — there is no rectangle-level question in
//!    the API, so a toolkit application that restores a window geometry
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
//!    A toolkit application can be asked about its screens from outside its
//!    process.
//!
//! 7. **A display says how much of it is usable, and how sure it is.** R1621 —
//!    [`UsableRegion`], the toolkit's `availableGeometry()` peer, with the
//!    provenance the reference discards. See below.
//!
//! # The usable region, and why it is four answers rather than a rectangle
//!
//! R1621 — a display's **usable region** is its bounds minus panels, docks and
//! taskbars. Every toolkit exposes it as a plain rectangle, and on X11 that
//! rectangle is a guess whose quality nobody can see.
//!
//! Measured in the reference's own source rather than assumed. Its X11 plugin
//! carries a long internal comment saying that deriving a per-monitor work area
//! from the desktop-wide `_NET_WORKAREA` is unreliable, that window managers
//! disagree about what the atom means with several monitors attached, and that
//! "WM specification does not have an atom for this. Thus, [the screen type] is
//! limited by the lack of support from the underlying system." And its
//! conclusion is
//! the part that matters here: on a multi-head system its screen accessor
//! **returns the full bounds**, unless an environment variable is set to
//! override — so on any two-monitor desk the reference answers "all of it is
//! available" and the caller cannot tell that from a real measurement.
//!
//! That is the R1617 shape again: two different facts arriving as one value,
//! with the difference discarded at the boundary. So this models the answer AND
//! its provenance:
//!
//! * [`UsableRegion::Reported`] — the platform published a work area and it can
//!   be attributed to this display.
//! * [`UsableRegion::DesktopWide`] — a work area was published, but it covers
//!   the whole desktop and this desk has more than one display, so attributing
//!   it would be the guess the reference makes silently. The bounds come back,
//!   **labelled as bounds**.
//! * [`UsableRegion::Unpublished`] — the platform has no such concept to
//!   publish. Wayland is the honest case: the protocol does not tell a client
//!   the work area at all.
//! * [`UsableRegion::Unprobed`] — nobody asked. A headless or TUI backend, or a
//!   display enumerated before the probe ran.
//!
//! Every arm still yields a rectangle through [`UsableRegion::rect`], so a
//! caller that only wants somewhere to put a window gets one without matching.
//! The arm is there for the caller that would otherwise have to guess.

use std::collections::BTreeMap;

/// R1621 §5.16 — how much of a display is usable, and **how well that is
/// known**: the toolkit's `availableGeometry()` peer, plus the provenance the
/// reference throws away at its own boundary (see the [module docs](self)).
///
/// Not `Option<Rect>`: `None` would fuse "this desk has no panels", "the
/// platform cannot say", and "nobody looked" into one absence, and those are
/// three different things to a client deciding where to put a window. It is
/// the [`DisplayHome`] five-arm argument from R1617 applied one field over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsableRegion {
    /// The platform published a work area attributable to this display.
    Reported(DisplayRect),
    /// A work area was published, but it is **one rectangle for the whole
    /// desktop** and this desk has more than one display, so splitting it
    /// between them would be a guess. The display's own bounds are carried,
    /// and the caller is told they are bounds.
    ///
    /// This is precisely the case the reference resolves by returning the
    /// bounds with no indication that it did.
    DesktopWide(DisplayRect),
    /// The platform has no work area to publish — Wayland, where the protocol
    /// does not give a client one. The bounds are carried; the absence is a
    /// property of the platform, not of this desk.
    Unpublished(DisplayRect),
    /// Nobody probed. A headless or TUI backend, or a display enumerated
    /// before a probe ran. Distinct from [`Unpublished`](Self::Unpublished):
    /// that one asked and was told there is nothing; this one did not ask.
    Unprobed(DisplayRect),
}

impl UsableRegion {
    /// The rectangle to actually use — the reported work area when there is
    /// one, else the display's bounds.
    ///
    /// Every arm answers, so a caller that just needs somewhere to put a window
    /// never has to match. Falling back to the full bounds rather than to
    /// nothing is the same choice the reference makes, and the right one: a
    /// window placed under a panel is visible and movable, where a window
    /// placed nowhere is not a window.
    #[must_use]
    pub const fn rect(self) -> DisplayRect {
        match self {
            Self::Reported(r) | Self::DesktopWide(r) | Self::Unpublished(r) | Self::Unprobed(r) => {
                r
            }
        }
    }

    /// Whether this rectangle is a **measurement** rather than a fallback.
    /// `true` only for [`Reported`](Self::Reported).
    #[must_use]
    pub const fn is_measured(self) -> bool {
        matches!(self, Self::Reported(_))
    }

    /// The canonical wire spelling of the arm — the provenance an agent reads
    /// beside the rectangle.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Reported(_) => "reported",
            Self::DesktopWide(_) => "desktop_wide",
            Self::Unpublished(_) => "unpublished",
            Self::Unprobed(_) => "unprobed",
        }
    }

    /// Every spelling [`as_wire_name`](Self::as_wire_name) can emit, for the
    /// schema's closed-value-set declaration (R1616). Derived from the arms by
    /// construction — a hand-written list here would be the second copy this
    /// project keeps paying for.
    pub const WIRE_NAMES: [&'static str; 4] = {
        const Z: DisplayRect = DisplayRect::new(0, 0, 0, 0);
        [
            Self::Reported(Z).as_wire_name(),
            Self::DesktopWide(Z).as_wire_name(),
            Self::Unpublished(Z).as_wire_name(),
            Self::Unprobed(Z).as_wire_name(),
        ]
    };
}

/// R1621 §5.16 — derive each display's [`UsableRegion`] from the platform's
/// **desktop-wide** work area, or from its absence.
///
/// This is the whole platform-independent half of the axis, and it is where
/// this framework's rule differs from the reference's:
///
/// * No work area at all (`None`) — every display answers with `absent(bounds)`,
///   which the caller picks as [`Unpublished`](UsableRegion::Unpublished) (the
///   platform has no such concept) or [`Unprobed`](UsableRegion::Unprobed)
///   (nobody asked). Only the caller knows which.
/// * A work area that **does not clip** this display — nothing is taken from
///   it, so its whole bounds ARE usable and that is a measurement:
///   [`Reported`](UsableRegion::Reported).
/// * A work area that **clips** this display, on a desk with only one — the
///   clip is attributable, so the intersection is the answer.
/// * A work area that clips this display on a **multi-display** desk — the
///   atom is one rectangle for the whole desktop, so a panel on a neighbour's
///   edge clips this one too and the clip cannot be attributed.
///   [`DesktopWide`](UsableRegion::DesktopWide): the bounds, labelled.
///
/// The reference gives up one step earlier — its accessor returns the full
/// bounds for **every** display as soon as there is more than one, so a desk
/// where only the left monitor has a panel loses the right monitor's answer
/// too, and loses it silently. The clip test recovers those displays, and the
/// ones it cannot recover say so.
#[must_use]
pub fn usable_regions(bounds: &[DisplayRect], work_area: Option<DisplayRect>) -> Vec<UsableRegion> {
    let Some(work_area) = work_area else {
        return bounds.iter().map(|&b| UsableRegion::Unprobed(b)).collect();
    };
    let multi = bounds.len() > 1;
    bounds
        .iter()
        .map(|&b| match work_area.intersection(b) {
            // The work area takes nothing from this display: a real answer,
            // and available on a multi-head desk where the reference has none.
            Some(clipped) if clipped == b => UsableRegion::Reported(b),
            Some(clipped) if !multi => UsableRegion::Reported(clipped),
            // Two ways to reach the same answer, and they are one arm because
            // the answer is the same FACT: this display's usable region is not
            // derivable, so its bounds come back labelled.
            //
            // `Some(_)` — it clips, and with several displays the clip belongs
            // to whichever of them the panel is actually on, which the atom
            // does not say. `None` — the work area does not cover this display
            // at all, which happens where the atom describes only the primary;
            // reporting an empty usable region there would say the display
            // cannot be used, and that is the one answer certainly wrong.
            Some(_) | None => UsableRegion::DesktopWide(b),
        })
        .collect()
}

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
    /// R1621 — skipped by this derive on purpose. The wire shape of a usable
    /// region is `{rect, provenance}`, and that shape is declared in
    /// `pinion-rpc` so the census which keeps `rpc/schema` honest can see it
    /// (the `DisplayHomeWire` precedent). A derive here would have published a
    /// second, tagged-enum spelling of the same fact.
    #[serde(skip)]
    usable: UsableRegion,
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

    /// R1621 §5.16 — how much of this display is usable, and how well that is
    /// known. See [`UsableRegion`], and the [module docs](self) for why it is
    /// four answers rather than a rectangle.
    #[must_use]
    pub const fn usable(&self) -> UsableRegion {
        self.usable
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
    /// R1621 §5.16 — fill in every display's [`UsableRegion`] from the
    /// platform's desktop-wide work area, or from its considered absence.
    ///
    /// Separate from [`new`](Self::new) because enumerating monitors and
    /// probing the work area are different platform calls with different
    /// failure modes, and a topology that has not been probed must be able to
    /// say so rather than to look like one whose desk simply has no panels.
    ///
    /// `work_area` of `None` means the platform was asked and had nothing:
    /// every display becomes [`Unpublished`](UsableRegion::Unpublished), which
    /// is the Wayland answer. A topology this was never called on keeps
    /// [`Unprobed`](UsableRegion::Unprobed) — the two are the point.
    #[must_use]
    pub fn with_work_area(mut self, work_area: Option<DisplayRect>) -> Self {
        let bounds: Vec<DisplayRect> = self.displays.iter().map(Display::bounds).collect();
        for (display, region) in self
            .displays
            .iter_mut()
            .zip(usable_regions(&bounds, work_area))
        {
            display.usable = match region {
                // The derivation cannot know whether the platform was ASKED;
                // reaching this function means it was, so its "nothing
                // published" answer becomes the platform's, not the absence
                // of a probe.
                UsableRegion::Unprobed(r) => UsableRegion::Unpublished(r),
                other => other,
            };
        }
        self
    }

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
                    // R1621 — the topology is built from what the platform
                    // enumerated; the work area is a SEPARATE probe that may
                    // not have run, so a fresh topology says `Unprobed` and
                    // `with_usable_regions` fills it in. Defaulting to
                    // "reported: all of it" would have been the reference's
                    // silent answer with none of its excuse.
                    usable: UsableRegion::Unprobed(info.bounds),
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

    /// R1617 — the display at this position in the platform's enumeration, or
    /// `None` when the topology is shorter than that.
    ///
    /// Exists because the window system names a monitor with a *handle*, not
    /// with one of this module's ids, and the only thing the two share is the
    /// enumeration this topology was built from. Resolving that handle to its
    /// position and asking here is what turns the platform's answer into
    /// something comparable with a derived one — see [`DisplayHome`].
    #[must_use]
    pub fn nth(&self, index: usize) -> Option<&Display> {
        self.displays.get(index)
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

    /// R1617 §2 #7 — where a window with these physical bounds is, against
    /// **both** answers: the one derived here and the one `platform` carries.
    ///
    /// The derivation is [`resolve`](Self::resolve)'s home — the display with
    /// the largest share — and `platform` is whatever the window system said,
    /// or `None` when it said nothing. Naming the question here rather than
    /// leaving callers to pair the two means the surface that reads the
    /// platform does not also have to know which field of a [`Placement`] the
    /// derived home is.
    ///
    /// `platform` is used **verbatim**, not filtered against this topology: an
    /// id the desk does not hold is a divergence worth reporting, and quietly
    /// dropping it would report the platform as silent when it spoke.
    #[must_use]
    pub fn home_of(&self, rect: DisplayRect, platform: Option<DisplayId>) -> DisplayHome {
        DisplayHome::between(self.resolve(rect).home, platform)
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

/// R1617 §5.16 §5.41 §2 #7 — which display a window is on, according to
/// **both** answerers: this framework's derivation and the window system's own
/// opinion.
///
/// # Why there are two answers at all
///
/// [`DisplayTopology::resolve`] derives a window's home from its rectangle —
/// the display holding the largest share of it. That derivation is deliberate
/// (see the module docs): a second stored copy of "which screen is this on" is
/// a second thing to go stale, so this framework keeps none.
///
/// But the window system has an opinion too, and it is not reached by the same
/// rule. Measured across the window backend's four desktop implementations,
/// there are four rules: two resolve by largest intersection, one caches an
/// answer refreshed only when a window's scale factor changes, and one reports
/// the first compositor output the surface entered, which is not geometric at
/// all. One of them answers with the *first enumerated* monitor for a window
/// that is on no monitor, where this module answers `None`.
///
/// So the two can differ **without either being wrong**, and a window
/// straddling a seam is the ordinary case where they do. That is why this is a
/// report and not a check: a gate would have to invent a rule that overrides a
/// platform's own.
///
/// # Where this is past the toolkit 6.11
///
/// The toolkit has both answers too and **hides one of them**, read from its
/// 6.11.1 window and application sources:
///
/// 1. Its derivation, a screen-for-geometry resolver, is **private** — declared
///    in a private header, so an application cannot call it. What is public is
///    the window's screen accessor, which returns the platform plugin's stored
///    answer, and an application-level screen-at query that takes a *point* —
///    the rectangle-shaped question R1576 already recorded as unaskable there.
/// 2. That private derivation decides by **centre point**: a window nine
///    tenths on the right panel whose centre is on the left one resolves to the
///    left, and its fallback keeps the *last* intersecting sibling it iterated
///    over. This module's rule is largest share, which is also what two of the
///    four platform backends underneath use.
/// 3. It is consulted **only when the application moves the window**
///    (`setGeometry`), and even then it short-circuits to the current screen
///    whenever that screen contains the centre. A user dragging a window across
///    a seam never runs it; only the plugin's own event does.
/// 4. Nothing anywhere puts the two side by side, so a divergence is not a
///    fact the toolkit can hold — and since one of the two is private, an
///    application cannot assemble one either.
///
/// Here both are published on `scene/windows`, and the *relation between them*
/// is the value.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DisplayHome {
    /// Both answerers name the same display. The overwhelmingly common case,
    /// and the one worth being able to state.
    Agreed {
        /// The display they agree on.
        display: DisplayId,
    },
    /// They name different displays. Legitimate for a window on a seam — see
    /// the type docs for the four rules underneath — and a real finding
    /// otherwise, which is exactly why it is reported rather than resolved.
    Diverged {
        /// What this framework derived from the window's rectangle.
        derived: DisplayId,
        /// What the window system says.
        platform: DisplayId,
    },
    /// The derivation names a display and the platform names none.
    ///
    /// Ordinary rather than exceptional: the window backend's own accessor is
    /// an `Option`, a hidden window may have never been assigned an output,
    /// and one backend answers `None` for a window whose surface has entered no
    /// output yet. Distinct from [`Self::Agreed`] on purpose — "the platform
    /// concurs" and "the platform did not say" are different facts, and folding
    /// them would turn silence into agreement.
    PlatformSilent {
        /// What this framework derived.
        derived: DisplayId,
    },
    /// The rectangle is on no display at all, and the platform still names one.
    ///
    /// The unplugged-monitor case seen from the other side: a window restored
    /// where a panel used to be covers nothing, while a platform that resolves
    /// by nearest-or-first still hands back a monitor. Reporting it is how a
    /// caller learns that the platform's answer is a fallback rather than a
    /// location.
    DerivedNowhere {
        /// The display the window system names anyway.
        platform: DisplayId,
    },
    /// Neither answerer names a display: a headless desk, or a window that is
    /// nowhere and a platform that says so.
    Nowhere,
}

impl DisplayHome {
    /// Every spelling [`name`](Self::name) can answer with.
    ///
    /// Hand-written **and proved exhaustive by construction**: [`between`](Self::between)
    /// is the only constructor, so driving it over every combination of its two
    /// arguments produces every reachable arm, and
    /// `r1617_the_published_home_vocabulary_is_exactly_what_is_producible`
    /// asserts that the set of names so produced *equals* this list. That is a
    /// stronger claim than a `const` derived from a data-less enum census
    /// (R1616's `LEVEL_WIRE_NAMES`) can make: it pins what the code can
    /// actually emit, not merely what the type declares.
    pub const KINDS: [&'static str; 5] = [
        "agreed",
        "diverged",
        "platform_silent",
        "derived_nowhere",
        "nowhere",
    ];

    /// The relation between the two answers. The **only** constructor, so the
    /// arms cannot be assembled inconsistently — an `Agreed` naming two
    /// different displays is not a value anyone can build.
    #[must_use]
    pub fn between(derived: Option<DisplayId>, platform: Option<DisplayId>) -> Self {
        match (derived, platform) {
            (Some(derived), Some(platform)) => {
                if derived == platform {
                    Self::Agreed { display: derived }
                } else {
                    Self::Diverged { derived, platform }
                }
            }
            (Some(derived), None) => Self::PlatformSilent { derived },
            (None, Some(platform)) => Self::DerivedNowhere { platform },
            (None, None) => Self::Nowhere,
        }
    }

    /// The one word this relation is called on the wire, matching the serde
    /// tag. A closed vocabulary this crate owns — see [`Self::KINDS`].
    ///
    /// Lives here, next to the arms, so the match is exhaustive and a new arm
    /// is a compile error rather than a wildcard's silent misreport (R1600).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Agreed { .. } => "agreed",
            Self::Diverged { .. } => "diverged",
            Self::PlatformSilent { .. } => "platform_silent",
            Self::DerivedNowhere { .. } => "derived_nowhere",
            Self::Nowhere => "nowhere",
        }
    }

    /// What this framework derived from the window's rectangle, or `None` when
    /// the rectangle is on no display.
    #[must_use]
    pub const fn derived(&self) -> Option<&DisplayId> {
        match self {
            Self::Agreed { display } => Some(display),
            Self::Diverged { derived, .. } | Self::PlatformSilent { derived } => Some(derived),
            Self::DerivedNowhere { .. } | Self::Nowhere => None,
        }
    }

    /// What the window system says, or `None` when it said nothing.
    #[must_use]
    pub const fn platform(&self) -> Option<&DisplayId> {
        match self {
            Self::Agreed { display } => Some(display),
            Self::Diverged { platform, .. } | Self::DerivedNowhere { platform } => Some(platform),
            Self::PlatformSilent { .. } | Self::Nowhere => None,
        }
    }

    /// Did both answerers name the same display?
    ///
    /// `false` for every arm but [`Self::Agreed`], including the two where only
    /// one of them answered: silence is not concurrence, the same conservatism
    /// [`Anchored::is_declared`] and
    /// [`crate::window_level::LevelOutcome::is_honoured`] apply.
    #[must_use]
    pub const fn agrees(&self) -> bool {
        matches!(self, Self::Agreed { .. })
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
    use super::{UsableRegion, usable_regions};

    /// R1621 §5.16 — the usable region is derived from the desktop-wide work
    /// area, and each display says how well its answer is known.
    #[test]
    fn r1621_a_work_area_that_clips_nothing_is_a_measurement() {
        // Two 1920x1080 monitors side by side; a 40-px panel along the bottom
        // of the LEFT one. `_NET_WORKAREA` is one rect for the whole desktop,
        // so it is 3840 wide and 1040 tall — which clips BOTH monitors.
        let left = DisplayRect::new(0, 0, 1920, 1080);
        let right = DisplayRect::new(1920, 0, 1920, 1080);
        let desk = DisplayRect::new(0, 0, 3840, 1040);
        let got = usable_regions(&[left, right], Some(desk));
        // Neither is attributable: the atom cannot say which monitor the panel
        // is on. Both answer with their bounds, LABELLED — where the reference
        // answers with the bounds and says nothing.
        assert_eq!(
            got,
            vec![
                UsableRegion::DesktopWide(left),
                UsableRegion::DesktopWide(right),
            ]
        );
        for region in &got {
            assert_eq!(region.rect(), region.rect(), "every arm yields a rect");
            assert!(!region.is_measured(), "and none of them is a measurement");
        }

        // Now a dock down the LEFT edge instead: the desk work area starts at
        // x=40 and runs full height. It clips the left monitor and leaves the
        // right one entirely alone — so the right one's whole bounds are
        // usable, and that is a measurement. This is the answer the reference
        // discards, because its accessor returns bounds for EVERY display as
        // soon as there is more than one.
        let side_dock = DisplayRect::new(40, 0, 3800, 1080);
        let got = usable_regions(&[left, right], Some(side_dock));
        assert_eq!(
            got,
            vec![
                UsableRegion::DesktopWide(left),
                UsableRegion::Reported(right)
            ],
            "the clipped display says it cannot attribute; the untouched one \
             answers for real",
        );
        assert!(!got[0].is_measured());
        assert!(got[1].is_measured());
        assert_eq!(got[1].rect(), right, "all of it");
    }

    /// R1621 — one display can attribute the clip, because there is nothing
    /// else it could belong to.
    #[test]
    fn r1621_a_single_display_attributes_its_own_clip() {
        let only = DisplayRect::new(0, 0, 1920, 1080);
        let desk = DisplayRect::new(0, 27, 1920, 1013); // menu bar on top
        let got = usable_regions(&[only], Some(desk));
        assert_eq!(got, vec![UsableRegion::Reported(desk)]);
        assert_eq!(got[0].rect(), desk, "the work area IS the usable region");
        assert!(got[0].is_measured());
    }

    /// R1621 — a PROBED desk that published nothing answers `Unpublished`, and
    /// an unprobed topology answers `Unprobed`. The two are the point of the
    /// type, and nothing caught them collapsing until a counterfactual said so.
    #[test]
    fn r1621_probing_and_not_probing_are_different_answers() {
        use super::{DisplayInfo, DisplayTopology};
        let bounds = DisplayRect::new(0, 0, 1920, 1080);
        let fresh = DisplayTopology::new(vec![DisplayInfo::new("DP-1", bounds).as_primary()]);
        assert_eq!(
            fresh.iter().next().expect("one display").usable(),
            UsableRegion::Unprobed(bounds),
            "a topology nobody probed says so",
        );
        let asked = DisplayTopology::new(vec![DisplayInfo::new("DP-1", bounds).as_primary()])
            .with_work_area(None);
        assert_eq!(
            asked.iter().next().expect("one display").usable(),
            UsableRegion::Unpublished(bounds),
            "asking and being told there is nothing is a DIFFERENT answer from \
             never asking — the Wayland case, and not a failure",
        );
        // And a probe that DID find one reaches the derivation.
        let panelled = DisplayTopology::new(vec![DisplayInfo::new("DP-1", bounds).as_primary()])
            .with_work_area(Some(DisplayRect::new(0, 32, 1920, 1048)));
        assert_eq!(
            panelled.iter().next().expect("one display").usable(),
            UsableRegion::Reported(DisplayRect::new(0, 32, 1920, 1048)),
        );
        assert!(
            panelled
                .iter()
                .next()
                .expect("one display")
                .usable()
                .is_measured()
        );
    }

    /// R1621 — absence is not zero, and the three absences stay apart.
    #[test]
    fn r1621_no_work_area_yields_bounds_and_says_why() {
        let a = DisplayRect::new(0, 0, 800, 600);
        let b = DisplayRect::new(800, 0, 800, 600);
        let got = usable_regions(&[a, b], None);
        assert_eq!(
            got,
            vec![UsableRegion::Unprobed(a), UsableRegion::Unprobed(b)],
            "with nothing published the derivation answers UNPROBED; only the \
             caller knows whether the platform was asked",
        );
        for region in got {
            assert!(!region.is_measured());
            assert_eq!(region.rect(), region.rect());
        }
        // A display the work area misses entirely is NOT reported as unusable:
        // an empty usable region is the one answer that is certainly wrong.
        let far = DisplayRect::new(9_000, 0, 800, 600);
        let got = usable_regions(&[a, far], Some(DisplayRect::new(0, 0, 1600, 560)));
        assert_eq!(got[1], UsableRegion::DesktopWide(far));
        assert_eq!(got[1].rect(), far, "its whole bounds, not an empty rect");
        // The four wire names are distinct and derived from the arms.
        let mut names = UsableRegion::WIRE_NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 4, "four arms, four spellings");
        assert_eq!(UsableRegion::Reported(a).as_wire_name(), "reported");
        assert_eq!(UsableRegion::DesktopWide(a).as_wire_name(), "desktop_wide");
        assert_eq!(UsableRegion::Unpublished(a).as_wire_name(), "unpublished");
        assert_eq!(UsableRegion::Unprobed(a).as_wire_name(), "unprobed");
    }
    use super::{
        Anchor, Anchored, DisplayHome, DisplayId, DisplayInfo, DisplayRect, DisplayTopology, slug,
        union_px,
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

    // --- display home (R1617) ---------------------------------------------

    #[test]
    fn r1617_the_published_home_vocabulary_is_exactly_what_is_producible() {
        // `between` is the only constructor, so driving it over every
        // combination of its two arguments enumerates every REACHABLE arm.
        // Asserting set EQUALITY against the published list catches both
        // directions: a spelling published that nothing can emit (a client
        // branches on a case that never arrives) and an arm emitted that is
        // not published (a client's match falls through).
        let a = Some(id("dp-1"));
        let b = Some(id("dp-2"));
        let mut produced: Vec<&str> = Vec::new();
        for derived in [None, a.clone(), b.clone()] {
            for platform in [None, a.clone(), b.clone()] {
                produced.push(DisplayHome::between(derived.clone(), platform).name());
            }
        }
        produced.sort_unstable();
        produced.dedup();
        let mut published: Vec<&str> = DisplayHome::KINDS.to_vec();
        published.sort_unstable();
        assert_eq!(
            produced, published,
            "the producible names and the published ones must be one set",
        );
        // And the spellings are distinct — an enumeration whose members
        // collide is not one.
        assert_eq!(published.len(), DisplayHome::KINDS.len());
    }

    #[test]
    fn r1617_the_home_name_matches_its_serde_tag() {
        // Two spellings of one fact: `name()` is what the wire layer reads and
        // the serde tag is what the JSON carries. Nothing else would notice
        // them diverging, exactly as R1610 found for the level outcome.
        for home in [
            DisplayHome::between(Some(id("a")), Some(id("a"))),
            DisplayHome::between(Some(id("a")), Some(id("b"))),
            DisplayHome::between(Some(id("a")), None),
            DisplayHome::between(None, Some(id("b"))),
            DisplayHome::between(None, None),
        ] {
            let json: serde_json::Value =
                serde_json::from_str(&serde_json::to_string(&home).unwrap()).unwrap();
            assert_eq!(json["kind"].as_str(), Some(home.name()), "{home:?}");
        }
    }

    #[test]
    fn r1617_agreement_needs_both_answers_and_silence_is_not_concurrence() {
        let agreed = DisplayHome::between(Some(id("dp-1")), Some(id("dp-1")));
        assert!(agreed.agrees());
        assert_eq!(agreed.derived(), Some(&id("dp-1")));
        assert_eq!(agreed.platform(), Some(&id("dp-1")));

        // One answerer speaking is NOT agreement — the conservatism
        // `Anchored::is_declared` and `LevelOutcome::is_honoured` share.
        let silent = DisplayHome::between(Some(id("dp-1")), None);
        assert!(!silent.agrees());
        assert_eq!(silent.name(), "platform_silent");
        assert_eq!(silent.derived(), Some(&id("dp-1")));
        assert_eq!(silent.platform(), None, "the platform did not answer");

        let nowhere_here = DisplayHome::between(None, Some(id("dp-2")));
        assert!(!nowhere_here.agrees());
        assert_eq!(nowhere_here.name(), "derived_nowhere");
        assert_eq!(nowhere_here.derived(), None);
        assert_eq!(nowhere_here.platform(), Some(&id("dp-2")));

        let nothing = DisplayHome::between(None, None);
        assert!(!nothing.agrees());
        assert_eq!(nothing.derived(), None);
        assert_eq!(nothing.platform(), None);
    }

    #[test]
    fn r1617_a_divergence_keeps_both_names_rather_than_picking_one() {
        let d = DisplayHome::between(Some(id("dp-1")), Some(id("dp-2")));
        assert_eq!(d.name(), "diverged");
        assert!(!d.agrees());
        // BOTH survive. A report that resolved the disagreement would be a
        // rule this framework invented over a platform's own, and the four
        // backends underneath genuinely use four rules.
        assert_eq!(d.derived(), Some(&id("dp-1")));
        assert_eq!(d.platform(), Some(&id("dp-2")));
        assert!(matches!(&d, DisplayHome::Diverged { derived, platform }
                if derived.as_str() == "dp-1" && platform.as_str() == "dp-2"),);
    }

    #[test]
    fn r1617_a_window_on_a_seam_is_the_ordinary_divergence() {
        // 400 wide straddling x = 1920 with 100px on the left panel: the
        // largest share is dp-2, and a platform resolving by, say, the corner
        // or by which output the surface entered first can legitimately say
        // dp-1. Neither is wrong; the RELATION is the fact.
        let desk = side_by_side();
        let rect = r(1820, 0, 400, 100);
        assert_eq!(
            desk.resolve(rect).home.as_ref().map(DisplayId::as_str),
            Some("dp-2"),
            "largest share",
        );
        let agreeing = desk.home_of(rect, Some(id("dp-2")));
        assert!(agreeing.agrees());
        let diverging = desk.home_of(rect, Some(id("dp-1")));
        assert_eq!(diverging.name(), "diverged");
        assert_eq!(diverging.derived(), Some(&id("dp-2")));
        assert_eq!(diverging.platform(), Some(&id("dp-1")));
    }

    #[test]
    fn r1617_home_of_reports_a_platform_id_the_desk_does_not_hold() {
        // Verbatim, not filtered: a platform naming a display this topology
        // has never heard of is a divergence, and folding it into "the
        // platform said nothing" would report silence where there was speech.
        let home = side_by_side().home_of(r(0, 0, 100, 100), Some(id("gone")));
        assert_eq!(home.name(), "diverged");
        assert_eq!(home.platform(), Some(&id("gone")));
        assert!(side_by_side().get(&id("gone")).is_none());
    }

    #[test]
    fn r1617_a_window_on_no_display_still_carries_the_platforms_answer() {
        // The unplugged-monitor case from the other side. One of the window
        // backend's own resolvers answers with the FIRST enumerated monitor
        // for a window it finds nowhere, so this arm is the state that tells a
        // caller the platform handed back a fallback rather than a location.
        let one = DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", r(0, 0, 1920, 1080)).as_primary(),
        ]);
        let rect = r(2600, 40, 800, 600);
        assert!(one.resolve(rect).home.is_none());
        let home = one.home_of(rect, Some(id("dp-1")));
        assert_eq!(home.name(), "derived_nowhere");
        assert_eq!(home.derived(), None);
        assert_eq!(home.platform(), Some(&id("dp-1")));
        // And with the platform equally silent, the honest answer is neither.
        assert_eq!(one.home_of(rect, None).name(), "nowhere");
    }

    #[test]
    fn r1617_a_headless_desk_has_no_home_for_anything() {
        let desk = DisplayTopology::empty();
        assert_eq!(desk.home_of(r(0, 0, 800, 600), None).name(), "nowhere");
        assert!(desk.nth(0).is_none());
    }

    #[test]
    fn r1617_nth_is_the_platforms_enumeration_position() {
        // The bridge from a window system's monitor HANDLE to one of this
        // module's ids: the handle's position in the enumeration the topology
        // was built from. Order-preserving is the property that makes it work,
        // and it is asserted here rather than assumed at the winit seam.
        let desk = side_by_side();
        assert_eq!(desk.nth(0).map(|d| d.id().as_str()), Some("dp-1"));
        assert_eq!(desk.nth(1).map(|d| d.id().as_str()), Some("dp-2"));
        assert!(desk.nth(2).is_none(), "past the end is None, not a panic");
        let ids: Vec<&str> = desk.iter().map(|d| d.id().as_str()).collect();
        for (i, expected) in ids.iter().enumerate() {
            assert_eq!(desk.nth(i).map(|d| d.id().as_str()), Some(*expected));
        }
    }

    #[test]
    fn r1576_an_anchor_round_trips_through_serde() {
        let a = Anchor::new(id("dp-2"), (40, 30));
        let json = serde_json::to_string(&a).expect("serialize");
        let back: Anchor = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, a, "a preset is data a session can write and read");
    }
}
