//! R1607 §5.21 — **a dashboard's tiles are a value**, and the layout it needs
//! already existed.
//!
//! The analyzer-tool audit's plane-C gap was a monitoring-tool-class tile
//! dashboard: independent cards on a fixed column grid, dragged, snapped and
//! resized, with the arrangement saved under a name. pinion's docking is the
//! toolkit's dock widget model — a splitter tree with tabs and tear-off — and
//! that is a **different thing**: a splitter tree is nested bisection, so its
//! freedom is bounded by the tree's shape, and widening one card re-divides
//! its neighbours.
//!
//! ## What this round did NOT build, and why that is the finding
//!
//! The debt registering this gap asked for one thing first: **measure whether
//! R1560's CSS Grid already holds it** before adding a second layout kind. It
//! does. `pinion-runtime`'s `tile_dashboard_measurement` places a full-width
//! header, two halves and a two-row-tall card on twelve `Fr(1.0)` tracks and
//! asserts the resolved pixels, and asserts the property a splitter tree cannot
//! have — widening one card leaves the other rows' columns alone, because grid
//! tracks are sized once **across** the container.
//!
//! So there is no new layout here. What was missing is the half a layout engine
//! has no opinion about: **which tile is where**, the rule that keeps two of
//! them off the same slot, and what a drag does to the tiles it lands on.
//!
//! ## The invariant is the type
//!
//! [`TileGrid`] holds tiles that **do not overlap**, and every operation either
//! keeps that true or is refused by name. That is the whole difficulty: a drag
//! does not fail when it lands on an occupied slot, it *displaces* — so the
//! interesting part is not rejection but a reflow that terminates, is
//! deterministic, and can be told to a person.
//!
//! ## Past the dashboard tool
//!
//! The dashboard tool pushes displaced panels down and **says nothing**; its
//! layout runs as a side effect of the drag. Two choices here differ:
//!
//! * **A move reports what it displaced.** [`Reflow`] names every tile that
//!   moved and where it came from, which is what an undo record, a "your layout
//!   changed" notice, and an agent driving the dashboard over the wire all need.
//! * **Compaction is a verb, not a consequence.** the dashboard tool floats tiles up to
//!   close gaps automatically, so a drag has a non-local effect nobody asked
//!   for and its inverse is not a drag. [`TileGrid::compact`] is a separate
//!   call, so an editor decides whether "tidy up" is part of the gesture.
//!
//! ## R1609 — an arrangement is editable without a pointer, and an edge is a
//! handle
//!
//! R1608 gave the board a drag and left two things open: a resize existed only
//! as a *verb* (no corner handle), and there was **no keyboard channel at all**
//! — a screen-reader user could read the arrangement and not change it. Both
//! close here, and they close *together*, because they are the same derivation:
//!
//! > **Moving one edge of a tile to a grid line, holding the opposite edge
//! > still.** A [`TileHandle`] drag moves one edge (a side) or two (a corner);
//! > a [`TileNudge`] moves one by a single cell. One private `set_edge`, four
//! > public entry points.
//!
//! The toolkit's floor for all of this is MDI child window, and it is a real
//! floor — keyboard move *and* resize exist there. Measured against that child
//! window's implementation in the toolkit 6.11.1, five things here are
//! different on purpose:
//!
//! * **No mode.** the toolkit's keyboard editing lives behind `isInInteractiveMode`,
//!   entered only from the *system menu* — `_q_enterInteractiveMode` starts by
//!   casting `q->sender()` to a action and returns if it is not one of
//!   `actions[MoveAction]` / `actions[ResizeAction]`. So switching from moving
//!   to resizing costs a menu round trip. Here the chord says which, and
//!   [`TileNudge`] is one flat vocabulary of twelve.
//! * **Nothing warps the pointer.** the toolkit implements each arrow key as
//!   `parentWidget()->mapFromGlobal(cursor().pos() + delta)` and then
//!   `cursor().setPos(...)` to catch the mouse up, with the *whole*
//!   `keyPressEvent` body inside `#ifndef QT_NO_CURSOR` — so on a cursor-less
//!   build the toolkit has no keyboard layout editing at all, and on every other build a
//!   keyboard user's physical pointer jumps across the screen. A nudge here is
//!   arithmetic on cells.
//! * **A keyboard edit can be taken back.** the toolkit saves `oldGeometry` on entering
//!   interactive mode and **never restores it**: `Key_Escape`, `Key_Return` and
//!   `Key_Enter` all fall to the same `leaveInteractiveMode()`, so Escape
//!   *commits*. [`TileEdit`] is the undo point, and it must be one rather than
//!   a saved rectangle because a move displaces *other* tiles — restoring the
//!   card alone would leave the board rearranged around it.
//! * **A session's reflow is a difference, not a sum.** Five nudges that push
//!   one card from row 1 to row 5 are one displacement, not five;
//!   [`Reflow::between`] derives it from the two arrangements.
//! * **Arrow navigation is total, and the invariant is why.** Two tiles that do
//!   not overlap are separated on at least one axis — that is literally the
//!   negation of [`Tile::overlaps`] — so every other tile lies beyond one of
//!   the four edges ([`Tile::lies_beyond`]) and [`TileGrid::neighbour`] can
//!   reach all of them. MDI area cannot have this property, because MDI
//!   windows *may* overlap; its navigation is `activateNextSubWindow`, a walk
//!   down a list in creation order.
//!
//! One more, on the pointer side: the toolkit's `initOperationMap` hand-writes nine rows
//! pairing each of its private `Operation` values with a `ChangeFlag` bitmask (`HMove | HResize | HResizeReverse` …) **and** a
//! cursor, and the enum, the map and the regions are all private, so no caller
//! can enumerate the handles a subwindow has. [`TileHandle::ALL`] is the enumeration, and
//! both the edges it moves and the [`TileHandle::cursor`] it asks for are *derived* from which
//! axes it touches.

use serde::{Deserialize, Serialize};

use crate::stacking::{Revealed, Stack, StackRefusal, StackUnreadable};
use crate::style::{CursorHint, GridPlacement};

/// A tile's identity, stable across moves and saves.
///
/// The application's own — a panel id, a widget key — so a layout can be
/// serialized beside the panels it arranges and re-bound after a reload.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TileId(pub String);

impl TileId {
    /// A tile id from anything string-like.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// (R1733) So a call that takes an id can be handed a literal, the way
/// [`TileId::new`] already allows. Two concrete impls rather than one blanket
/// one over `Into<String>`, because a blanket impl would collide with the
/// reflexive `From<T> for T`.
impl From<&str> for TileId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

impl From<String> for TileId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for TileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One card's slot: a rectangle of grid cells.
///
/// **Zero-based**, like the dashboard tool's `gridPos`, because a model that counts
/// from zero and a CSS grid that counts from one is one conversion — and it
/// happens in exactly one place ([`TileGrid::placement`]) rather than at every call site.
///
/// # ★★★★★ Its stored form (R1900)
///
/// `{"id": "packet", "col": 0, "row": 0, "w": 6, "h": 2}` — and, only when the
/// cell is shared, a `"here"` listing every occupant in strip order:
///
/// ```json
/// {"id": "share", "col": 0, "row": 0, "w": 6, "h": 2, "here": ["packet", "share"]}
/// ```
///
/// **`id` is the front**, exactly as it is in memory, so the wire has no second
/// place where "who is in front" is written and no index that can point past
/// the end. An arrangement saved before this round has no `here` and loads as
/// a cell with one occupant — which is what it was. That compatibility is not
/// a courtesy: R1897 made a person's layout outlive the run that saved it, so
/// a field added here without it would have deleted layouts already on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "TileWire", try_from = "TileWire")]
pub struct Tile {
    /// **Whose tile — the occupant a reader sees**, which is the front of
    /// [`Self::members`] and not a separate name.
    ///
    /// ★★★★★ R1900 — before this round a tile's id and its card's id were the
    /// same fact spelled once, so a cell could not be shared: the type had no
    /// room for a second occupant. It now holds a [`Stack`], and this field is
    /// kept equal to that stack's front by every mutation on
    /// [`TileGrid`] — so a shell that reads `tile.id` to decide what to paint
    /// keeps working unchanged, and a shell that wants the strip asks
    /// [`Self::members`].
    pub id: TileId,
    /// Zero-based column of the left edge.
    pub col: u32,
    /// Zero-based row of the top edge.
    pub row: u32,
    /// Width in columns. Never zero.
    pub w: u32,
    /// Height in rows. Never zero.
    pub h: u32,
    /// Who shares this cell, in tab-strip order, and which of them is in front.
    ///
    /// Private, because the invariant that `id` equals its front is this
    /// module's to keep. A tile placed the ordinary way holds exactly one.
    stack: Stack<TileId>,
}

/// A [`Tile`] as it is stored. See the note on [`Tile`] for why `here` is
/// absent from a cell with one occupant and why there is no front index.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TileWire {
    id: TileId,
    col: u32,
    row: u32,
    w: u32,
    h: u32,
    /// Everyone sharing the cell, in strip order, including the front. Omitted
    /// when the cell is not shared — so an arrangement written before cells
    /// could be shared reads back unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    here: Vec<TileId>,
}

impl From<Tile> for TileWire {
    fn from(tile: Tile) -> Self {
        let here = if tile.stack.is_shared() {
            tile.stack.members().to_vec()
        } else {
            Vec::new()
        };
        Self {
            id: tile.id,
            col: tile.col,
            row: tile.row,
            w: tile.w,
            h: tile.h,
            here,
        }
    }
}

impl TryFrom<TileWire> for Tile {
    type Error = StackUnreadable;

    fn try_from(stored: TileWire) -> Result<Self, Self::Error> {
        let members = if stored.here.is_empty() {
            vec![stored.id.clone()]
        } else {
            stored.here
        };
        Ok(Self {
            stack: Stack::rebuild(members, &stored.id)?,
            id: stored.id,
            col: stored.col,
            row: stored.row,
            w: stored.w,
            h: stored.h,
        })
    }
}

impl Tile {
    /// A tile at `(col, row)` covering `w` x `h` cells, occupied by `id` alone.
    #[must_use]
    pub fn new(id: impl Into<String>, col: u32, row: u32, w: u32, h: u32) -> Self {
        let id = TileId::new(id);
        Self {
            stack: Stack::of(id.clone()),
            id,
            col,
            row,
            w,
            h,
        }
    }

    /// (R1900) Who shares this cell, in the order a tab strip draws them.
    ///
    /// One long, and equal to `[self.id]`, unless something has joined it.
    #[must_use]
    pub fn members(&self) -> &[TileId] {
        self.stack.members()
    }

    /// (R1900) Whether more than one occupant shares this cell — the only
    /// condition a painter should branch on to draw a tab strip.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        self.stack.is_shared()
    }

    /// (R1900) Whether `id` is one of this cell's occupants.
    ///
    /// This is what every lookup on [`TileGrid`] asks, so a shell can go on
    /// naming a card and get the cell that holds it whether or not the cell is
    /// shared.
    #[must_use]
    pub fn holds(&self, id: &TileId) -> bool {
        self.stack.holds(id)
    }

    /// (R1900) Where `id` sits in the strip.
    #[must_use]
    pub fn position(&self, id: &TileId) -> Option<usize> {
        self.stack.position(id)
    }

    /// (R1900) Which tab is in front.
    #[must_use]
    pub fn fore_index(&self) -> usize {
        self.stack.fore_index()
    }

    /// One past the right edge.
    #[must_use]
    pub const fn right(&self) -> u32 {
        self.col + self.w
    }

    /// One past the bottom edge.
    #[must_use]
    pub const fn bottom(&self) -> u32 {
        self.row + self.h
    }

    /// Whether the two tiles share a cell.
    ///
    /// Half-open on both axes, so tiles that merely touch do not overlap —
    /// which is what makes a packed dashboard legal rather than a violation.
    #[must_use]
    pub const fn overlaps(&self, other: &Self) -> bool {
        self.col < other.right()
            && other.col < self.right()
            && self.row < other.bottom()
            && other.row < self.bottom()
    }

    /// (R1609) Whether this tile lies entirely beyond `from`'s `dir` edge — the
    /// half-plane [`TileGrid::neighbour`] searches.
    ///
    /// ★ **This is where arrow navigation gets its totality, and the source is
    /// the invariant itself.** Expand [`Self::overlaps`] and negate it: two
    /// tiles that do not overlap satisfy `a.right() <= b.col` or
    /// `b.right() <= a.col` or `a.bottom() <= b.row` or `b.bottom() <= a.row` —
    /// which is exactly "one of them lies beyond one of the other's four
    /// edges". So on a legal arrangement *every* other card is a candidate in at
    /// least one direction, and no card can be stranded where no arrow key
    /// reaches it.
    ///
    /// MDI area cannot make this claim, because its subwindows may overlap and
    /// the negation therefore does not hold; `activateNextSubWindow` walks a
    /// list in creation order instead, so its "next" window bears no relation
    /// to where the user is looking.
    #[must_use]
    pub const fn lies_beyond(&self, from: &Self, dir: TileDirection) -> bool {
        match dir {
            TileDirection::Left => self.right() <= from.col,
            TileDirection::Right => from.right() <= self.col,
            TileDirection::Up => self.bottom() <= from.row,
            TileDirection::Down => from.bottom() <= self.row,
        }
    }

    /// Whether the two tiles' row ranges intersect — regardless of columns.
    #[must_use]
    const fn shares_rows(&self, other: &Self) -> bool {
        self.row < other.bottom() && other.row < self.bottom()
    }

    /// Whether the two tiles' column ranges intersect — regardless of rows.
    #[must_use]
    const fn shares_columns(&self, other: &Self) -> bool {
        self.col < other.right() && other.col < self.right()
    }
}

/// (R1609) One side of a tile's rectangle.
///
/// The unit every resize is expressed in: a gesture moves an edge to a grid
/// line and the opposite edge stays put, which is what makes dragging a card's
/// left side change both its column *and* its width without those being two
/// decisions that can disagree.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, pinion_derive::VariantCensus,
)]
#[variant_census(all)]
pub enum TileEdge {
    /// The left side — moving it changes `col` and `w`.
    Left,
    /// The right side — moving it changes `w` alone.
    Right,
    /// The top side — moving it changes `row` and `h`.
    Top,
    /// The bottom side — moving it changes `h` alone.
    Bottom,
}

impl TileEdge {
    /// Every edge, so a consumer can enumerate rather than spell four out.
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    /// Whether this edge is the one nearer the grid's origin.
    ///
    /// The single asymmetry in the whole resize derivation: growing a *start*
    /// edge moves its line **down** in value and growing an *end* edge moves it
    /// up, so [`TileGrid::nudge`] needs this and nothing else to turn a
    /// direction into a signed step.
    #[must_use]
    pub const fn is_start(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }

    /// Whether this edge runs along the column axis.
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    /// Where this edge of `tile` currently sits, as a zero-based grid line.
    ///
    /// A start edge's line is the cell it occupies; an end edge's line is one
    /// *past* the last cell — the same half-open convention [`Tile::right`] and
    /// [`Tile::bottom`] already use, so a resize and the overlap test measure
    /// the rectangle the same way.
    #[must_use]
    pub const fn line_of(self, tile: &Tile) -> u32 {
        match self {
            Self::Left => tile.col,
            Self::Right => tile.right(),
            Self::Top => tile.row,
            Self::Bottom => tile.bottom(),
        }
    }
}

