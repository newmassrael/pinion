//! R968 §5.41 — cell-native coordinate metric.
//!
//! [`CellMetric`] is the typed substrate that replaces the pre-R968
//! `PIXEL_PER_CELL_*` free constants the TUI backend used. It is the
//! single source of truth for *how many logical pixels one terminal
//! cell spans* on each axis.
//!
//! Per the R968 ratify the metric is **node-local**: a
//! [`Scene::TextGrid`](crate::scene::Scene::TextGrid) carries its own
//! [`CellMetric`] (derived from the grid's monospace font on the Vello
//! backend via [`CellMetric::new`], or `1 cell = 1 character cell` on
//! the TUI backend), while the shared [`crate::scene::Rect`] and pointer
//! geometry stay in logical pixels. Promotion of cells to a global
//! `CoordSpace::Cell` is deferred until a second cross-cutting consumer
//! appears (YAGNI — a closed-enum global coordinate space is not added
//! speculatively).
//!
//! The conversion math lives here, on the metric, so both render
//! backends reuse one implementation rather than each re-deriving it:
//!
//! - [`CellMetric::cell_to_px`] — cell `(col, row)` → logical-pixel
//!   `(x, y)`. The forward direction shared by the TUI input mapping
//!   and (when cell rendering lands) Vello cell placement.
//! - [`CellMetric::cols_for`] / [`CellMetric::rows_for`] — the
//!   authoritative whole-cell `(cols, rows)` a grid of a given
//!   logical-pixel size holds: the R1.4 PTY-winsize dimensions a
//!   [`Scene::TextGrid`](crate::scene::Scene::TextGrid) reports from its
//!   layout-resolved rect.
//!
//! The signed, `-∞`-flooring pixel→cell variant the TUI scroll cascade
//! needs (negative scrolled-out coordinates) stays in the TUI paint
//! adapter: the Vello backend clips in pixels and never needs it, so
//! lifting it here would be a single-consumer abstraction. The unsigned
//! pixel→cell *hit-test* ([`CellMetric::px_to_cell`], R1.8) **landed in
//! R1008** when its forcing consumer arrived — `hello-grid-pointer`'s
//! pointer coordinator resolving a click to its `(col, row)` cell — sharing
//! the `whole_cells` floor SSOT with the winsize `cols_for` / `rows_for` so
//! a hit and the PTY dimensions can never round differently.
//!
//! [`CellMetric::DEFAULT`] is the behaviour-preserving 8×16 bitmap-font
//! baseline (the exact value the pre-R968 `PIXEL_PER_CELL_*` constants
//! carried), so existing TUI rendering is byte-unchanged.

/// Logical pixels spanned by one terminal cell, per axis.
///
/// Both axes are non-zero by construction ([`CellMetric::new`] rejects a
/// zero axis and [`CellMetric::DEFAULT`] is `8×16`); a cell cannot span
/// zero pixels and the conversion math would otherwise divide by zero.
/// The fields are private to preserve that invariant — read them via
/// [`CellMetric::cell_w`] / [`CellMetric::cell_h`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellMetric {
    cell_w: u32,
    cell_h: u32,
}

impl CellMetric {
    /// The 8×16 bitmap-font baseline — the exact value the pre-R968
    /// `PIXEL_PER_CELL_*` constants carried. Used wherever a real
    /// per-node metric is not yet sourced (the whole TUI backend until
    /// `Scene::TextGrid` cell rendering lands), keeping behaviour
    /// byte-unchanged.
    pub const DEFAULT: Self = Self {
        cell_w: 8,
        cell_h: 16,
    };

    /// Construct a metric from measured per-axis cell sizes — the R968
    /// font-derivation source hook (Vello: monospace advance width +
    /// line height). Returns `None` if either axis is zero, since a
    /// cell spanning zero pixels is degenerate and would divide by zero
    /// in the conversion math.
    #[must_use]
    pub const fn new(cell_w: u32, cell_h: u32) -> Option<Self> {
        if cell_w == 0 || cell_h == 0 {
            None
        } else {
            Some(Self { cell_w, cell_h })
        }
    }

    /// Logical pixels spanned by one cell column.
    #[must_use]
    pub const fn cell_w(self) -> u32 {
        self.cell_w
    }

    /// Logical pixels spanned by one cell row.
    #[must_use]
    pub const fn cell_h(self) -> u32 {
        self.cell_h
    }

    /// Cell `(col, row)` → logical-pixel `(x, y)` top-left origin.
    ///
    /// The forward direction shared by every backend. `f64` matches the
    /// logical-pixel coordinate axis the input router and Vello render
    /// target speak; the products are exact (`u16 × u32` fits f64's
    /// 53-bit mantissa).
    #[must_use]
    pub fn cell_to_px(self, col: u16, row: u16) -> (f64, f64) {
        (
            f64::from(col) * f64::from(self.cell_w),
            f64::from(row) * f64::from(self.cell_h),
        )
    }

