//! R1607 §5.21 — **a dashboard's tiles are a value**, and the layout it needs
//! already existed.
//!
//! The analyzer-tool audit's plane-C gap was a Grafana-class tile dashboard:
//! independent cards on a fixed column grid, dragged, snapped and resized, with
//! the arrangement saved under a name. pinion's docking is Qt's `QDockWidget`
//! model — a splitter tree with tabs and tear-off — and that is a **different
//! thing**: a splitter tree is nested bisection, so its freedom is bounded by the
//! tree's shape, and widening one card re-divides its neighbours.
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
//! ## Past Grafana
//!
//! Grafana pushes displaced panels down and **says nothing**; its layout runs
//! as a side effect of the drag. Two choices here differ:
//!
//! * **A move reports what it displaced.** [`Reflow`] names every tile that
//!   moved and where it came from, which is what an undo record, a "your layout
//!   changed" notice, and an agent driving the dashboard over the wire all need.
//! * **Compaction is a verb, not a consequence.** Grafana floats tiles up to
//!   close gaps automatically, so a drag has a non-local effect nobody asked
//!   for and its inverse is not a drag. [`TileGrid::compact`] is a separate
//!   call, so an editor decides whether "tidy up" is part of the gesture.

use serde::{Deserialize, Serialize};

use crate::style::GridPlacement;

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

impl std::fmt::Display for TileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One card's slot: a rectangle of grid cells.
///
/// **Zero-based**, like Grafana's `gridPos`, because a model that counts from
/// zero and a CSS grid that counts from one is one conversion — and it happens
/// in exactly one place ([`TileGrid::placement`]) rather than at every call
/// site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tile {
    /// Whose tile.
    pub id: TileId,
    /// Zero-based column of the left edge.
    pub col: u32,
    /// Zero-based row of the top edge.
    pub row: u32,
    /// Width in columns. Never zero.
    pub w: u32,
    /// Height in rows. Never zero.
    pub h: u32,
}

impl Tile {
    /// A tile at `(col, row)` covering `w` x `h` cells.
    #[must_use]
    pub fn new(id: impl Into<String>, col: u32, row: u32, w: u32, h: u32) -> Self {
        Self {
            id: TileId::new(id),
            col,
            row,
            w,
            h,
        }
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
}

/// What a move or a resize did to the tiles it landed on.
///
/// Grafana reflows silently; this is the record of it. Empty means the gesture
/// fit where it was put.
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
        }
    }
}

impl std::error::Error for TileError {}

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

    /// The tile with that id.
    #[must_use]
    pub fn tile(&self, id: &TileId) -> Option<&Tile> {
        self.tiles.iter().find(|t| &t.id == id)
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
        tile.col = tile.col.min(self.columns - tile.w);
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
        let columns = self.columns;
        let target = self
            .tiles
            .iter_mut()
            .find(|t| &t.id == id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        target.col = col.min(columns - target.w);
        target.row = row;
        Ok(self.reflow_around(id))
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
        let target = self
            .tiles
            .iter_mut()
            .find(|t| &t.id == id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        target.w = w;
        target.h = h;
        target.col = target.col.min(columns - w);
        Ok(self.reflow_around(id))
    }

    /// Take a tile out. The gap it leaves stays a gap until [`Self::compact`].
    ///
    /// # Errors
    ///
    /// [`TileError::NoSuchTile`].
    pub fn remove(&mut self, id: &TileId) -> Result<Tile, TileError> {
        let at = self
            .tiles
            .iter()
            .position(|t| &t.id == id)
            .ok_or_else(|| TileError::NoSuchTile(id.clone()))?;
        Ok(self.tiles.remove(at))
    }

    /// Float every tile as far up as it will go, closing gaps.
    ///
    /// **A verb rather than a consequence.** Grafana does this after every drag,
    /// so a gesture moves tiles the user did not touch and its inverse is not a
    /// gesture; here an editor chooses whether tidying is part of the drag, and
    /// the tiles that moved are named either way.
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
            "★ Grafana reflows silently; every tile that moved is named here"
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

    #[test]
    fn a_degenerate_column_count_is_clamped_rather_than_making_a_gridless_grid() {
        let mut grid = TileGrid::new(0);
        assert_eq!(grid.columns(), 1);
        grid.place(Tile::new("only", 5, 0, 1, 1)).unwrap();
        assert_eq!(grid.tile(&TileId::new("only")).unwrap().col, 0);
    }
}