/// (R1609) Where a resize gesture grabbed a tile: one edge, or the two that
/// meet at a corner.
///
/// The toolkit's peer is the private `Operation`, which has the same eight resize
/// values plus `Move` and `None`. Two differences, and both come from *deriving*
/// rather than tabulating: the set is [enumerable](Self::ALL) where the
/// toolkit's enum is in a `_p.h` and its region map is private, and the
/// [cursor](Self::cursor) follows from which axes the handle moves where `initOperationMap`
/// writes one per row by hand.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, pinion_derive::VariantCensus,
)]
#[variant_census(all)]
pub enum TileHandle {
    /// The left side alone.
    Left,
    /// The right side alone.
    Right,
    /// The top side alone.
    Top,
    /// The bottom side alone.
    Bottom,
    /// The top-left corner — left and top together.
    TopLeft,
    /// The top-right corner.
    TopRight,
    /// The bottom-left corner.
    BottomLeft,
    /// The bottom-right corner — the only one size grip can be.
    BottomRight,
}

impl TileHandle {
    /// Every handle, in a stable order.
    ///
    /// What lets a card paint its whole handle ring in a loop — and what MDI
    /// child window cannot answer at all, since `isPersistent`-style per-thing queries
    /// are the only public surface and `Operation` is private.
    pub const ALL: [Self; 8] = [
        Self::Left,
        Self::Right,
        Self::Top,
        Self::Bottom,
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    /// The column-axis edge this handle moves, if any.
    #[must_use]
    pub const fn horizontal(self) -> Option<TileEdge> {
        match self {
            Self::Left | Self::TopLeft | Self::BottomLeft => Some(TileEdge::Left),
            Self::Right | Self::TopRight | Self::BottomRight => Some(TileEdge::Right),
            Self::Top | Self::Bottom => None,
        }
    }

    /// The row-axis edge this handle moves, if any.
    #[must_use]
    pub const fn vertical(self) -> Option<TileEdge> {
        match self {
            Self::Top | Self::TopLeft | Self::TopRight => Some(TileEdge::Top),
            Self::Bottom | Self::BottomLeft | Self::BottomRight => Some(TileEdge::Bottom),
            Self::Left | Self::Right => None,
        }
    }

    /// Whether this handle moves `edge`.
    ///
    /// Derived from the two axis accessors rather than a third table, so a
    /// handle cannot claim an edge its own resize does not touch.
    #[must_use]
    pub fn moves(self, edge: TileEdge) -> bool {
        self.horizontal() == Some(edge) || self.vertical() == Some(edge)
    }

    /// The mouse cursor this handle asks for.
    ///
    /// **Derived, including the diagonal's slope.** A handle on one axis wants
    /// that axis's double arrow. A corner wants a diagonal, and *which*
    /// diagonal follows from [`TileEdge::is_start`]: two start edges (top-left) or two end
    /// edges (bottom-right) lie on the `⤡` diagonal, one of each on `⤢`. The
    /// toolkit writes the same four cursors as literal values in nine `operationMap.insert` rows
    /// beside a hand-written `ChangeFlag` mask, so a row whose flags and cursor
    /// disagree is a state that exists there and cannot exist here.
    #[must_use]
    pub const fn cursor(self) -> CursorHint {
        match (self.horizontal(), self.vertical()) {
            (Some(h), Some(v)) => {
                if h.is_start() == v.is_start() {
                    CursorHint::NwseResize
                } else {
                    CursorHint::NeswResize
                }
            }
            (Some(_), None) => CursorHint::ColResize,
            (None, _) => CursorHint::RowResize,
        }
    }

    /// Which handle a point inside a card hits, `None` for its interior.
    ///
    /// `u` and `v` are the point's position within the card as `[0, 1]`
    /// fractions; `band` is how much of each side counts as a handle, clamped
    /// to at most half so the left and right bands cannot both claim a point.
    ///
    /// The toolkit's `getOperation` walks nine cached regions that `updateDirtyRegions` has to rebuild
    /// whenever the widget's geometry changes — a second copy of the geometry,
    /// kept in step by a callback. This is a pure function of the point, so
    /// there is nothing to keep in step and nothing to invalidate. A
    /// non-finite coordinate compares false against both bounds and so reads
    /// as the interior, which is the safe answer: a bad pointer sample moves a
    /// card rather than resizing it.
    #[must_use]
    pub fn at(u: f32, v: f32, band: f32) -> Option<Self> {
        let band = band.clamp(f32::EPSILON, 0.5);
        let horizontal = if u < band {
            Some(TileEdge::Left)
        } else if u > 1.0 - band {
            Some(TileEdge::Right)
        } else {
            None
        };
        let vertical = if v < band {
            Some(TileEdge::Top)
        } else if v > 1.0 - band {
            Some(TileEdge::Bottom)
        } else {
            None
        };
        match (horizontal, vertical) {
            (Some(TileEdge::Left), Some(TileEdge::Top)) => Some(Self::TopLeft),
            (Some(TileEdge::Left), Some(_)) => Some(Self::BottomLeft),
            (Some(_), Some(TileEdge::Top)) => Some(Self::TopRight),
            (Some(_), Some(_)) => Some(Self::BottomRight),
            (Some(TileEdge::Left), None) => Some(Self::Left),
            (Some(_), None) => Some(Self::Right),
            (None, Some(TileEdge::Top)) => Some(Self::Top),
            (None, Some(_)) => Some(Self::Bottom),
            (None, None) => None,
        }
    }
}

/// (R1609) The four directions a keyboard edit works in.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, pinion_derive::VariantCensus,
)]
#[variant_census(all)]
pub enum TileDirection {
    /// Toward column zero.
    Left,
    /// Away from column zero.
    Right,
    /// Toward row zero.
    Up,
    /// Away from row zero.
    Down,
}

impl TileDirection {
    /// Every direction, so a keymap can be built by iteration.
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Up, Self::Down];

    /// The edge a resize in this direction acts on.
    #[must_use]
    pub const fn edge(self) -> TileEdge {
        match self {
            Self::Left => TileEdge::Left,
            Self::Right => TileEdge::Right,
            Self::Up => TileEdge::Top,
            Self::Down => TileEdge::Bottom,
        }
    }

    /// The direction that undoes this one.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }
}

/// (R1609) A one-cell keyboard edit — the whole vocabulary, twelve values.
///
/// Flat on purpose. The toolkit reaches the same behaviours through a *mode*
/// (`currentOperation`, set from a system-menu action) plus a delta, which means the same
/// arrow key means different things depending on state the user cannot see and
/// a screen reader is not told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TileNudge {
    /// Slide the whole card one cell.
    Move(TileDirection),
    /// Push that side one cell outward, making the card bigger.
    Grow(TileDirection),
    /// Pull that side one cell inward, making the card smaller.
    Shrink(TileDirection),
}

impl TileNudge {
    /// The direction this nudge names.
    #[must_use]
    pub const fn direction(self) -> TileDirection {
        match self {
            Self::Move(d) | Self::Grow(d) | Self::Shrink(d) => d,
        }
    }

    /// The nudge that undoes this one **for the card, not for the board**.
    ///
    /// The distinction is load-bearing and is why [`TileEdit`] exists rather
    /// than an inverse-gesture stack. `Grow(Down)` then `Shrink(Down)` returns
    /// the card to the exact rectangle it had — and the cards it pushed on the
    /// way down **stay** pushed, because the reflow only ever moves tiles
    /// downward and floating them back up is [`TileGrid::compact`]'s job, a
    /// separate verb. `Move` is weaker still: an inverse `Move` is not even
    /// exact for the card once a bound has clamped it.
    ///
    /// So this is the right thing for building a keymap out of
    /// [`TileDirection::ALL`], and the wrong thing to build an undo out of.
    #[must_use]
    pub const fn inverse(self) -> Self {
        match self {
            Self::Move(d) => Self::Move(d.opposite()),
            Self::Grow(d) => Self::Shrink(d),
            Self::Shrink(d) => Self::Grow(d),
        }
    }
}

/// What a move or a resize did to the tiles it landed on.
///
/// The dashboard tool reflows silently; this is the record of it. Empty means
/// the gesture fit where it was put.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reflow {
    displaced: Vec<Displaced>,
}

impl Reflow {
    /// Every tile the gesture pushed, in the order they were resolved.
    #[must_use]
    pub fn displaced(&self) -> &[Displaced] {
        &self.displaced
    }

    /// Whether anything had to move out of the way.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.displaced.is_empty()
    }

    /// (R1609) What a whole *session* of edits did to the other tiles, as the
    /// difference between two arrangements.
    ///
    /// ★ **A session's reflow is a difference and not a sum**, and the two are
    /// genuinely different answers. Five arrow presses that walk a card down
    /// past another one push that other card from row 1 to row 5; adding the
    /// per-press reflows says `x:1>2, x:2>3, x:3>4, x:4>5` — four displacements
    /// of a card that moved once, and an undo record built from that list has to
    /// know to collapse it. Comparing the arrangements says `x:1>5`.
    ///
    /// `edited` is excluded because being *displaced* means being pushed by
    /// somebody else; the card the user is holding moved because they moved it.
    /// Order follows `before`'s tile order, so the answer is stable across calls
    /// rather than depending on which cards happened to settle first.
    ///
    /// Only rows can differ: the reflow moves tiles **down** and nothing else,
    /// so a column change belongs to whoever was edited.
    #[must_use]
    pub fn between(before: &TileGrid, after: &TileGrid, edited: &TileId) -> Self {
        let displaced = before
            .tiles()
            .iter()
            .filter(|tile| &tile.id != edited)
            .filter_map(|tile| {
                let now = after.tile(&tile.id)?;
                (now.row != tile.row).then(|| Displaced {
                    id: tile.id.clone(),
                    from: tile.row,
                    to: now.row,
                })
            })
            .collect();
        Self { displaced }
    }
}

/// (R1609) An edit in progress: which card, and the arrangement to go back to.
///
/// **The undo point has to be the whole board, not the card's rectangle.** A
/// move displaces other cards, so restoring only the one being dragged would
/// leave the board rearranged around a card that had returned home. The
/// toolkit makes exactly this mistake in miniature: `_q_enterInteractiveMode` stores `oldGeometry = q->geometry()` and then no
/// path ever reads it back — `Key_Escape`, `Key_Return` and `Key_Enter` share one `leaveInteractiveMode()` arm, so a keyboard
/// move in the toolkit cannot be abandoned at all.
///
/// The session deliberately does **not** hold the live arrangement. R1608's
/// design point was that the painter, the wire and the assistive-technology
/// tree read *one* `TileGrid`; a session carrying its own copy would make two that
/// could disagree about what is on screen. So the caller keeps editing its
/// grid and this holds only what to restore, plus the derivations that need
/// both. Serializable for the same reason [`TileGrid`] is, and the framework asks for
/// it directly: a session held in a `Signal` must round-trip, so an in-flight edit
/// survives a persisted session and <kbd>Escape</kbd> still knows what to
/// restore. The toolkit's `oldGeometry` is a private rect member that dies with the
/// widget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileEdit {
    id: TileId,
    before: TileGrid,
}

impl TileEdit {
    /// Open a session on a tile.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`] — a session on a card that is not there would
    /// be an undo point for nothing.
    pub fn begin(grid: &TileGrid, id: &TileId) -> Result<Self, TileError> {
        if grid.tile(id).is_none() {
            return Err(TileError::NoSuchTile(id.clone()));
        }
        Ok(Self {
            id: id.clone(),
            before: grid.clone(),
        })
    }

    /// Which card the session is editing.
    #[must_use]
    pub const fn id(&self) -> &TileId {
        &self.id
    }

    /// The arrangement as it was when the session opened.
    #[must_use]
    pub const fn before(&self) -> &TileGrid {
        &self.before
    }

    /// What the session has displaced so far, against the arrangement it opened
    /// on — see [`Reflow::between`].
    #[must_use]
    pub fn reflow(&self, now: &TileGrid) -> Reflow {
        Reflow::between(&self.before, now, &self.id)
    }

    /// Whether anything at all has changed since the session opened.
    ///
    /// The question a bound makes necessary: a held arrow key at column zero
    /// leaves the arrangement **equal**, and an announcement or an undo entry
    /// for an edit that did nothing is noise. The toolkit asks the same
    /// question the same way and only about the one widget — `keyPressEvent` compares `currentGeometry == oldGeometry`
    /// and returns early — which is cheap here because the arrangement is a
    /// value.
    #[must_use]
    pub fn changed(&self, now: &TileGrid) -> bool {
        &self.before != now
    }

    /// Abandon the session: the arrangement as it was, other cards included.
    #[must_use]
    pub fn cancel(self) -> TileGrid {
        self.before
    }
}

/// One tile pushed out of the way, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Displaced {
    /// Which tile.
    pub id: TileId,
    /// The row it was on.
    pub from: u32,
    /// The row it ended on.
    pub to: u32,
}

/// ★ (R1900) What a [`TileGrid::share`] did: the cell two things now share,
/// who is in it, and the cell the joining one left behind.
///
/// `vacated` is [`Some`] exactly when the joining occupant was alone where it
/// came from, so its cell ceased to exist. A caller undoing the gesture needs
/// that rectangle and cannot re-derive it — which is why it is handed back
/// rather than left for the caller to have snapshotted first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shared {
    /// The cell's name now — which is the occupant that just joined, since a
    /// join comes to the front.
    pub place: TileId,
    /// Everyone sharing that cell, in strip order.
    pub members: Vec<TileId>,
    /// The cell the joining occupant left empty, when it had one to itself.
    pub vacated: Option<Tile>,
}

/// Why an edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TileError {
    /// No tile with that id.
    NoSuchTile(TileId),
    /// A tile with that id is already here — an id is an identity, so adding a
    /// second is a caller's mistake rather than a second tile.
    Duplicate(TileId),
    /// A tile wider than the grid. Named rather than clamped: a card that does
    /// not fit is a layout the caller did not mean, and silently narrowing it
    /// would hide the arithmetic that produced it.
    TooWide {
        /// The width asked for.
        w: u32,
        /// How many columns the grid has.
        columns: u32,
    },
    /// A tile with no area. A zero-cell card is invisible and un-clickable, so
    /// it is refused rather than placed where nobody can find it.
    Empty {
        /// The width asked for.
        w: u32,
        /// The height asked for.
        h: u32,
    },
    /// (R1900) The occupants of a cell refused the change — see
    /// [`StackRefusal`] for which of the three it was.
    ///
    /// Wrapped rather than restated, so the sentence a person reads is the
    /// stacking module's own and this enum does not become a second place where
    /// "the last occupant cannot leave" is spelled.
    Stacking(StackRefusal),
    /// (R1900) A cell was asked to take an occupant into itself.
    ///
    /// Not a [`Self::Stacking`], because the stack never saw the request: the
    /// two ids resolve to the same cell, so there is no second place for one of
    /// them to come from.
    SelfShare {
        /// The id that was on both ends of the request.
        id: TileId,
    },
}

impl From<StackRefusal> for TileError {
    fn from(refusal: StackRefusal) -> Self {
        Self::Stacking(refusal)
    }
}