    /// Grid-relative logical-pixel `(x_px, y_px)` → the cell `(col, row)`
    /// that pixel falls in — the **unsigned pixel→cell hit-test** (R1.8),
    /// the inverse of [`cell_to_px`](Self::cell_to_px). A pixel in
    /// `0..cell_w` is column 0, `cell_w..2·cell_w` column 1, and so on
    /// (floor); both axes saturate at [`u16::MAX`]. This is the geometry an
    /// xterm mouse-reporting terminal needs: a pointer click inside a
    /// [`Scene::TextGrid`](crate::scene::Scene::TextGrid) maps to the
    /// `(row, col)` cell the host forwards to the PTY. The caller passes
    /// **grid-relative** pixels (it subtracts the grid's
    /// [`Rect`](crate::scene::Rect) origin first); the router delivers a
    /// `(x_rel, y_rel)` fraction, so the caller reconstructs `x_rel ·
    /// width_px` before calling.
    ///
    /// The result is **unbounded** by design, mirroring
    /// [`cell_to_px`](Self::cell_to_px) (which maps an out-of-range cell
    /// past the rect): a pixel past the grid's right/bottom edge returns an
    /// index `>=` [`cols_for`](Self::cols_for) / [`rows_for`](Self::rows_for).
    /// A caller wanting a strictly in-grid hit clamps the result to the
    /// grid's `(cols, rows)` — the geometry primitive stays pure (the floor
    /// is the same `whole_cells` SSOT the [`cols_for`](Self::cols_for) /
    /// [`rows_for`](Self::rows_for) counts use, so a hit and the winsize count
    /// can never round differently).
    #[must_use]
    pub fn px_to_cell(self, x_px: u32, y_px: u32) -> (u16, u16) {
        (
            Self::whole_cells(x_px, self.cell_w),
            Self::whole_cells(y_px, self.cell_h),
        )
    }

    /// Whole cell columns spanning `width_px` logical pixels — the
    /// authoritative column count for a grid occupying that width (the
    /// R1.4 PTY-winsize authority a terminal host reports via
    /// `TIOCSWINSZ`). A trailing partial cell is not a usable column, so
    /// the count floors; saturates at [`u16::MAX`]. The non-zero axis
    /// invariant guarantees the division never traps.
    ///
    /// Consumed by [`TextGridNode::cols`](crate::scene::TextGridNode::cols):
    /// a grid derives its `(cols, rows)` from its layout-resolved pixel
    /// rect — the R969 one-directional `(rows, cols)` SSOT.
    #[must_use]
    pub fn cols_for(self, width_px: u32) -> u16 {
        Self::whole_cells(width_px, self.cell_w)
    }

    /// Whole cell rows spanning `height_px` logical pixels — the
    /// authoritative row count (the R1.4 PTY-winsize authority). Floors a
    /// trailing partial cell; saturates at [`u16::MAX`]. See
    /// [`CellMetric::cols_for`] for the rationale; consumed by
    /// [`TextGridNode::rows`](crate::scene::TextGridNode::rows).
    #[must_use]
    pub fn rows_for(self, height_px: u32) -> u16 {
        Self::whole_cells(height_px, self.cell_h)
    }

    /// Whole cells spanning `px` logical pixels at `cell` pixels per cell
    /// — the single home of the winsize floor+saturate policy: a trailing
    /// partial cell is not a usable cell (floor), and the count saturates
    /// at [`u16::MAX`] so it always fits a ratatui buffer coordinate.
    /// Shared by [`Self::cols_for`] / [`Self::rows_for`] so the two axes
    /// can never drift to a different rounding — the R971 "one formula,
    /// not parallel copies" SSOT (restored R972.1: the R972 `cell_at`
    /// removal had briefly inlined it into both axes). The non-zero axis
    /// invariant guarantees the division never traps.
    fn whole_cells(px: u32, cell: u32) -> u16 {
        u16::try_from(px / cell).unwrap_or(u16::MAX)
    }
}

impl Default for CellMetric {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::CellMetric;

    #[test]
    fn default_is_8x16_baseline() {
        let m = CellMetric::DEFAULT;
        assert_eq!(m.cell_w(), 8);
        assert_eq!(m.cell_h(), 16);
        assert_eq!(CellMetric::default(), CellMetric::DEFAULT);
    }

