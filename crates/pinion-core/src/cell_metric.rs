//! R968 §5.41 — cell-native coordinate metric.
//!
//! [`CellMetric`] is the typed substrate that replaces the pre-R968
//! `PIXEL_PER_CELL_*` free constants the TUI backend used. It is the
//! single source of truth for *how many logical pixels one terminal
//! cell spans* on each axis.
//!
//! Per the R968 ratify the metric is **node-local**: a future
//! `Scene::TextGrid` carries its own [`CellMetric`] (derived from the
//! grid's monospace font on the Vello backend via [`CellMetric::new`],
//! or `1 cell = 1 character cell` on the TUI backend), while the shared
//! [`crate::scene::Rect`] and pointer geometry stay in logical pixels.
//! Promotion of cells to a global `CoordSpace::Cell` is deferred until a
//! second cross-cutting consumer appears (YAGNI — a closed-enum global
//! coordinate space is not added speculatively).
//!
//! The conversion math lives here, on the metric, so both render
//! backends reuse one implementation rather than each re-deriving it:
//!
//! - [`CellMetric::cell_to_px`] — cell `(col, row)` → logical-pixel
//!   `(x, y)`. The forward direction shared by the TUI input mapping
//!   and Vello cell placement.
//! - [`CellMetric::cell_at`] — logical-pixel `(x, y)` → cell
//!   `(col, row)`, truncating toward the cell origin.
//!
//! The signed, `-∞`-flooring pixel→cell variant the TUI scroll cascade
//! needs (negative scrolled-out coordinates) stays in the TUI paint
//! adapter: the Vello backend clips in pixels and never needs it, so
//! lifting it here would be a single-consumer abstraction.
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
    /// `Scene::TextGrid` lands), keeping behaviour byte-unchanged.
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

    /// Logical-pixel `(x, y)` → cell `(col, row)`, truncating toward the
    /// cell origin (a pixel anywhere inside a cell maps to that cell).
    ///
    /// Saturates at [`u16::MAX`] so the result always fits a ratatui
    /// buffer coordinate. The non-zero axis invariant guarantees the
    /// division never traps.
    #[must_use]
    pub fn cell_at(self, px: u32, py: u32) -> (u16, u16) {
        let col = u16::try_from(px / self.cell_w).unwrap_or(u16::MAX);
        let row = u16::try_from(py / self.cell_h).unwrap_or(u16::MAX);
        (col, row)
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
    fn cell_at_truncates_into_owning_cell() {
        let m = CellMetric::DEFAULT;
        // exact origins
        assert_eq!(m.cell_at(0, 0), (0, 0));
        assert_eq!(m.cell_at(8, 16), (1, 1));
        assert_eq!(m.cell_at(24, 32), (3, 2));
        // a pixel inside a cell floors to that cell's origin
        assert_eq!(m.cell_at(7, 15), (0, 0));
        assert_eq!(m.cell_at(15, 31), (1, 1));
    }

    #[test]
    fn cell_at_saturates_at_u16_max() {
        let m = CellMetric::DEFAULT;
        let (col, row) = m.cell_at(u32::MAX, u32::MAX);
        assert_eq!((col, row), (u16::MAX, u16::MAX));
    }

    #[test]
    fn round_trips_cell_to_px_then_back() {
        // Font-independent: the metric is injected explicitly, no system
        // font is consulted. cell_to_px is exact on integer cell origins,
        // so cell_at recovers the originating cell for every in-range pair.
        let m = CellMetric::DEFAULT;
        for col in [0u16, 1, 3, 40, 79] {
            for row in [0u16, 1, 2, 23, 100] {
                let px = u32::from(col) * m.cell_w();
                let py = u32::from(row) * m.cell_h();
                assert_eq!(m.cell_at(px, py), (col, row));
            }
        }
    }

    #[test]
    fn round_trips_under_a_non_default_metric() {
        // The substrate must hold for any sourced metric, not just 8×16.
        let m = CellMetric::new(10, 20).expect("non-zero");
        let (x, y) = m.cell_to_px(5, 4);
        assert_eq!((x, y), (50.0, 80.0));
        assert_eq!(m.cell_at(50, 80), (5, 4));
        assert_eq!(m.cell_at(59, 99), (5, 4)); // interior floors to origin
    }
}