impl std::fmt::Display for TileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchTile(id) => write!(f, "no tile {id} in this grid"),
            Self::Duplicate(id) => write!(f, "tile {id} is already in this grid"),
            Self::TooWide { w, columns } => {
                write!(
                    f,
                    "a tile {w} columns wide does not fit a grid of {columns}"
                )
            }
            Self::Empty { w, h } => write!(f, "a tile of {w}x{h} cells has no area"),
            Self::Stacking(refusal) => f.write_str(refusal.reason().as_str()),
            Self::SelfShare { id } => {
                write!(f, "{id} is already the cell it was asked to share")
            }
        }
    }
}

impl std::error::Error for TileError {}

/// (R1648) The way back from [`TileGrid::maximize`]: which tile was maximised,
/// and the arrangement the board had before it was.
///
/// `#[must_use]` because dropping it is exactly the bug this type exists to
/// make visible — a maximise whose restore was discarded leaves a board the
/// user cannot get their layout back from, and it looks like a working maximise
/// until they try.
///
/// It is serialisable for the same reason the arrangement is: a session that
/// was saved while a card was maximised must reopen with the way home intact.
#[must_use = "dropping this loses the arrangement the board had before it was maximised"]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Maximized {
    id: TileId,
    restore: TileGrid,
}

impl Maximized {
    /// Which tile is filling the board.
    #[must_use]
    pub const fn id(&self) -> &TileId {
        &self.id
    }

    /// The arrangement to go back to.
    ///
    /// Consumes the token, so a restore happens once — a second one would put
    /// back an arrangement that is two edits old.
    #[must_use]
    pub fn restore(self) -> TileGrid {
        self.restore
    }

    /// The arrangement to go back to, without consuming the token.
    ///
    /// For a shell that wants to *show* what un-maximising will return to (a
    /// preview, a tooltip, the wire) without performing it.
    #[must_use]
    pub const fn peek(&self) -> &TileGrid {
        &self.restore
    }
}

/// A dashboard's arrangement: tiles on a fixed number of columns, none of them
/// overlapping.
///
/// Rows are unbounded — a dashboard scrolls — so only the column count is a
/// declaration and the height is whatever the tiles need
/// ([`TileGrid::rows`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileGrid {
    columns: u32,
    tiles: Vec<Tile>,
}

impl TileGrid {
    /// An empty grid of `columns` columns, at least one.
    #[must_use]
    pub const fn new(columns: u32) -> Self {
        Self {
            columns: if columns == 0 { 1 } else { columns },
            tiles: Vec::new(),
        }
    }

    /// How many columns the grid has.
    #[must_use]
    pub const fn columns(&self) -> u32 {
        self.columns
    }

    /// The tiles, in placement order.
    #[must_use]
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// How many rows the tiles reach — the grid's derived height.
    #[must_use]
    pub fn rows(&self) -> u32 {
        self.tiles.iter().map(Tile::bottom).max().unwrap_or(0)
    }

    /// The tile that **holds** that id — the cell it occupies, whether it is
    /// the cell's sole occupant or one of several sharing it.
    ///
    /// ★ R1900 — this used to compare `t.id == id`, which was the same question
    /// while a cell could hold one card. It is now membership, so a shell that
    /// asks "where is this card" gets an answer that survives the card being
    /// stacked with another. Every lookup in this module goes through one
    /// private index-of helper for the same reason.
    #[must_use]
    pub fn tile(&self, id: &TileId) -> Option<&Tile> {
        self.index_of(id).map(|at| &self.tiles[at])
    }

    /// Where in `tiles` the cell holding `id` is.
    fn index_of(&self, id: &TileId) -> Option<usize> {
        self.tiles.iter().position(|t| t.holds(id))
    }

    /// Whether any cell of the grid is free of tiles in the given rectangle.
    #[must_use]
    pub fn is_free(&self, col: u32, row: u32, w: u32, h: u32) -> bool {
        let probe = Tile::new("", col, row, w.max(1), h.max(1));
        !self.tiles.iter().any(|t| t.overlaps(&probe))
    }

    /// Add a tile, pushing whatever it lands on downward.
    ///
    /// # Errors
    ///
    /// [`TileError::Duplicate`], [`TileError::TooWide`] or [`TileError::Empty`].
    pub fn place(&mut self, tile: Tile) -> Result<Reflow, TileError> {
        if tile.w == 0 || tile.h == 0 {
            return Err(TileError::Empty {
                w: tile.w,
                h: tile.h,
            });
        }
        if tile.w > self.columns {
            return Err(TileError::TooWide {
                w: tile.w,
                columns: self.columns,
            });
        }
        if self.tile(&tile.id).is_some() {
            return Err(TileError::Duplicate(tile.id));
        }
        let mut tile = tile;
        // R1733 — the same clamp `landing` and `landing_for` apply, called
        // rather than repeated. It had been written out here, in `landing`, and
        // a third time in every shell drawing a preview.
        tile.col = self.landing_for(tile.w, tile.col, tile.row).0;
        let id = tile.id.clone();
        self.tiles.push(tile);
        Ok(self.reflow_around(&id))
    }