    #[test]
    fn new_rejects_zero_axis() {
        assert!(CellMetric::new(0, 16).is_none());
        assert!(CellMetric::new(8, 0).is_none());
        assert!(CellMetric::new(0, 0).is_none());
        assert_eq!(CellMetric::new(8, 16), Some(CellMetric::DEFAULT));
        assert!(CellMetric::new(9, 18).is_some());
    }

    #[test]
    fn cell_to_px_scales_per_axis() {
        let m = CellMetric::DEFAULT;
        assert_eq!(m.cell_to_px(0, 0), (0.0, 0.0));
        assert_eq!(m.cell_to_px(3, 2), (24.0, 32.0));
    }

    #[test]
    fn cell_to_px_scales_under_a_non_default_metric() {
        // The forward map must hold for any sourced metric, not just 8×16.
        let m = CellMetric::new(10, 20).expect("non-zero");
        assert_eq!(m.cell_to_px(5, 4), (50.0, 80.0));
    }

    #[test]
    fn cols_and_rows_for_floor_whole_cells() {
        let m = CellMetric::DEFAULT; // 8x16
        assert_eq!(m.cols_for(0), 0);
        assert_eq!(m.cols_for(7), 0); // partial first cell is not usable
        assert_eq!(m.cols_for(8), 1); // exact
        assert_eq!(m.cols_for(639), 79); // 639 / 8 = 79.875 -> 79
        assert_eq!(m.cols_for(640), 80); // exact 80 columns
        assert_eq!(m.rows_for(15), 0);
        assert_eq!(m.rows_for(16), 1);
        assert_eq!(m.rows_for(384), 24); // 384 / 16 = 24 rows
    }

    #[test]
    fn cols_and_rows_for_under_a_non_default_metric() {
        let m = CellMetric::new(10, 20).expect("non-zero");
        assert_eq!(m.cols_for(100), 10);
        assert_eq!(m.cols_for(105), 10); // trailing 5px partial floors
        assert_eq!(m.rows_for(80), 4);
        assert_eq!(m.rows_for(99), 4); // trailing 19px partial floors
    }

    #[test]
    fn cols_and_rows_for_saturate_at_u16_max() {
        let m = CellMetric::DEFAULT;
        assert_eq!(m.cols_for(u32::MAX), u16::MAX);
        assert_eq!(m.rows_for(u32::MAX), u16::MAX);
    }

    #[test]
    fn px_to_cell_floors_pixel_into_its_cell() {
        let m = CellMetric::DEFAULT; // 8x16
        assert_eq!(m.px_to_cell(0, 0), (0, 0));
        assert_eq!(
            m.px_to_cell(7, 15),
            (0, 0),
            "any pixel inside cell (0,0) maps to it"
        );
        assert_eq!(
            m.px_to_cell(8, 16),
            (1, 1),
            "the first pixel of the next cell"
        );
        assert_eq!(m.px_to_cell(24, 32), (3, 2)); // inverse of cell_to_px(3, 2) = (24, 32)
        assert_eq!(
            m.px_to_cell(31, 47),
            (3, 2),
            "trailing pixels still inside cell (3,2)"
        );
    }

    #[test]
    fn px_to_cell_is_the_inverse_of_cell_to_px() {
        // For every cell, its top-left pixel and any interior pixel map back to it.
        let m = CellMetric::new(10, 20).expect("non-zero");
        for col in 0..8u16 {
            for row in 0..8u16 {
                let (x, y) = m.cell_to_px(col, row);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (x0, y0) = (x as u32, y as u32);
                assert_eq!(
                    m.px_to_cell(x0, y0),
                    (col, row),
                    "top-left of cell ({col},{row})"
                );
                // The last pixel still inside the cell (origin + size - 1).
                assert_eq!(
                    m.px_to_cell(x0 + 9, y0 + 19),
                    (col, row),
                    "interior of ({col},{row})"
                );
            }
        }
    }

    #[test]
    fn px_to_cell_saturates_at_u16_max() {
        let m = CellMetric::DEFAULT;
        assert_eq!(m.px_to_cell(u32::MAX, u32::MAX), (u16::MAX, u16::MAX));
    }

    #[test]
    fn px_to_cell_past_the_grid_edge_is_unbounded() {
        // A pixel past a 80x24 (640x384) grid's edge returns an index >= the
        // winsize count; the caller clamps to (cols, rows) for a strict hit.
        let m = CellMetric::DEFAULT;
        let (cols, rows) = (m.cols_for(640), m.rows_for(384)); // 80, 24
        let (c, r) = m.px_to_cell(640, 384); // exactly one cell past each edge
        assert_eq!((c, r), (80, 24));
        assert!(
            c >= cols && r >= rows,
            "off-grid pixel exceeds the cell counts"
        );
    }
}