    /// Move a tile to `(col, row)`, pushing whatever it lands on downward.
    ///
    /// The column is clamped so the tile stays inside the grid — a drag past
    /// the right edge is a gesture, not an error, and stopping at the edge is
    /// what every dashboard does.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`].
    pub fn move_to(&mut self, id: &TileId, col: u32, row: u32) -> Result<Reflow, TileError> {
        let (col, row) = self
            .landing(id, col, row)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        let at = self
            .index_of(id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        let target = &mut self.tiles[at];
        target.col = col;
        target.row = row;
        Ok(self.reflow_around(id))
    }

    /// (R1668) Where a tile asked to go to `(col, row)` would actually land,
    /// without moving it — the cell a drag preview must draw.
    ///
    /// [`None`] when the grid holds no such tile.
    ///
    /// ## Why this exists
    ///
    /// A tile cannot start further right than `columns - w`, or it would run
    /// off the board. [`move_to`](Self::move_to) has always applied that, and a
    /// shell drawing a drag preview applied its own rule — one fact with two
    /// clamps, the shape R1654 named. Measured: dragging a six-column card to
    /// column seven of a twelve-column board previewed column seven and
    /// committed column six, so the preview was a promise the release broke.
    ///
    /// `move_to` is now written in terms of this, so a preview that asks and a
    /// release that acts cannot disagree.
    #[must_use]
    pub fn landing(&self, id: &TileId, col: u32, row: u32) -> Option<(u32, u32)> {
        let target = self.tile(id)?;
        Some(self.landing_for(target.w, col, row))
    }

    /// (R1733) Where a tile `w` columns wide asked for `(col, row)` would
    /// start — **whether or not it is on the board yet**.
    ///
    /// [`landing`](Self::landing) is this asked about a tile the grid already
    /// holds. This is the same question for a footprint that is still on a
    /// palette, which is the one a drop preview needs and the one that had no
    /// answer: a shell drawing "where this new card would go" had to write the
    /// clamp itself, which is the two-clamps-one-fact shape R1668 measured on
    /// the *other* drag and repaired only there.
    ///
    /// Height takes no part. Rows are unbounded ([`rows`](Self::rows) is
    /// derived), so nothing about `h` can move where a tile starts — and a
    /// parameter that could not change the answer would be a promise this
    /// cannot keep.
    ///
    /// The column stops at `columns - w` so the tile stays on the board; a drag
    /// past the right edge is a gesture, not an error. A `w` of zero is
    /// treated as one, because a zero-cell tile is refused at
    /// [`place`](Self::place) and answering a landing for it here would let a
    /// preview be drawn for something that can never be placed.
    #[must_use]
    pub fn landing_for(&self, w: u32, col: u32, row: u32) -> (u32, u32) {
        (col.min(self.columns.saturating_sub(w.max(1))), row)
    }

    /// Resize a tile, pushing whatever it grows into downward.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`], [`TileError::TooWide`] or [`TileError::Empty`].
    pub fn resize(&mut self, id: &TileId, w: u32, h: u32) -> Result<Reflow, TileError> {
        if w == 0 || h == 0 {
            return Err(TileError::Empty { w, h });
        }
        if w > self.columns {
            return Err(TileError::TooWide {
                w,
                columns: self.columns,
            });
        }
        let columns = self.columns;
        let at = self
            .index_of(id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        let target = &mut self.tiles[at];
        target.w = w;
        target.h = h;
        target.col = target.col.min(columns - w);
        Ok(self.reflow_around(id))
    }

    /// (R1609) Drag one of a tile's eight handles to a cell, pushing whatever
    /// the result grows into downward.
    ///
    /// The cell is the one the pointer is over, and each edge reads it the way
    /// its own half-open line does: a left or top edge lands *on* the cell, a
    /// right or bottom edge lands one past it, so the dragged side always ends up
    /// covering the cell under the cursor.
    ///
    /// ★ **A corner moves both its edges before anything reflows, and that is a
    /// decision rather than an implementation detail.** Resolving the horizontal
    /// edge, reflowing, then resolving the vertical one would let an
    /// *intermediate* rectangle — one the user never asked for and never sees —
    /// displace cards the final rectangle does not touch, and those cards do not
    /// come back, because the reflow only ever pushes down. One gesture is one
    /// reflow.
    ///
    /// Bounds are **clamped**, not refused: a drag past the board's edge is a
    /// gesture rather than a mistake, and a side dragged past its opposite stops
    /// one cell short so a card never inverts or vanishes. That is
    /// [`Self::move_to`]'s existing rule, applied to sizes.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`].
    pub fn drag_handle(
        &mut self,
        id: &TileId,
        handle: TileHandle,
        col: u32,
        row: u32,
    ) -> Result<Reflow, TileError> {
        let columns = self.columns;
        let at = self
            .index_of(id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        let target = &mut self.tiles[at];
        for edge in [handle.horizontal(), handle.vertical()]
            .into_iter()
            .flatten()
        {
            let line = match edge {
                TileEdge::Left => col,
                TileEdge::Right => col.saturating_add(1),
                TileEdge::Top => row,
                TileEdge::Bottom => row.saturating_add(1),
            };
            set_edge(target, edge, line, columns);
        }
        Ok(self.reflow_around(id))
    }

    /// (R1609) Apply a one-cell keyboard edit.
    ///
    /// The whole keyboard channel: twelve values, no mode, and every one of
    /// them either the existing [`Self::move_to`] or the same `set_edge` a handle drag runs. A
    /// nudge that has nowhere to go leaves the arrangement **equal** — `TileGrid` is
    /// `PartialEq`, so "did that do anything" is one comparison ([`TileEdit::changed`]) rather than a
    /// second return channel, and a held arrow key at a bound stops instead of
    /// erroring (R1549's rule, where abstract spin box keeps its repeat timer
    /// running against a pinned value).
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`].
    pub fn nudge(&mut self, id: &TileId, nudge: TileNudge) -> Result<Reflow, TileError> {
        if let TileNudge::Move(direction) = nudge {
            let tile = self
                .tile(id)
                .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
            let (col, row) = match direction {
                TileDirection::Left => (tile.col.saturating_sub(1), tile.row),
                TileDirection::Right => (tile.col.saturating_add(1), tile.row),
                TileDirection::Up => (tile.col, tile.row.saturating_sub(1)),
                TileDirection::Down => (tile.col, tile.row.saturating_add(1)),
            };
            // Through `move_to`, so the right-edge clamp lives in one place.
            return self.move_to(id, col, row);
        }
        let columns = self.columns;
        let at = self
            .index_of(id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        let target = &mut self.tiles[at];
        let edge = nudge.direction().edge();
        // Growing a start edge lowers its line; growing an end edge raises it.
        // `TileEdge::is_start` is the only asymmetry the resize needs.
        let outward = matches!(nudge, TileNudge::Grow(_));
        let line = edge.line_of(target);
        let line = if edge.is_start() == outward {
            line.saturating_sub(1)
        } else {
            line.saturating_add(1)
        };
        set_edge(target, edge, line, columns);
        Ok(self.reflow_around(id))
    }

    /// (R1609) The tile an arrow key in `dir` moves the selection to.
    ///
    /// Spatial, not ordinal: the nearest tile lying wholly beyond that edge
    /// ([`Tile::lies_beyond`]), preferring one whose band on the *other* axis
    /// overlaps this tile's, then the least distance, then the nearer on the
    /// cross axis, then top-to-bottom-left-to-right — a total order, so the
    /// answer never depends on the order cards were added.
    ///
    /// The band preference is what makes it feel like a grid: from the left card
    /// of a row, Right goes to its neighbour in that row and not to a card three
    /// rows down that happens to start one column sooner. The fallback past the
    /// band is what keeps navigation **total** — see [`Tile::lies_beyond`] for why
    /// the non-overlap invariant guarantees every card is reachable.
    ///
    /// MDI area offers `activateNextSubWindow` / `activatePreviousSubWindow`
    /// over a list, so it has no notion of *direction* at all.
    #[must_use]
    pub fn neighbour(&self, id: &TileId, dir: TileDirection) -> Option<&Tile> {
        let from = self.tile(id)?;
        self.tiles
            .iter()
            .filter(|tile| tile.id != from.id && tile.lies_beyond(from, dir))
            .min_by_key(|tile| {
                let (band, gap, cross) = if dir.edge().is_horizontal() {
                    let gap = match dir {
                        TileDirection::Left => from.col - tile.right(),
                        _ => tile.col - from.right(),
                    };
                    (!tile.shares_rows(from), gap, tile.row.abs_diff(from.row))
                } else {
                    let gap = match dir {
                        TileDirection::Up => from.row - tile.bottom(),
                        _ => tile.row - from.bottom(),
                    };
                    (!tile.shares_columns(from), gap, tile.col.abs_diff(from.col))
                };
                (band, gap, cross, tile.row, tile.col)
            })
    }

    /// Take a tile out. The gap it leaves stays a gap until [`Self::compact`].
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`].
    pub fn remove(&mut self, id: &TileId) -> Result<Tile, TileError> {
        let at = self
            .index_of(id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        Ok(self.tiles.remove(at))
    }

    /// ★★★★★ (R1900) Put `member` into the cell that holds `into`, so the two
    /// **share one cell** and a strip chooses between them.
    ///
    /// The joining occupant comes to the front, because a person who just
    /// dropped it there is looking for it. If it was the sole occupant of its
    /// own cell, that cell is vacated — [`Shared::vacated`] hands it back, so a
    /// caller can undo the gesture or animate it without re-deriving a
    /// rectangle that no longer exists.
    ///
    /// Nothing reflows. A vacated cell leaves a hole, exactly as
    /// [`remove`](Self::remove) does, and closing it is [`compact`](Self::compact)'s
    /// job — the R1607 rule that compaction is a verb rather than a
    /// consequence.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`] when either id is on no cell;
    /// [`TileError::SelfShare`] when they are already the same cell;
    /// [`TileError::Stacking`] for a refusal the occupants themselves make.
    ///
    /// # Examples
    ///
    /// ```
    /// use pinion_core::widgets::tile_grid::{Tile, TileGrid, TileId};
    ///
    /// let mut board = TileGrid::new(12);
    /// board.place(Tile::new("packet", 0, 0, 6, 2)).expect("an empty board");
    /// board.place(Tile::new("share", 6, 0, 6, 2)).expect("beside it");
    ///
    /// let joined = board
    ///     .share(&TileId::new("share"), &TileId::new("packet"))
    ///     .expect("two cells that exist");
    /// assert_eq!(joined.members, ["packet".into(), "share".into()]);
    /// assert!(joined.vacated.is_some(), "its own cell is now free");
    ///
    /// // One cell, holding both, showing the one just dropped in.
    /// assert_eq!(board.tiles().len(), 1);
    /// let cell = board.tile(&TileId::new("packet")).expect("still findable");
    /// assert!(cell.is_shared());
    /// assert_eq!(cell.id, TileId::new("share"), "the front is what a reader sees");
    /// ```
    pub fn share(&mut self, member: &TileId, into: &TileId) -> Result<Shared, TileError> {
        let from = self
            .index_of(member)
            .ok_or_else(|| TileError::NoSuchTile(member.clone()))?;
        let host = self
            .index_of(into)
            .ok_or_else(|| TileError::NoSuchTile(into.clone()))?;
        if from == host {
            return Err(TileError::SelfShare { id: member.clone() });
        }
        // Take it out of its own cell first, so a refusal there leaves both
        // cells untouched: a `join` that succeeded and a `part` that then
        // refused would be the one state this pair must not reach.
        let (vacated, host) = if self.tiles[from].stack.is_shared() {
            self.tiles[from].stack.part(member)?;
            self.resync(from);
            (None, host)
        } else {
            let cell = self.tiles.remove(from);
            // ★ The shift is ARITHMETIC rather than a second lookup. Asking
            // `index_of` again would be a search that cannot fail and therefore
            // an `expect` — a panic documented as impossible, which is the
            // shape that survives until the day the invariant moves. Removing
            // an earlier element shifts a later one by exactly one; `from` and
            // `host` differ (the equal case returned above), so this is total.
            (Some(cell), if from < host { host - 1 } else { host })
        };
        self.tiles[host].stack.join(member.clone())?;
        self.resync(host);
        Ok(Shared {
            place: self.tiles[host].id.clone(),
            members: self.tiles[host].members().to_vec(),
            vacated,
        })
    }

    /// ★ (R1900) Take `member` back out of the cell it shares and give it a
    /// cell of its own at `(col, row)`, the same size as the one it left.
    ///
    /// The inverse of [`share`](Self::share), and it refuses the same way a
    /// stack does: an occupant that is already alone in its cell has nothing to
    /// come out of, and the refusal names the gesture that moves the cell
    /// itself.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`]; [`TileError::Stacking`] with
    /// [`StackRefusal::Sole`] when it is the cell's only occupant.
    pub fn unshare(&mut self, member: &TileId, col: u32, row: u32) -> Result<Reflow, TileError> {
        let at = self
            .index_of(member)
            .ok_or_else(|| TileError::NoSuchTile(member.clone()))?;
        let (w, h) = (self.tiles[at].w, self.tiles[at].h);
        self.tiles[at].stack.part(member)?;
        self.resync(at);
        self.place(Tile::new(member.as_str(), col, row, w, h))
    }

    /// (R1900) Bring `member` to the front of the cell it shares.
    ///
    /// What a press on a tab does, and the only way the front changes — so a
    /// strip's highlight and the card a reader sees cannot come from two
    /// different decisions.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`] when it is on no cell.
    pub fn reveal(&mut self, member: &TileId) -> Result<Revealed<TileId>, TileError> {
        let at = self
            .index_of(member)
            .ok_or_else(|| TileError::NoSuchTile(member.clone()))?;
        let moved = self.tiles[at].stack.reveal(member)?;
        self.resync(at);
        Ok(moved)
    }

    /// (R1900) Take `member` out of whatever cell holds it, without placing it
    /// anywhere — for a gesture that carries it off the board entirely.
    ///
    /// When it was the cell's sole occupant the cell goes with it, which is
    /// [`remove`](Self::remove)'s behaviour; when it was sharing, the cell stays
    /// for the others. The returned tile carries the rectangle it had, so a
    /// caller can give it that size wherever it is going.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`].
    pub fn lift(&mut self, member: &TileId) -> Result<Tile, TileError> {
        let at = self
            .index_of(member)
            .ok_or_else(|| TileError::NoSuchTile(member.clone()))?;
        if !self.tiles[at].stack.is_shared() {
            return Ok(self.tiles.remove(at));
        }
        let cell = self.tiles[at].clone();
        // `?` rather than an `expect` that argues the refusal is unreachable:
        // it is unreachable *today*, and a returned refusal names the defect
        // where a panic would only end the process.
        self.tiles[at].stack.part(member)?;
        self.resync(at);
        Ok(Tile::new(
            member.as_str(),
            cell.col,
            cell.row,
            cell.w,
            cell.h,
        ))
    }

    /// Restore the invariant this module keeps: a cell's `id` **is** its
    /// stack's front.
    ///
    /// One private call after every stack mutation, rather than the assignment
    /// written out at each of them — which is how the two would come to
    /// disagree in exactly the one path nobody re-read.
    fn resync(&mut self, at: usize) {
        self.tiles[at].id = self.tiles[at].stack.fore().clone();
    }

    /// Float every tile as far up as it will go, closing gaps.
    ///
    /// **A verb rather than a consequence.** the dashboard tool does this
    /// after every drag, so a gesture moves tiles the user did not touch and
    /// its inverse is not a gesture; here an editor chooses whether tidying is
    /// part of the drag, and the tiles that moved are named either way.
    pub fn compact(&mut self) -> Reflow {
        let mut order: Vec<usize> = (0..self.tiles.len()).collect();
        order.sort_by_key(|&i| (self.tiles[i].row, self.tiles[i].col));
        let mut displaced = Vec::new();
        for &index in &order {
            let from = self.tiles[index].row;
            let mut row = from;
            while row > 0 {
                let candidate = Tile {
                    row: row - 1,
                    ..self.tiles[index].clone()
                };
                if self
                    .tiles
                    .iter()
                    .enumerate()
                    .any(|(other, t)| other != index && t.overlaps(&candidate))
                {
                    break;
                }
                row -= 1;
            }
            if row != from {
                self.tiles[index].row = row;
                displaced.push(Displaced {
                    id: self.tiles[index].id.clone(),
                    from,
                    to: row,
                });
            }
        }
        Reflow { displaced }
    }

    /// (R1648) Fill the board with one tile, and hand back the way home.
    ///
    /// Every dashboard has this and every one of them implements it the same
    /// wrong way: keep a copy of the arrangement somewhere on the side, swap
    /// the board for a single full-width tile, and hope the copy is still
    /// around when the user un-maximises. The copy is the part that gets lost —
    /// a second maximise overwrites it, a preset load replaces it, a panic
    /// drops it — and what the user loses is a layout they arranged by hand.
    ///
    /// So the way home is not a copy the caller keeps: it is the **return
    /// value**, it is `#[must_use]`, and it is the only thing
    /// [`Maximized::restore`] accepts. Dropping it is still possible — this is
    /// Rust, not a linear type system — but it cannot happen silently, and
    /// maximising twice cannot clobber the first token because the second call
    /// starts from a board that already has one tile.
    ///
    /// The tiles that are not `id` are **removed**, not hidden: a hidden tile
    /// is a second visibility model beside the arrangement, and the two would
    /// have to be kept in agreement by whoever walks the board.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`].
    pub fn maximize(&mut self, id: &TileId) -> Result<Maximized, TileError> {
        let tile = self
            .tile(id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?
            .clone();
        let restore = self.clone();
        // The full board: every column, and as many rows as the arrangement
        // reached — so a maximised card is the size of what it replaced rather
        // than an arbitrary height, and un-maximising does not resize the
        // scroll extent under the pointer.
        let height = self.rows().max(tile.h).max(1);
        self.tiles = vec![Tile::new(id.as_str(), 0, 0, self.columns, height)];
        Ok(Maximized {
            id: id.clone(),
            restore,
        })
    }

    /// (R1648) Whether the board is showing one tile because of
    /// [`Self::maximize`].
    ///
    /// A board with exactly one tile spanning every column is indistinguishable
    /// from a board somebody arranged that way, which is why this is a shape
    /// question and not a state flag: the authority on "is maximised" is
    /// whether a [`Maximized`] token exists, and this only reports the shape.
    #[must_use]
    pub fn is_filled_by_one(&self) -> bool {
        matches!(self.tiles.as_slice(), [only] if only.col == 0 && only.w == self.columns)
    }

    /// The CSS grid placement of a tile — `(column, row)`.
    ///
    /// **The one place zero-based model coordinates become CSS's one-based
    /// lines.** Doing it here rather than at each call site is what keeps a
    /// painter, an assistive-technology tree and the wire from each adding one
    /// separately, which is the defect R1560 already recorded for a text
    /// table's cell address.
    #[must_use]
    pub fn placement(&self, id: &TileId) -> Option<(GridPlacement, GridPlacement)> {
        let tile = self.tile(id)?;
        Some((
            GridPlacement::spanning(line(tile.col), span(tile.w)),
            GridPlacement::spanning(line(tile.row), span(tile.h)),
        ))
    }

    /// Whether the arrangement holds its invariant.
    ///
    /// Every operation maintains it, so this is for a grid that arrived from a
    /// file or a peer and has promised nothing — the same reason
    /// `Document::validate` exists in the node graph.
    #[must_use]
    pub fn violations(&self) -> Vec<(TileId, TileId)> {
        let mut found = Vec::new();
        for (i, a) in self.tiles.iter().enumerate() {
            if a.w == 0 || a.h == 0 || a.right() > self.columns {
                found.push((a.id.clone(), a.id.clone()));
            }
            for b in &self.tiles[i + 1..] {
                if a.overlaps(b) {
                    found.push((a.id.clone(), b.id.clone()));
                }
            }
        }
        found
    }

    /// Push everything that overlaps `anchor` downward, and everything that
    /// then overlaps, until nothing does.
    ///
    /// Deterministic and terminating, and both properties come from the same
    /// rule: a displaced tile only ever moves **down**, and it is placed just
    /// below the lowest tile it collided with. Rows are monotonically
    /// increasing and the tile set is finite, so the walk cannot cycle; the
    /// `settle` bound is a backstop for a caller that handed us a grid already
    /// in violation, not part of the argument.
    fn reflow_around(&mut self, anchor: &TileId) -> Reflow {
        let mut displaced: Vec<Displaced> = Vec::new();
        let mut pinned: Vec<usize> = self
            .tiles
            .iter()
            .position(|t| &t.id == anchor)
            .into_iter()
            .collect();
        // Everyone else, nearest the top first: a tile that was already above
        // the anchor keeps its place if it can, which is what makes a drag feel
        // local rather than re-shuffling the whole board.
        let mut rest: Vec<usize> = (0..self.tiles.len())
            .filter(|i| !pinned.contains(i))
            .collect();
        rest.sort_by_key(|&i| (self.tiles[i].row, self.tiles[i].col));

        for index in rest {
            let from = self.tiles[index].row;
            for _settle in 0..=self.tiles.len() {
                let lowest = pinned
                    .iter()
                    .filter(|&&other| self.tiles[other].overlaps(&self.tiles[index]))
                    .map(|&other| self.tiles[other].bottom())
                    .max();
                match lowest {
                    Some(bottom) => self.tiles[index].row = bottom,
                    None => break,
                }
            }
            if self.tiles[index].row != from {
                displaced.push(Displaced {
                    id: self.tiles[index].id.clone(),
                    from,
                    to: self.tiles[index].row,
                });
            }
            pinned.push(index);
        }
        Reflow { displaced }
    }
}

/// ★★★★★ (R1733) What a board drag is carrying.
///
/// A board has one drag in flight, and it is **either** moving something
/// already on the board **or** placing something that is not on it yet. Two
/// arms rather than two fields, because two nullable fields can be set at once
/// and that state has no meaning.
///
/// The behaviour reference spells it the other way — a held card id and a held
/// palette kind, each nullable — so every handler has to remember to check the
/// other one before acting. ★ Measured in that prototype, and it had already
/// decayed: the held-card field appears **three** times and **two** of those
/// are guards reading it, while **nothing in the whole script ever assigns it a
/// non-null value** — the reorder gesture was moved onto another field and the
/// guards it left behind were never removed. That is what a pair of nullable
/// fields costs in practice, ahead of the forgotten-check it also invites.
///
/// Here the check is a `match`, the compiler performs it, and a guard on a case
/// that cannot arise does not compile into silence — it is an arm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Carried {
    /// A tile the board already holds, gripped `dx` columns and `dy` rows
    /// inside its own rectangle — so a card grabbed by its middle keeps that
    /// grip instead of jumping its top-left corner under the pointer.
    Placed {
        /// Which tile.
        id: TileId,
        /// Columns between the tile's left edge and the grip.
        dx: u32,
        /// Rows between the tile's top edge and the grip.
        dy: u32,
    },
    /// A footprint that is not on the board — a palette entry. Its id is the
    /// one a drop would place it under, decided when the drag opens so an
    /// abandoned drag consumes nothing.
    Fresh {
        /// The id a drop would place it under.
        id: TileId,
        /// Width in columns.
        w: u32,
        /// Height in rows.
        h: u32,
    },
}

impl Carried {
    /// Whose tile this drag would land — the id already on the board, or the
    /// one a fresh footprint would take.
    #[must_use]
    pub const fn id(&self) -> &TileId {
        match self {
            Self::Placed { id, .. } | Self::Fresh { id, .. } => id,
        }
    }

    /// Whether the board already holds what is being carried.
    #[must_use]
    pub const fn is_placed(&self) -> bool {
        matches!(self, Self::Placed { .. })
    }
}

/// (R1733) What a [`TileDrag::drop_on`] did.
///
/// Three arms, because a release has three outcomes a caller must tell apart
/// and only one of them is a change. R1701 measured what happens when the
/// middle arm is left to a remembered comparison: a click that carried nothing
/// reflowed the board and announced a move that had not happened.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Dropped {
    /// The board took it, at that cell, displacing whatever the reflow names.
    Landed {
        /// Where it went — the cell the preview was drawing.
        at: (u32, u32),
        /// What had to move out of the way.
        reflow: Reflow,
    },
    /// The landing is where the tile already was: a press and a release that
    /// carried it nowhere. The board is untouched, and saying "moved" here is
    /// the lie this arm exists to make unspellable.
    Unmoved,
    /// The pointer was not over the board when the button came up. Nothing
    /// happened, and nothing was wrong — which is a different answer from a
    /// drop the board refused.
    Abandoned,
}

/// ★★★★★ (R1733) A board drag in flight: what is being carried, and the cell a
/// release would put it in.
///
/// # The one fact the preview and the commit share
///
/// [`preview`](Self::preview) is what a painter draws and
/// [`drop_on`](Self::drop_on) is what a release does, and both read the same
/// stored landing — so they cannot disagree. R1668 measured the failure this
/// forecloses on the move gesture (a six-column card dragged to column seven
/// previewed seven and committed six, because a shell had written the clamp and
/// the grid had written it too), repaired it for tiles already on the board,
/// and left the same shape open for anything arriving from a palette.
///
/// # Against the reference toolkit at 6.11.1
///
/// Measured by building a probe against it and running it, not by reading about
/// it. Its grid container, the layout base, its widget class and its item view
/// were enumerated through the runtime meta-object, and a drag was driven onto
/// a real drop target.
///
/// | question | there | here |
/// |---|---|---|
/// | any member answering "where would a `w`-wide item land at this cell" | **0** across all four classes. The one name that matches at all is a *bool* on the item view saying whether to draw an indicator | [`TileGrid::landing_for`] |
/// | asking for an occupied cell | both items get geometry and **overlap**; the add call returns `void`, so there is nothing to refuse with, and the position query answers only the first | [`TileError`], and a reflow that names what it displaced |
/// | what a drag-move event carries | a **pixel**. Nothing on the event, the widget or the layout turns it into the cell a release would use, so the highlight and the commit are two computations | one landing, stored, read by both |
/// | a release that carried nothing | the target's drop handler runs regardless; telling a click from a drag is the application's | [`Dropped::Unmoved`] |
/// | what a surface accepts, asked **before** the drag | one bool on the widget. A part can say yes or no and cannot say *what*; the kinds are declared once for the whole model, outside the meta-object, and the refusal is a bare bool with no reason | ★the floor is **above** us on the neighbouring axis — see below |
///
/// The last row is honest rather than favourable: a target there accepts a
/// payload from a source that has never heard of it, negotiated by format, and
/// pinion's drag session dispatches only to the source. What the floor cannot
/// do is answer *which* kinds, per part, without running a drag.
///
/// # Examples
///
/// ```
/// use pinion_core::widgets::tile_grid::{Dropped, Tile, TileDrag, TileGrid};
///
/// let mut board = TileGrid::new(12);
/// board.place(Tile::new("first", 0, 0, 6, 1)).expect("an empty board");
///
/// // A palette entry is picked up. Nothing is over the board yet.
/// let mut drag = TileDrag::pick(&board, "second", 4, 2).expect("a free id that fits");
/// assert_eq!(drag.landing(), None);
///
/// // The pointer crosses the board, well past the right edge.
/// drag.hover(&board, 11, 1);
/// assert_eq!(drag.landing(), Some((8, 1)), "clamped so the footprint stays on");
/// assert_eq!(drag.preview(&board).map(|t| (t.col, t.row, t.w, t.h)), Some((8, 1, 4, 2)));
///
/// // The release lands exactly where the preview was.
/// let Ok(Dropped::Landed { at, .. }) = drag.drop_on(&mut board) else {
///     panic!("the board takes it")
/// };
/// assert_eq!(at, (8, 1));
/// assert_eq!(board.tile(&"second".into()).map(|t| (t.col, t.row)), Some((8, 1)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileDrag {
    carried: Carried,
    /// The landing, already resolved through the grid — or [`None`] while the
    /// pointer is not over the board at all.
    ///
    /// One field rather than a flag beside a cell. The reference keeps a
    /// "the pointer is over the board" bool next to a nullable snap and so can
    /// spell *not over the board, and here is where it would go*, which is
    /// nothing.
    over: Option<(u32, u32)>,
}

impl TileDrag {
    /// Open a drag on a tile the board holds, gripped at board cell
    /// `(col, row)`.
    ///
    /// The landing starts where the tile already is, so a press that never
    /// moves previews the truth rather than nothing.
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`].
    pub fn grip(grid: &TileGrid, id: &TileId, col: u32, row: u32) -> Result<Self, TileError> {
        let tile = grid
            .tile(id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        Ok(Self {
            carried: Carried::Placed {
                id: id.clone(),
                dx: col.saturating_sub(tile.col),
                dy: row.saturating_sub(tile.row),
            },
            over: Some((tile.col, tile.row)),
        })
    }

    /// Open a drag carrying a footprint the board does not hold yet.
    ///
    /// Refused **here**, at pick-up, rather than at the release: a palette
    /// entry that can never be placed should not be draggable, and the reason
    /// is available while the person is still holding it. The floor's add call
    /// returns nothing at all, so there the first news is a card drawn on top
    /// of another one.
    ///
    /// The landing starts as [`None`] — a footprint is picked up over the
    /// palette, which is not the board.
    ///
    /// # Errors
    ///
    /// [`TileError::Empty`] for a footprint with no area, [`TileError::TooWide`]
    /// for one wider than the board, [`TileError::Duplicate`] when the board
    /// already holds that id.
    pub fn pick(grid: &TileGrid, id: impl Into<TileId>, w: u32, h: u32) -> Result<Self, TileError> {
        if w == 0 || h == 0 {
            return Err(TileError::Empty { w, h });
        }
        if w > grid.columns() {
            return Err(TileError::TooWide {
                w,
                columns: grid.columns(),
            });
        }
        let id = id.into();
        if grid.tile(&id).is_some() {
            return Err(TileError::Duplicate(id));
        }
        Ok(Self {
            carried: Carried::Fresh { id, w, h },
            over: None,
        })
    }

    /// What is being carried.
    #[must_use]
    pub const fn carried(&self) -> &Carried {
        &self.carried
    }

    /// The pointer is over board cell `(col, row)`: resolve the landing.
    ///
    /// The cell is the one the *pointer* is in; the grip offset and the column
    /// clamp are applied here, once, so no caller applies either.
    pub fn hover(&mut self, grid: &TileGrid, col: u32, row: u32) {
        let (w, wanted) = match &self.carried {
            Carried::Placed { id, dx, dy } => {
                let Some(tile) = grid.tile(id) else { return };
                (tile.w, (col.saturating_sub(*dx), row.saturating_sub(*dy)))
            }
            Carried::Fresh { w, .. } => (*w, (col, row)),
        };
        self.over = Some(grid.landing_for(w, wanted.0, wanted.1));
    }

    /// The pointer left the board. There is no landing until it comes back, and
    /// a release now is [`Dropped::Abandoned`].
    pub fn leave(&mut self) {
        self.over = None;
    }

    /// The cell a release would put it in, or [`None`] while the pointer is off
    /// the board.
    #[must_use]
    pub const fn landing(&self) -> Option<(u32, u32)> {
        self.over
    }

    /// The tile a painter should draw as the preview — id, landing and
    /// footprint — or [`None`] while the pointer is off the board.
    ///
    /// Read from the same landing [`drop_on`](Self::drop_on) commits, which is
    /// the whole point of the type.
    #[must_use]
    pub fn preview(&self, grid: &TileGrid) -> Option<Tile> {
        let (col, row) = self.over?;
        match &self.carried {
            Carried::Placed { id, .. } => {
                let tile = grid.tile(id)?;
                Some(Tile::new(id.as_str(), col, row, tile.w, tile.h))
            }
            Carried::Fresh { id, w, h } => Some(Tile::new(id.as_str(), col, row, *w, *h)),
        }
    }

    /// Release: put what is carried where the preview was.
    ///
    /// # Errors
    ///
    /// Whatever [`TileGrid::place`] or [`TileGrid::move_to`] refuses — a board
    /// that changed under a drag long enough (a tile removed, an id taken) is
    /// a refusal with a name rather than a silent no-op.
    pub fn drop_on(self, grid: &mut TileGrid) -> Result<Dropped, TileError> {
        let Some((col, row)) = self.over else {
            return Ok(Dropped::Abandoned);
        };
        match self.carried {
            Carried::Placed { id, .. } => {
                if grid.tile(&id).map(|t| (t.col, t.row)) == Some((col, row)) {
                    return Ok(Dropped::Unmoved);
                }
                let reflow = grid.move_to(&id, col, row)?;
                Ok(Dropped::Landed {
                    at: (col, row),
                    reflow,
                })
            }
            Carried::Fresh { id, w, h } => {
                let reflow = grid.place(Tile::new(id.as_str(), col, row, w, h))?;
                Ok(Dropped::Landed {
                    at: (col, row),
                    reflow,
                })
            }
        }
    }
}

/// (R1609) **The one derivation under every resize**: put one edge of a tile on
/// a grid line, holding the opposite edge still.
///
/// Both public entry points funnel through here — [`TileGrid::drag_handle`] calls it once per edge
/// in the handle, [`TileGrid::nudge`] once with a one-cell step — so a corner drag and a `Grow`
/// chord cannot disagree about what moving a side means. The toolkit spreads
/// the equivalent across `setNewGeometry`, a per-operation `ChangeFlag` mask (`HResizeReverse` marks the two
/// edges that also move the origin) and widget's own min/max clamping.
///
/// Clamping, stated once here rather than at each call site:
///
/// * A start edge stops one line short of its opposite, so the tile keeps at
///   least one cell and never inverts. `w`/`h` are documented never zero and this
///   is where that stays true under a drag.
/// * A start edge stops at line zero by being unsigned — the caller's
///   `saturating_sub` already floored it.
/// * The right edge stops at the column count, which is the board's only bound.
/// * The bottom edge has **no** upper bound, because a dashboard's rows are
///   unbounded ([`TileGrid::rows`] is derived) and a card may always grow taller.
fn set_edge(tile: &mut Tile, edge: TileEdge, line: u32, columns: u32) {
    match edge {
        TileEdge::Left => {
            let right = tile.right();
            let col = line.min(right - 1);
            tile.col = col;
            tile.w = right - col;
        }
        TileEdge::Right => {
            // `col + w <= columns` and `w >= 1`, so `col + 1 <= columns`: the
            // clamp's bounds are ordered and it cannot panic.
            tile.w = line.clamp(tile.col + 1, columns) - tile.col;
        }
        TileEdge::Top => {
            let bottom = tile.bottom();
            let row = line.min(bottom - 1);
            tile.row = row;
            tile.h = bottom - row;
        }
        TileEdge::Bottom => {
            tile.h = line.max(tile.row + 1) - tile.row;
        }
    }
}

/// Zero-based model coordinate to CSS's one-based grid line.
///
/// A dashboard with more than 65 000 rows is not a dashboard, so the clamp is
/// the honest answer — and it is `try_from` rather than a hand-written bound,
/// because the cast is where a wrap would put a tile at the TOP of the grid
/// instead of off the bottom of it.
fn line(zero_based: u32) -> u16 {
    u16::try_from(zero_based.saturating_add(1)).unwrap_or(u16::MAX)
}

/// A span, never zero — `GridPlacement` lowers `span.max(1)` anyway, and saying
/// so here keeps the two from disagreeing.
fn span(cells: u32) -> u16 {
    u16::try_from(cells.max(1)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    /// ★★★★★ R1733 — the cell a palette drag PREVIEWS is the cell its release
    /// PLACES, over the whole board and past both edges.
    ///
    /// The property R1668 established for a tile the board already holds, on
    /// the drag that had no answer at all: before this round `landing` needed a
    /// tile in the grid, so a shell drawing "where this new card would go" had
    /// to write the clamp itself — which is exactly the two-clamps-one-fact
    /// shape R1668 measured, one gesture over.
    ///
    /// Driven rather than reasoned: every cell of a twelve-column board plus
    /// four columns past its right edge, for three footprint widths.
    #[test]
    fn r1733_a_fresh_drags_preview_is_the_cell_its_release_places() {
        use super::{Dropped, Tile, TileDrag, TileGrid, TileId};
        for w in [1_u32, 4, 12] {
            for col in 0..16_u32 {
                for row in 0..3_u32 {
                    let mut board = TileGrid::new(12);
                    board.place(Tile::new("sitting", 0, 0, 6, 1)).expect("fits");
                    let mut drag = TileDrag::pick(&board, "new", w, 2).expect("free id, fits");
                    drag.hover(&board, col, row);
                    let previewed = drag.preview(&board).expect("the pointer is on the board");
                    let landing = drag.landing().expect("the pointer is on the board");

                    let Ok(Dropped::Landed { at, .. }) = drag.drop_on(&mut board) else {
                        panic!("the board takes a {w}-wide footprint at ({col},{row})")
                    };
                    let placed = board.tile(&TileId::new("new")).expect("it is there now");

                    assert_eq!(at, landing, "{w} wide at ({col},{row})");
                    assert_eq!(
                        (previewed.col, previewed.row),
                        (placed.col, placed.row),
                        "{w} wide at ({col},{row}): previewed {previewed:?}, placed {placed:?}",
                    );
                    assert_eq!(
                        (previewed.w, previewed.h),
                        (placed.w, placed.h),
                        "the preview's footprint is the placed one",
                    );
                    assert!(
                        placed.col + placed.w <= 12,
                        "a drag past the right edge stops on the board",
                    );
                }
            }
        }
    }

    /// R1733 — a footprint that can never be placed is refused **while it is
    /// being picked up**, with the reason, rather than at the release.
    ///
    /// The floor's add call returns nothing at all (measured: `void`, and two
    /// items asked for the same cell both get geometry and overlap), so there
    /// the first news is a card drawn on top of another one.
    #[test]
    fn r1733_a_footprint_that_cannot_be_placed_is_refused_at_pick_up() {
        use super::{Tile, TileDrag, TileError, TileGrid, TileId};
        let mut board = TileGrid::new(12);
        board.place(Tile::new("taken", 0, 0, 2, 1)).expect("fits");

        assert_eq!(
            TileDrag::pick(&board, "zero", 0, 3).unwrap_err(),
            TileError::Empty { w: 0, h: 3 },
        );
        assert_eq!(
            TileDrag::pick(&board, "zero", 3, 0).unwrap_err(),
            TileError::Empty { w: 3, h: 0 },
        );
        assert_eq!(
            TileDrag::pick(&board, "huge", 13, 1).unwrap_err(),
            TileError::TooWide { w: 13, columns: 12 },
        );
        assert_eq!(
            TileDrag::pick(&board, "taken", 2, 1).unwrap_err(),
            TileError::Duplicate(TileId::new("taken")),
        );
        assert!(
            TileDrag::pick(&board, "fine", 12, 1).is_ok(),
            "exactly wide"
        );
    }

    /// ★★★★★ R1733 — a release with no landing changes nothing, and says so
    /// with its own word.
    ///
    /// Three outcomes a caller has to tell apart, and only one is a change.
    /// R1701 measured what a remembered comparison costs: a click that carried
    /// nothing reflowed the board and announced a move that had not happened.
    #[test]
    fn r1733_a_release_that_carried_nothing_is_its_own_answer() {
        use super::{Dropped, Tile, TileDrag, TileGrid, TileId};
        let mut board = TileGrid::new(12);
        board.place(Tile::new("a", 0, 0, 6, 1)).expect("fits");
        board.place(Tile::new("b", 6, 0, 6, 1)).expect("fits");
        let before = board.clone();

        // A palette drag released off the board.
        let mut fresh = TileDrag::pick(&board, "c", 3, 1).expect("free");
        fresh.hover(&board, 4, 4);
        assert!(fresh.landing().is_some());
        fresh.leave();
        assert_eq!(fresh.landing(), None, "leaving takes the preview away");
        assert_eq!(fresh.clone().drop_on(&mut board), Ok(Dropped::Abandoned));
        assert_eq!(board, before, "an abandoned drag is not a placement");

        // A card pressed and released without moving.
        let held = TileDrag::grip(&board, &TileId::new("b"), 8, 0).expect("it is there");
        assert_eq!(held.landing(), Some((6, 0)), "the preview starts truthful");
        assert_eq!(held.drop_on(&mut board), Ok(Dropped::Unmoved));
        assert_eq!(board, before, "a click is not a move");

        // And the same drag, carried one row down, IS a change.
        let mut moved = TileDrag::grip(&board, &TileId::new("b"), 8, 0).expect("there");
        moved.hover(&board, 8, 1);
        let Ok(Dropped::Landed { at, reflow }) = moved.drop_on(&mut board) else {
            panic!("the board takes it")
        };
        assert_eq!(at, (6, 1));
        assert!(reflow.is_clean(), "nothing was in the way");
        assert_ne!(board, before);
    }

    /// R1733 — the grip offset is applied once, inside the drag.
    ///
    /// A card grabbed by its middle keeps that grip: the pointer moving to
    /// column nine of a six-wide card gripped three columns in previews column
    /// six, not nine. The caller does no arithmetic, so there is no second
    /// place for it to be done differently.
    #[test]
    fn r1733_a_grip_offset_is_the_drags_own_arithmetic() {
        use super::{Tile, TileDrag, TileGrid, TileId};
        let mut board = TileGrid::new(12);
        board.place(Tile::new("wide", 0, 2, 6, 2)).expect("fits");
        // Gripped three columns in and one row down from the card's corner.
        let mut drag = TileDrag::grip(&board, &TileId::new("wide"), 3, 3).expect("there");
        drag.hover(&board, 9, 5);
        assert_eq!(
            drag.landing(),
            Some((6, 4)),
            "the pointer's cell less the grip"
        );
        drag.hover(&board, 1, 0);
        assert_eq!(
            drag.landing(),
            Some((0, 0)),
            "clamped at the left by unsign"
        );
        drag.hover(&board, 14, 2);
        assert_eq!(
            drag.landing(),
            Some((6, 1)),
            "and at the right by the board"
        );
    }

    /// ★★★★★ R1733 — what a drag carries is ONE thing, and the compiler is
    /// what checks it.
    ///
    /// The behaviour reference keeps a held card id and a held palette kind as
    /// two nullable fields, so every handler must remember to check the other
    /// before acting — and measured there, two guards read a field nothing ever
    /// sets. This asserts the property that makes such a guard impossible:
    /// reading what is carried is a match with no third case and no
    /// both-at-once case, and each arm names the id a drop would use.
    #[test]
    fn r1733_a_board_carries_one_thing_and_it_names_itself() {
        use super::{Carried, Tile, TileDrag, TileGrid, TileId};
        let mut board = TileGrid::new(12);
        board.place(Tile::new("sitting", 0, 0, 4, 1)).expect("fits");

        let held = TileDrag::grip(&board, &TileId::new("sitting"), 0, 0).expect("there");
        assert!(held.carried().is_placed());
        assert_eq!(held.carried().id(), &TileId::new("sitting"));
        assert!(matches!(
            held.carried(),
            Carried::Placed { dx: 0, dy: 0, .. }
        ));

        let fresh = TileDrag::pick(&board, "arriving", 3, 2).expect("free");
        assert!(!fresh.carried().is_placed());
        assert_eq!(fresh.carried().id(), &TileId::new("arriving"));
        assert!(matches!(fresh.carried(), Carried::Fresh { w: 3, h: 2, .. }));
        assert_eq!(
            fresh.landing(),
            None,
            "a footprint is picked up over the palette, which is not the board",
        );
    }

    /// R1733 — `landing_for`, `landing` and `place` apply ONE clamp.
    ///
    /// The clamp had been written out three times: here, in `landing`, and in
    /// every shell drawing a preview. This asserts the first two agree with the
    /// third for every column of a board and four past its edge.
    #[test]
    fn r1733_one_clamp_serves_the_query_the_move_and_the_placement() {
        use super::{Tile, TileGrid, TileId};
        let grid = TileGrid::new(12);
        for w in 1..=12_u32 {
            for col in 0..16_u32 {
                let asked = grid.landing_for(w, col, 1);
                let mut placed = grid.clone();
                placed.place(Tile::new("t", col, 1, w, 1)).expect("fits");
                let landed = placed.tile(&TileId::new("t")).expect("there");
                assert_eq!(asked, (landed.col, landed.row), "{w} wide at column {col}");

                let asked_again = placed.landing(&TileId::new("t"), col, 1).expect("there");
                assert_eq!(asked, asked_again, "the two queries are one clamp");
            }
        }
        assert_eq!(
            grid.landing_for(0, 5, 0),
            grid.landing_for(1, 5, 0),
            "a zero-wide footprint is treated as one, as `place` refuses it",
        );
    }

    /// R1668 — a preview that asks where a tile would land and a release that
    /// moves it agree, because the release is written in terms of the ask.
    ///
    /// Found on the wire: a six-column card dragged toward column seven of a
    /// twelve-column board previewed seven and committed six, because the shell
    /// clamped one way and this file clamped another. One fact, two clamps --
    /// the shape R1654 named.
    #[test]
    fn r1668_the_landing_a_preview_asks_for_is_the_one_a_move_takes() {
        use super::{Tile, TileGrid, TileId};
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("wide", 0, 0, 6, 1)).expect("fits");
        let id = TileId::new("wide");
        for col in 0..14 {
            let asked = grid.landing(&id, col, 3).expect("the tile is there");
            let mut moved = grid.clone();
            moved.move_to(&id, col, 3).expect("the tile is there");
            let landed = moved
                .tiles()
                .iter()
                .find(|t| t.id == id)
                .map(|t| (t.col, t.row))
                .expect("still there");
            assert_eq!(asked, landed, "asked for column {col}");
        }
        assert_eq!(
            grid.landing(&id, 11, 0),
            Some((6, 0)),
            "a six-wide tile cannot start past column six of twelve",
        );
        assert_eq!(grid.landing(&TileId::new("absent"), 0, 0), None);
    }

    use super::*;

    fn dashboard() -> TileGrid {
        // The classic arrangement the runtime measurement lays out: a full-width
        // header, two halves under it, and a tall card under those.
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("header", 0, 0, 12, 1)).unwrap();
        grid.place(Tile::new("left", 0, 1, 6, 1)).unwrap();
        grid.place(Tile::new("right", 6, 1, 6, 1)).unwrap();
        grid.place(Tile::new("tall", 0, 2, 4, 2)).unwrap();
        grid
    }

    #[test]
    fn r1648_maximizing_fills_the_board_and_hands_back_the_way_home() {
        let mut grid = dashboard();
        let before = grid.clone();
        let token = grid.maximize(&TileId::new("left")).unwrap();

        assert_eq!(grid.tiles().len(), 1, "one card fills the board");
        assert!(grid.is_filled_by_one());
        let only = &grid.tiles()[0];
        assert_eq!(only.id, TileId::new("left"));
        assert_eq!((only.col, only.row, only.w), (0, 0, 12));
        assert_eq!(
            only.h,
            before.rows(),
            "as tall as the arrangement it replaced, so the scroll extent holds"
        );

        assert_eq!(token.id(), &TileId::new("left"));
        assert_eq!(
            token.peek(),
            &before,
            "peeking does not consume the way home"
        );
        assert_eq!(token.restore(), before, "and restoring returns it exactly");
    }

    #[test]
    fn r1648_maximizing_an_absent_tile_is_refused_and_leaves_the_board_alone() {
        // The failure direction that matters: a board that lost its
        // arrangement to a typo would be unrecoverable, because the token that
        // recovers it is the return value the failed call did not produce.
        let mut grid = dashboard();
        let before = grid.clone();
        let refused = grid.maximize(&TileId::new("nope"));
        assert!(matches!(refused, Err(TileError::NoSuchTile(_))));
        assert_eq!(grid, before, "a refused maximise is not an edit");
    }

    #[test]
    fn r1648_a_second_maximise_cannot_clobber_the_first_way_home() {
        // The bug the token exists to make impossible. With the arrangement
        // kept on the side, maximising `left` and then `right` overwrites the
        // saved copy with a board that is already maximised, and the original
        // is gone. Here the second call's token restores to the ONE-tile board
        // and the first token still holds the real arrangement.
        let mut grid = dashboard();
        let original = grid.clone();
        let first = grid.maximize(&TileId::new("left")).unwrap();
        let one_tile = grid.clone();
        let second = grid.maximize(&TileId::new("left")).unwrap();

        assert_eq!(
            second.peek(),
            &one_tile,
            "the second token is one edit deep"
        );
        assert_eq!(
            first.peek(),
            &original,
            "and the first still holds the arrangement the user made"
        );
    }

    #[test]
    fn r1648_a_maximised_board_round_trips_with_its_way_home() {
        // A session saved while a card was maximised must reopen able to
        // un-maximise: a token that did not serialise would make "save" a way
        // to lose the layout.
        let mut grid = dashboard();
        let before = grid.clone();
        let token = grid.maximize(&TileId::new("tall")).unwrap();
        let json = serde_json::to_string(&token).expect("serialize");
        let back: Maximized = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id(), &TileId::new("tall"));
        assert_eq!(back.restore(), before);
    }

    #[test]
    fn r1648_one_full_width_tile_reads_as_filled_and_a_narrower_one_does_not() {
        // `is_filled_by_one` reports a SHAPE, and this states the boundary so a
        // reader does not mistake it for a maximised flag: an arrangement a
        // user built by hand can have the shape without a token existing.
        let mut hand_made = TileGrid::new(12);
        hand_made.place(Tile::new("solo", 0, 0, 12, 3)).unwrap();
        assert!(hand_made.is_filled_by_one(), "shape, not provenance");

        let mut narrow = TileGrid::new(12);
        narrow.place(Tile::new("solo", 0, 0, 6, 3)).unwrap();
        assert!(!narrow.is_filled_by_one());
    }

    #[test]
    fn a_packed_arrangement_is_legal_and_derives_its_height() {
        let grid = dashboard();
        assert!(grid.violations().is_empty(), "touching is not overlapping");
        assert_eq!(grid.rows(), 4);
        assert_eq!(grid.columns(), 12);
        assert!(grid.is_free(4, 2, 8, 2));
        assert!(!grid.is_free(0, 0, 1, 1));
    }

    #[test]
    fn the_placement_is_the_one_conversion_to_css_lines() {
        let grid = dashboard();
        let (col, row) = grid.placement(&TileId::new("right")).unwrap();
        assert_eq!(col, GridPlacement::spanning(7, 6), "col 6 is grid line 7");
        assert_eq!(row, GridPlacement::spanning(2, 1), "row 1 is grid line 2");
        let (col, row) = grid.placement(&TileId::new("header")).unwrap();
        assert_eq!((col.start_line, row.start_line), (Some(1), Some(1)));
        assert_eq!(grid.placement(&TileId::new("nope")), None);
    }

    #[test]
    fn a_move_onto_an_occupied_slot_displaces_and_says_what_it_displaced() {
        let mut grid = dashboard();
        let reflow = grid.move_to(&TileId::new("tall"), 0, 0).unwrap();

        assert!(!reflow.is_clean());
        let names: Vec<&str> = reflow.displaced().iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            names,
            vec!["header", "left"],
            "★ the dashboard tool reflows silently; every tile that moved is named here"
        );
        assert_eq!(grid.tile(&TileId::new("tall")).unwrap().row, 0);
        assert_eq!(grid.tile(&TileId::new("header")).unwrap().row, 2);
        assert_eq!(
            grid.tile(&TileId::new("left")).unwrap().row,
            3,
            "pushed by `tall`, then again by the `header` that `tall` had pushed \
             onto it — the reflow is transitive"
        );
        assert_eq!(
            grid.tile(&TileId::new("right")).unwrap().row,
            1,
            "★ and `right` did not move at all: it shares no column with `tall`, \
             so a drag's effect stays LOCAL. The first draft of this test expected \
             it to move and the implementation was right"
        );
        assert!(grid.violations().is_empty());
    }

    #[test]
    fn a_move_that_fits_displaces_nothing() {
        let mut grid = dashboard();
        let reflow = grid.move_to(&TileId::new("tall"), 8, 2).unwrap();
        assert!(reflow.is_clean(), "the right of row 2 was free");
        assert!(grid.violations().is_empty());
        assert_eq!(grid.rows(), 4);
    }

    #[test]
    fn a_drag_past_the_edge_stops_at_the_edge() {
        let mut grid = dashboard();
        grid.move_to(&TileId::new("tall"), 99, 2).unwrap();
        let tall = grid.tile(&TileId::new("tall")).unwrap();
        assert_eq!(
            tall.col, 8,
            "12 columns, 4 wide, so the left edge stops at 8"
        );
        assert_eq!(tall.right(), 12);
        assert!(grid.violations().is_empty());
    }

    #[test]
    fn growing_a_tile_pushes_what_it_grows_into() {
        let mut grid = dashboard();
        let reflow = grid.resize(&TileId::new("header"), 12, 2).unwrap();
        assert!(!reflow.is_clean());
        assert_eq!(grid.tile(&TileId::new("left")).unwrap().row, 2);
        assert_eq!(grid.tile(&TileId::new("tall")).unwrap().row, 3);
        assert!(grid.violations().is_empty());
    }

    #[test]
    fn a_tile_that_does_not_fit_is_refused_by_name_rather_than_clamped() {
        let mut grid = TileGrid::new(6);
        let wide = grid.place(Tile::new("wide", 0, 0, 8, 1));
        assert_eq!(
            wide,
            Err(TileError::TooWide { w: 8, columns: 6 }),
            "silently narrowing it would hide the arithmetic that produced it"
        );
        assert_eq!(
            grid.place(Tile::new("flat", 0, 0, 3, 0)),
            Err(TileError::Empty { w: 3, h: 0 })
        );
        grid.place(Tile::new("ok", 0, 0, 3, 1)).unwrap();
        assert_eq!(
            grid.place(Tile::new("ok", 3, 0, 3, 1)),
            Err(TileError::Duplicate(TileId::new("ok"))),
            "an id is an identity"
        );
        assert_eq!(
            grid.move_to(&TileId::new("ghost"), 0, 0),
            Err(TileError::NoSuchTile(TileId::new("ghost")))
        );
        assert_eq!(grid.tiles().len(), 1, "no refusal changed the grid");
    }

    #[test]
    fn compaction_is_a_verb_and_it_names_what_it_moved() {
        let mut grid = dashboard();
        grid.remove(&TileId::new("header")).unwrap();
        assert_eq!(
            grid.tile(&TileId::new("left")).unwrap().row,
            1,
            "★ removing a tile leaves the gap: a drag's effect stays local, and \
             its inverse stays a drag"
        );

        let reflow = grid.compact();
        let names: Vec<&str> = reflow.displaced().iter().map(|d| d.id.as_str()).collect();
        assert_eq!(names, vec!["left", "right", "tall"]);
        assert_eq!(grid.tile(&TileId::new("left")).unwrap().row, 0);
        assert_eq!(grid.tile(&TileId::new("tall")).unwrap().row, 1);
        assert_eq!(grid.rows(), 3);
        assert!(grid.violations().is_empty());

        assert!(
            grid.compact().is_clean(),
            "and it is idempotent — a second tidy has nothing to do"
        );
    }

    #[test]
    fn compaction_does_not_float_a_tile_through_another() {
        let mut grid = TileGrid::new(4);
        grid.place(Tile::new("top", 0, 0, 2, 1)).unwrap();
        grid.place(Tile::new("under", 0, 5, 2, 1)).unwrap();
        grid.compact();
        assert_eq!(grid.tile(&TileId::new("top")).unwrap().row, 0);
        assert_eq!(
            grid.tile(&TileId::new("under")).unwrap().row,
            1,
            "it rose until it met the tile above rather than past it"
        );
    }

    #[test]
    fn the_invariant_survives_every_operation_in_sequence() {
        // ★ The property the type exists for, driven rather than argued: a run
        // of moves and resizes that deliberately collides, with the invariant
        // re-checked after each one.
        let mut grid = dashboard();
        let script: &[(&str, u32, u32)] = &[
            ("tall", 0, 0),
            ("header", 6, 1),
            ("left", 0, 0),
            ("right", 0, 0),
            ("tall", 4, 1),
            ("header", 0, 0),
        ];
        for (id, col, row) in script {
            grid.move_to(&TileId::new(*id), *col, *row).unwrap();
            assert!(
                grid.violations().is_empty(),
                "after moving {id} to ({col},{row}): {:?}",
                grid.tiles()
            );
        }
        for (id, w, h) in [("tall", 12u32, 1u32), ("left", 1, 4), ("right", 6, 2)] {
            grid.resize(&TileId::new(id), w, h).unwrap();
            assert!(grid.violations().is_empty(), "after resizing {id}");
        }
        assert_eq!(grid.tiles().len(), 4, "nothing was lost along the way");
    }

    #[test]
    fn a_grid_that_arrived_broken_names_its_violations() {
        // A layout from a file has promised nothing, which is why `violations`
        // exists at all.
        let json = r#"{"columns":4,"tiles":[
            {"id":"a","col":0,"row":0,"w":2,"h":2},
            {"id":"b","col":1,"row":1,"w":2,"h":1}]}"#;
        let grid: TileGrid = serde_json::from_str(json).unwrap();
        let found = grid.violations();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], (TileId::new("a"), TileId::new("b")));
    }

    #[test]
    fn an_arrangement_round_trips_so_a_named_preset_is_just_a_value() {
        let grid = dashboard();
        let wire = serde_json::to_string(&grid).unwrap();
        let back: TileGrid = serde_json::from_str(&wire).unwrap();
        assert_eq!(back, grid);
        assert!(back.violations().is_empty());
        assert!(
            wire.contains("\"header\""),
            "the ids are the application's, so a preset re-binds to its panels"
        );
    }

    /// ★★★★★ R1900 — the round that let a cell be shared must not delete the
    /// layouts R1897 put on people's disks.
    #[test]
    fn an_arrangement_saved_before_cells_could_be_shared_still_loads() {
        let json = r#"{"columns":12,"tiles":[
            {"id":"packet","col":0,"row":0,"w":6,"h":2},
            {"id":"share","col":6,"row":0,"w":6,"h":2}]}"#;
        let grid: TileGrid = serde_json::from_str(json).expect("a layout written before R1900");
        assert_eq!(grid.tiles().len(), 2);
        for tile in grid.tiles() {
            assert!(!tile.is_shared(), "a cell nobody shared holds one occupant");
            assert_eq!(tile.members(), &[tile.id.clone()]);
        }
    }

    #[test]
    fn a_shared_cell_round_trips_and_its_stored_form_names_the_front() {
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("packet", 0, 0, 6, 2)).expect("empty");
        grid.place(Tile::new("share", 6, 0, 6, 2)).expect("beside");
        grid.share(&TileId::new("share"), &TileId::new("packet"))
            .expect("two cells");

        let wire = serde_json::to_string(&grid).expect("an arrangement is a value");
        assert!(
            wire.contains(r#""id":"share""#) && wire.contains(r#""here":["packet","share"]"#),
            "the front is the id and the strip is listed once: {wire}"
        );
        let back: TileGrid = serde_json::from_str(&wire).expect("its own output");
        assert_eq!(back, grid, "a shared cell survives a save");
        assert!(back.tile(&TileId::new("packet")).expect("here").is_shared());
    }

    #[test]
    fn a_stored_cell_whose_front_is_not_one_of_its_occupants_is_refused() {
        let json = r#"{"columns":12,"tiles":[
            {"id":"ghost","col":0,"row":0,"w":6,"h":2,"here":["packet","share"]}]}"#;
        let refused =
            serde_json::from_str::<TileGrid>(json).expect_err("the front must be an occupant");
        let sentence = refused.to_string();
        assert!(sentence.contains("ghost"), "{sentence}");
        assert!(sentence.contains("packet, share"), "{sentence}");
    }

    #[test]
    fn sharing_a_cell_leaves_one_cell_holding_both_and_hands_back_what_it_vacated() {
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("packet", 0, 0, 6, 2)).expect("empty");
        grid.place(Tile::new("share", 6, 0, 6, 2)).expect("beside");

        let joined = grid
            .share(&TileId::new("share"), &TileId::new("packet"))
            .expect("two cells that exist");
        assert_eq!(
            joined.place,
            TileId::new("share"),
            "the joiner comes forward"
        );
        assert_eq!(
            joined.members,
            vec![TileId::new("packet"), TileId::new("share")]
        );
        let vacated = joined.vacated.expect("it was alone where it came from");
        assert_eq!((vacated.col, vacated.w), (6, 6), "the rectangle it left");

        assert_eq!(grid.tiles().len(), 1);
        let cell = grid
            .tile(&TileId::new("packet"))
            .expect("found by either name");
        assert!(cell.is_shared());
        assert_eq!(cell.col, 0, "the host cell did not move");
    }

    #[test]
    fn a_cell_cannot_be_shared_with_itself_and_the_refusal_is_its_own_arm() {
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("packet", 0, 0, 6, 2)).expect("empty");
        grid.place(Tile::new("share", 6, 0, 6, 2)).expect("beside");
        grid.share(&TileId::new("share"), &TileId::new("packet"))
            .expect("two cells");

        let refused = grid
            .share(&TileId::new("share"), &TileId::new("packet"))
            .expect_err("they are one cell now");
        assert!(
            matches!(refused, TileError::SelfShare { .. }),
            "not a stacking refusal: the stack was never asked — {refused}"
        );
        assert_eq!(grid.tiles().len(), 1, "a refusal changes nothing");
    }

    #[test]
    fn revealing_a_tab_changes_what_the_cell_is_named_and_nothing_else() {
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("packet", 0, 0, 6, 2)).expect("empty");
        grid.place(Tile::new("share", 6, 0, 6, 2)).expect("beside");
        grid.share(&TileId::new("share"), &TileId::new("packet"))
            .expect("two cells");

        let moved = grid.reveal(&TileId::new("packet")).expect("a member");
        assert_eq!(
            (moved.was, moved.now),
            (TileId::new("share"), TileId::new("packet"))
        );
        let cell = grid.tile(&TileId::new("share")).expect("still here");
        assert_eq!(cell.id, TileId::new("packet"), "the name follows the front");
        assert_eq!(cell.members().len(), 2, "and nobody left");
    }

    #[test]
    fn taking_a_tab_back_out_gives_it_the_cell_size_it_was_sharing() {
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("packet", 0, 0, 6, 2)).expect("empty");
        grid.place(Tile::new("share", 6, 0, 6, 2)).expect("beside");
        grid.share(&TileId::new("share"), &TileId::new("packet"))
            .expect("two cells");

        grid.unshare(&TileId::new("share"), 6, 0)
            .expect("a shared cell");
        let out = grid.tile(&TileId::new("share")).expect("its own cell now");
        assert_eq!((out.col, out.row, out.w, out.h), (6, 0, 6, 2));
        assert!(!out.is_shared());
        assert!(
            !grid.tile(&TileId::new("packet")).expect("here").is_shared(),
            "and the cell it left holds only the other one"
        );
        assert!(grid.violations().is_empty());
    }

    #[test]
    fn the_last_occupant_of_a_cell_cannot_be_taken_out_of_it() {
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("packet", 0, 0, 6, 2)).expect("empty");
        let refused = grid
            .unshare(&TileId::new("packet"), 6, 0)
            .expect_err("it shares with nobody");
        assert!(matches!(
            refused,
            TileError::Stacking(crate::stacking::StackRefusal::Sole { .. })
        ));
        assert_eq!(grid.tiles().len(), 1, "and the board is unchanged");
    }

    #[test]
    fn lifting_a_tab_off_the_board_leaves_the_cell_for_the_others() {
        let mut grid = TileGrid::new(12);
        grid.place(Tile::new("packet", 0, 0, 6, 2)).expect("empty");
        grid.place(Tile::new("share", 6, 0, 6, 2)).expect("beside");
        grid.share(&TileId::new("share"), &TileId::new("packet"))
            .expect("two cells");

        let carried = grid.lift(&TileId::new("share")).expect("a member");
        assert_eq!((carried.w, carried.h), (6, 2), "it carries the size it had");
        assert_eq!(grid.tiles().len(), 1, "the cell stays for the other one");
        assert!(!grid.tile(&TileId::new("packet")).expect("here").is_shared());

        let alone = grid
            .lift(&TileId::new("packet"))
            .expect("the sole occupant");
        assert_eq!(alone.id, TileId::new("packet"));
        assert!(
            grid.tiles().is_empty(),
            "and a sole occupant takes its cell"
        );
    }

    #[test]
    fn a_degenerate_column_count_is_clamped_rather_than_making_a_gridless_grid() {
        let mut grid = TileGrid::new(0);
        assert_eq!(grid.columns(), 1);
        grid.place(Tile::new("only", 5, 0, 1, 1)).unwrap();
        assert_eq!(grid.tile(&TileId::new("only")).unwrap().col, 0);
    }

    // ---- R1609: an edge is a handle, and the board is editable by keyboard ---

    fn tile_of(grid: &TileGrid, id: &str) -> Tile {
        grid.tile(&TileId::new(id)).expect("a seeded tile").clone()
    }

    #[test]
    fn r1609_a_handle_derives_its_edges_and_its_cursor() {
        // The enumeration the toolkit keeps in a private header: eight
        // handles, and each one's edges and cursor FOLLOW from which axes it
        // moves.
        assert_eq!(TileHandle::ALL.len(), 8);
        for handle in TileHandle::ALL {
            let axes = usize::from(handle.horizontal().is_some())
                + usize::from(handle.vertical().is_some());
            assert!((1..=2).contains(&axes), "{handle:?} moves {axes} axes");
            let moved: Vec<TileEdge> = TileEdge::ALL
                .into_iter()
                .filter(|e| handle.moves(*e))
                .collect();
            assert_eq!(moved.len(), axes, "{handle:?} moves exactly its own edges");
            // No handle may claim both sides of one axis — that would be a
            // rectangle with two lefts.
            assert!(!(handle.moves(TileEdge::Left) && handle.moves(TileEdge::Right)));
            assert!(!(handle.moves(TileEdge::Top) && handle.moves(TileEdge::Bottom)));
        }

        // The diagonal's slope is derived from `is_start`, not tabulated.
        assert_eq!(TileHandle::TopLeft.cursor(), CursorHint::NwseResize);
        assert_eq!(TileHandle::BottomRight.cursor(), CursorHint::NwseResize);
        assert_eq!(TileHandle::TopRight.cursor(), CursorHint::NeswResize);
        assert_eq!(TileHandle::BottomLeft.cursor(), CursorHint::NeswResize);
        assert_eq!(TileHandle::Left.cursor(), CursorHint::ColResize);
        assert_eq!(TileHandle::Bottom.cursor(), CursorHint::RowResize);
    }

    #[test]
    fn r1609_the_handle_hit_test_is_a_pure_function_of_the_point() {
        // The toolkit caches nine regions and rebuilds them from `updateDirtyRegions` whenever
        // the geometry moves; this needs no cache, so there is nothing to
        // invalidate.
        assert_eq!(TileHandle::at(0.5, 0.5, 0.25), None, "the interior moves");
        assert_eq!(TileHandle::at(0.02, 0.5, 0.25), Some(TileHandle::Left));
        assert_eq!(TileHandle::at(0.98, 0.5, 0.25), Some(TileHandle::Right));
        assert_eq!(TileHandle::at(0.5, 0.02, 0.25), Some(TileHandle::Top));
        assert_eq!(TileHandle::at(0.5, 0.98, 0.25), Some(TileHandle::Bottom));
        assert_eq!(TileHandle::at(0.02, 0.02, 0.25), Some(TileHandle::TopLeft));
        assert_eq!(TileHandle::at(0.98, 0.02, 0.25), Some(TileHandle::TopRight));
        assert_eq!(
            TileHandle::at(0.02, 0.98, 0.25),
            Some(TileHandle::BottomLeft)
        );
        assert_eq!(
            TileHandle::at(0.98, 0.98, 0.25),
            Some(TileHandle::BottomRight)
        );

        // A band wider than half would let left and right both claim the middle;
        // it is clamped instead, so the whole card is a handle ring with no
        // interior and every point still answers exactly once.
        assert_eq!(TileHandle::at(0.5, 0.5, 9.0), None, "0.5 is exclusive");
        assert_eq!(TileHandle::at(0.49, 0.5, 9.0), Some(TileHandle::Left));
        assert_eq!(TileHandle::at(0.51, 0.5, 9.0), Some(TileHandle::Right));

        // A non-finite sample reads as the interior: a bad pointer packet moves
        // a card rather than silently resizing it.
        assert_eq!(TileHandle::at(f32::NAN, f32::NAN, 0.25), None);
    }

    #[test]
    fn r1609_dragging_a_side_holds_the_opposite_one_still() {
        let mut grid = dashboard();
        // `tall` is col 0..4 on rows 2..4. Drag its RIGHT edge onto cell 7.
        grid.drag_handle(&TileId::new("tall"), TileHandle::Right, 7, 3)
            .unwrap();
        let tall = tile_of(&grid, "tall");
        assert_eq!((tall.col, tall.w), (0, 8), "the dragged side covers cell 7");
        assert_eq!((tall.row, tall.h), (2, 2), "the other axis is untouched");

        // Now its LEFT edge onto cell 3: the right edge must not move, so the
        // column and the width change together.
        grid.drag_handle(&TileId::new("tall"), TileHandle::Left, 3, 3)
            .unwrap();
        let tall = tile_of(&grid, "tall");
        assert_eq!((tall.col, tall.w, tall.right()), (3, 5, 8));
        assert!(grid.violations().is_empty());
    }

    #[test]
    fn r1609_a_side_dragged_past_its_opposite_stops_one_cell_short() {
        let mut grid = dashboard();
        // Drag `tall`'s left edge far to the RIGHT, past its own right edge.
        grid.drag_handle(&TileId::new("tall"), TileHandle::Left, 99, 3)
            .unwrap();
        let tall = tile_of(&grid, "tall");
        assert_eq!(
            (tall.col, tall.w),
            (3, 1),
            "a card never inverts and never reaches zero cells"
        );
        // And the top edge dragged past the bottom.
        grid.drag_handle(&TileId::new("tall"), TileHandle::Top, 3, 99)
            .unwrap();
        let tall = tile_of(&grid, "tall");
        assert_eq!((tall.row, tall.h), (3, 1));
        assert!(grid.violations().is_empty());
        // The right edge stops at the board's own bound.
        grid.drag_handle(&TileId::new("tall"), TileHandle::Right, 99, 3)
            .unwrap();
        assert_eq!(tile_of(&grid, "tall").right(), 12);
        assert!(grid.violations().is_empty());
    }

    #[test]
    fn r1609_a_corner_moves_both_edges_before_anything_reflows() {
        // ★ The decision the round had to make, asserted as a difference rather
        // than argued. A corner drag that SHRINKS one axis while GROWING the
        // other has an intermediate rectangle covering cells the final one does
        // not, so resolving the edges one at a time can displace a card the
        // gesture never reaches — and the reflow only pushes down, so that card
        // never comes back.
        //
        // ★ The first draft of this test grew both axes and its two routes
        // agreed, because a rectangle reached by two outward steps contains
        // every intermediate. The hazard needs one edge going each way.
        //
        // ★★ And the SECOND draft — one such fixture — was caught by a
        // counterfactual, which is the more valuable finding. A per-edge
        // implementation resolves the edges in some fixed order, and for any
        // single fixture one of the two orders is harmless; the loop here happens
        // to take the horizontal edge first, so a fixture where *vertical*-first
        // is the harmful one let the rejected design pass. Both mirror images are
        // therefore fixtures, so no order is safe in both and the assertion is
        // about the DESIGN rather than about one arrangement.
        //
        // In each: `keep` sits in the region the harmful intermediate sweeps and
        // the final rectangle never reaches.
        let board = |wide: Tile| {
            let mut grid = TileGrid::new(12);
            for tile in [wide, Tile::new("keep", 4, 1, 2, 1)] {
                grid.place(tile).unwrap();
            }
            grid
        };
        let wide = TileId::new("wide");

        for (label, start, to, target) in [
            (
                // Shrinks horizontally, grows vertically: taking the BOTTOM edge
                // first makes it six columns by four rows for an instant.
                "vertical-first is the harmful order",
                Tile::new("wide", 0, 0, 6, 1),
                (2u32, 3u32),
                Tile::new("wide", 0, 0, 3, 4),
            ),
            (
                // The mirror — grows horizontally, shrinks vertically: taking the
                // RIGHT edge first makes it six columns by four rows for an
                // instant, and that is the order the implementation's loop uses.
                "horizontal-first is the harmful order",
                Tile::new("wide", 0, 0, 3, 4),
                (5, 0),
                Tile::new("wide", 0, 0, 6, 1),
            ),
        ] {
            let (col, row) = to;
            let mut together = board(start.clone());
            let one = together
                .drag_handle(&wide, TileHandle::BottomRight, col, row)
                .unwrap();

            let mut horizontal_first = board(start.clone());
            horizontal_first
                .drag_handle(&wide, TileHandle::Right, col, row)
                .unwrap();
            horizontal_first
                .drag_handle(&wide, TileHandle::Bottom, col, row)
                .unwrap();

            let mut vertical_first = board(start.clone());
            vertical_first
                .drag_handle(&wide, TileHandle::Bottom, col, row)
                .unwrap();
            vertical_first
                .drag_handle(&wide, TileHandle::Right, col, row)
                .unwrap();

            for (name, grid) in [
                ("one gesture", &together),
                ("horizontal first", &horizontal_first),
                ("vertical first", &vertical_first),
            ] {
                assert_eq!(
                    tile_of(grid, "wide"),
                    target,
                    "{label}: {name} must reach the same rectangle — the routes \
                     differ in cost, not in destination"
                );
                assert!(
                    grid.violations().is_empty(),
                    "{label}: {name} left the board illegal"
                );
            }

            assert!(
                one.is_clean(),
                "{label}: the final rectangle never reaches `keep`, so one \
                 gesture displaces nothing"
            );
            assert_eq!(
                tile_of(&together, "keep").row,
                1,
                "{label}: and `keep` did not move"
            );

            // Exactly one of the split orders swept it, and which one is the
            // point: a per-edge route's answer depends on an order the user never
            // chose, while one gesture has no order to depend on.
            let split = [
                tile_of(&horizontal_first, "keep").row,
                tile_of(&vertical_first, "keep").row,
            ];
            assert!(
                split.contains(&1) && split.iter().any(|row| *row != 1),
                "{label}: one split order must sweep `keep` and the other must \
                 not, got {split:?}"
            );
        }
    }

    #[test]
    fn r1609_the_keyboard_vocabulary_is_twelve_values_and_every_one_is_an_edge() {
        let mut grid = dashboard();
        let tall = TileId::new("tall");

        // Move: the whole card slides.
        grid.nudge(&tall, TileNudge::Move(TileDirection::Right))
            .unwrap();
        assert_eq!(
            (tile_of(&grid, "tall").col, tile_of(&grid, "tall").w),
            (1, 4)
        );

        // Grow / Shrink on each side, checked against the edge that must hold.
        for (nudge, expect) in [
            (TileNudge::Grow(TileDirection::Right), (1u32, 5u32)),
            (TileNudge::Shrink(TileDirection::Right), (1, 4)),
            (TileNudge::Grow(TileDirection::Left), (0, 5)),
            (TileNudge::Shrink(TileDirection::Left), (1, 4)),
        ] {
            grid.nudge(&tall, nudge).unwrap();
            let t = tile_of(&grid, "tall");
            assert_eq!((t.col, t.w), expect, "after {nudge:?}");
        }
        for (nudge, expect) in [
            (TileNudge::Grow(TileDirection::Down), (2u32, 3u32)),
            (TileNudge::Shrink(TileDirection::Down), (2, 2)),
            (TileNudge::Grow(TileDirection::Up), (1, 3)),
            (TileNudge::Shrink(TileDirection::Up), (2, 2)),
        ] {
            grid.nudge(&tall, nudge).unwrap();
            let t = tile_of(&grid, "tall");
            assert_eq!((t.row, t.h), expect, "after {nudge:?}");
        }
        assert!(grid.violations().is_empty());

        // ★ Every Grow/Shrink pair is an exact inverse FOR THE CARD, and the
        // first draft of this asserted it for the whole BOARD and was wrong.
        // Growing downward pushes cards out of the way and shrinking back does
        // not float them home, because the reflow only moves tiles down and
        // undoing that is `compact`'s job — a separate verb by R1607's choice.
        // That is exactly why an undo has to be `TileEdit` and not an inverse
        // gesture, and it is asserted here instead of assumed.
        for direction in TileDirection::ALL {
            let grow = TileNudge::Grow(direction);
            assert_eq!(grow.inverse(), TileNudge::Shrink(direction));
            let board_before = grid.clone();
            let card_before = tile_of(&grid, "tall");
            let session = TileEdit::begin(&grid, &tall).unwrap();

            grid.nudge(&tall, grow).unwrap();
            grid.nudge(&tall, grow.inverse()).unwrap();
            assert_eq!(
                tile_of(&grid, "tall"),
                card_before,
                "Grow({direction:?}) then Shrink must restore the CARD"
            );
            let board_after = grid.clone();
            if direction == TileDirection::Down {
                assert_ne!(
                    board_after, board_before,
                    "★ growing downward displaced a card that shrinking back does \
                     NOT float home — an inverse gesture is not an undo"
                );
            }

            grid = session.cancel();
            assert_eq!(grid, board_before, "and cancel restores the BOARD");
        }
    }

    #[test]
    fn r1609_a_nudge_with_nowhere_to_go_leaves_the_arrangement_equal() {
        // R1549's rule: a held arrow key at a bound STOPS. It does not error
        // (that would make a repeat a failure) and it does not silently
        // pretend to have worked — the arrangement is a value, so "nothing
        // happened" is one comparison, which is how the toolkit's own
        // keyPressEvent detects it too.
        let mut grid = TileGrid::new(4);
        grid.place(Tile::new("one", 0, 0, 1, 1)).unwrap();
        let id = TileId::new("one");
        let edit = TileEdit::begin(&grid, &id).unwrap();

        for nudge in [
            TileNudge::Move(TileDirection::Left),
            TileNudge::Move(TileDirection::Up),
            TileNudge::Shrink(TileDirection::Left),
            TileNudge::Shrink(TileDirection::Right),
            TileNudge::Shrink(TileDirection::Up),
            TileNudge::Shrink(TileDirection::Down),
            TileNudge::Grow(TileDirection::Left),
            TileNudge::Grow(TileDirection::Up),
        ] {
            let reflow = grid.nudge(&id, nudge).expect("a bound is not an error");
            assert!(reflow.is_clean(), "{nudge:?} displaced something");
            assert!(!edit.changed(&grid), "{nudge:?} changed the board");
        }
        assert_eq!(tile_of(&grid, "one"), Tile::new("one", 0, 0, 1, 1));
        assert_eq!(
            grid.nudge(&TileId::new("ghost"), TileNudge::Move(TileDirection::Down)),
            Err(TileError::NoSuchTile(TileId::new("ghost")))
        );
    }

    #[test]
    fn r1609_a_session_can_be_taken_back_and_it_restores_the_whole_board() {
        // ★ the toolkit saves `oldGeometry` on entering interactive mode and never reads
        // it: Escape, Return and Enter share one arm. And even restoring the
        // rectangle would not be enough — a move displaces OTHER cards.
        let mut grid = dashboard();
        let tall = TileId::new("tall");
        let edit = TileEdit::begin(&grid, &tall).unwrap();
        assert_eq!(edit.id(), &tall);
        assert!(!edit.changed(&grid), "a fresh session has changed nothing");

        for _ in 0..3 {
            grid.nudge(&tall, TileNudge::Move(TileDirection::Up))
                .unwrap();
        }
        assert_eq!(tile_of(&grid, "tall").row, 0);
        assert!(edit.changed(&grid));
        assert_ne!(
            tile_of(&grid, "header").row,
            0,
            "the session displaced a card that is not the one being edited"
        );

        let restored = edit.cancel();
        assert_eq!(
            restored,
            dashboard(),
            "cancel puts the displaced cards back too, which a saved rectangle \
             could not do"
        );
    }

    #[test]
    fn r1609_a_sessions_reflow_is_a_difference_and_not_a_sum() {
        // Four presses that walk one card DOWN into another push that other card
        // once per press, so the per-press reflows sum to four displacements of a
        // card that moved a single time — 1>2, 2>3, 3>4, 4>5 — and an undo built
        // from that list has to know to collapse it.
        //
        // ★ The first draft walked the card UP and measured 1 against 1: moving
        // away from a card only ever collides with it once, so the fixture could
        // not tell the sum from the difference at all.
        let mut grid = TileGrid::new(4);
        for tile in [Tile::new("hand", 0, 0, 2, 1), Tile::new("post", 0, 1, 2, 1)] {
            grid.place(tile).unwrap();
        }
        let hand = TileId::new("hand");
        let edit = TileEdit::begin(&grid, &hand).unwrap();

        let mut summed = 0;
        for _ in 0..4 {
            summed += grid
                .nudge(&hand, TileNudge::Move(TileDirection::Down))
                .unwrap()
                .displaced()
                .len();
        }
        assert_eq!(summed, 4, "one displacement per press");

        let session = edit.reflow(&grid);
        assert_eq!(
            session.displaced().len(),
            1,
            "one card moved, so one displacement: {:?}",
            session.displaced()
        );
        assert_eq!(session.displaced()[0].id, TileId::new("post"));
        assert_eq!(session.displaced()[0].from, 1);
        assert_eq!(session.displaced()[0].to, tile_of(&grid, "post").row);
        assert!(
            summed > session.displaced().len(),
            "the sum over presses ({summed}) over-counts the difference (1)"
        );

        // And a card the session never touched is in neither answer.
        let untouched = TileEdit::begin(&grid, &hand).unwrap();
        assert!(untouched.reflow(&grid).is_clean());
        assert_eq!(
            TileEdit::begin(&grid, &TileId::new("ghost")),
            Err(TileError::NoSuchTile(TileId::new("ghost"))),
            "a session on a card that is not there is an undo point for nothing"
        );
    }

    #[test]
    fn r1609_every_other_card_lies_beyond_exactly_the_edges_it_should() {
        // ★ The theorem, asserted directly: `lies_beyond` is TOTAL over a legal
        // arrangement, because "does not overlap" IS "separated on some axis in
        // some direction". Run over a board that deliberately includes a card
        // sharing neither a row nor a column with another.
        let mut grid = dashboard();
        grid.place(Tile::new("island", 8, 4, 2, 1)).unwrap();
        assert!(grid.violations().is_empty());

        for a in grid.tiles() {
            for b in grid.tiles() {
                if a.id == b.id {
                    continue;
                }
                assert!(!a.overlaps(b));
                let sides: Vec<TileDirection> = TileDirection::ALL
                    .into_iter()
                    .filter(|dir| b.lies_beyond(a, *dir))
                    .collect();
                assert!(
                    !sides.is_empty(),
                    "{} lies beyond no edge of {} — a card no arrow key reaches",
                    b.id,
                    a.id
                );
            }
        }
    }

    #[test]
    fn r1609_arrow_navigation_prefers_the_band_and_reaches_every_card() {
        let mut grid = dashboard();
        grid.place(Tile::new("island", 8, 4, 2, 1)).unwrap();

        // Within a row band, Right is the neighbour in that band — not a card
        // further down that happens to start at a lower column.
        let right_of_left = grid
            .neighbour(&TileId::new("left"), TileDirection::Right)
            .expect("a card to the right");
        assert_eq!(right_of_left.id, TileId::new("right"));
        assert_eq!(
            grid.neighbour(&TileId::new("right"), TileDirection::Left)
                .map(|t| t.id.clone()),
            Some(TileId::new("left")),
            "and the reverse arrow comes back"
        );
        assert!(
            grid.neighbour(&TileId::new("header"), TileDirection::Up)
                .is_none(),
            "nothing lies above the top row"
        );
        assert!(
            grid.neighbour(&TileId::new("ghost"), TileDirection::Up)
                .is_none(),
            "an unknown card has no neighbours rather than panicking"
        );

        // Totality, driven: a walk over the four arrows from any single card
        // reaches all of them, `island` included — which is what the invariant
        // buys and what `activateNextSubWindow` cannot promise about direction.
        let start = grid.tiles()[0].id.clone();
        let mut seen = vec![start.clone()];
        let mut frontier = vec![start];
        while let Some(at) = frontier.pop() {
            for dir in TileDirection::ALL {
                if let Some(next) = grid.neighbour(&at, dir) {
                    let id = next.id.clone();
                    if !seen.contains(&id) {
                        seen.push(id.clone());
                        frontier.push(id);
                    }
                }
            }
        }
        assert_eq!(
            seen.len(),
            grid.tiles().len(),
            "unreachable by keyboard: {:?}",
            grid.tiles()
                .iter()
                .map(|t| t.id.as_str())
                .filter(|id| !seen.iter().any(|s| s.as_str() == *id))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn r1609_a_handle_drag_and_a_nudge_are_the_same_derivation() {
        // The round's claim, asserted rather than described: a `Grow` chord and a
        // one-cell handle drag on the same side reach the identical arrangement.
        for direction in TileDirection::ALL {
            let edge = direction.edge();
            let mut nudged = dashboard();
            let mut dragged = dashboard();
            let tall = TileId::new("tall");

            nudged.nudge(&tall, TileNudge::Grow(direction)).unwrap();

            let before = tile_of(&dragged, "tall");
            let line = edge.line_of(&before);
            let outward = if edge.is_start() {
                line.saturating_sub(1)
            } else {
                line + 1
            };
            // The cell the pointer would be over for that line, per the edge's
            // own half-open convention.
            let cell = if edge.is_start() {
                outward
            } else {
                outward - 1
            };
            let handle = match edge {
                TileEdge::Left => TileHandle::Left,
                TileEdge::Right => TileHandle::Right,
                TileEdge::Top => TileHandle::Top,
                TileEdge::Bottom => TileHandle::Bottom,
            };
            let (col, row) = if edge.is_horizontal() {
                (cell, before.row)
            } else {
                (before.col, cell)
            };
            dragged.drag_handle(&tall, handle, col, row).unwrap();

            assert_eq!(
                nudged, dragged,
                "Grow({direction:?}) and a one-cell {handle:?} drag differ"
            );
        }
    }
}
