//! R785 §5.27 — **per-column width model for a Model/View data grid**.
//!
//! R775–R784 land a virtualized data grid (sort / filter / select / multi /
//! keyboard nav + horizontal scroll). Every column was a uniform
//! [`TableStyle::col_width`](crate::widgets::table) — fine for a uniform
//! grid, but a DCC / IDE inspector sizes each column to its content and lets
//! the user (or an AI agent) widen the one they care about. This module adds
//! the **column-width axis**: a reactive `Vec<u32>` of per-column widths,
//! held once in a [`ColumnWidths`] (the
//! [`ScrollState`](crate::widgets::scroll::ScrollState) /
//! [`GridSortState`](crate::widgets::grid_sort::GridSortState) reactive-holder
//! pattern this crate shares interactive axes with), read by the paint layer
//! and the a11y tree through [`use_column_widths`], and mutated through the
//! [`ColumnWidthExternal`] `invoke` / `intervene` channels.
//!
//! Column widths are **orthogonal** to sort / selection (a re-sort does not
//! change a column's width; widening a column does not re-order rows) — a
//! separate reactive holder + `External`, exactly as sort ⊥ selection are
//! separate proxies (R778). Resizing widens the grid's content past the
//! viewport, so the R784 horizontal scroll engages: the two axes compose
//! (`max_x` grows as columns widen).
//!
//! Column-resize **undo** is the optionally-undoable-mutation axis deferred
//! for the proxies (R779.1 carry) — out of this slice; the holder mutates
//! directly. The live-drag **resize handle** is the pointer-wire consumer
//! (the R780 → R781 model-then-interaction split): this round lands the model
//! + the AI-first RPC path; a clicked-and-dragged column border is a follow-up.

use std::rc::Rc;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::reactive::{Owner, Signal};

/// Default minimum column width (logical pixels). A [`set_width`](ColumnWidths::set_width)
/// below this clamps up, so a column can never be dragged / set to a sliver
/// that hides its content or vanishes entirely.
pub const DEFAULT_MIN_COL_WIDTH: u32 = 40;

/// R785 §5.27 — reactive per-column widths for one data grid.
///
/// One instance corresponds to one logical grid; [`use_column_widths`] gives
/// it a scope-id-keyed home on the [`Owner::cache`](crate::reactive::Owner::cache)
/// substrate so the view, the a11y tree, and the [`ColumnWidthExternal`] all
/// reach the same `Rc`. [`width`](Self::width) / [`widths`](Self::widths) /
/// [`total`](Self::total) subscribe when read inside a view-fn, so a
/// [`set_width`](Self::set_width) repaints every subscribed view.
#[derive(Debug)]
pub struct ColumnWidths {
    tag: Option<&'static str>,
    /// Per-column widths in logical pixels; `widths.len()` is the column count.
    widths: Signal<Vec<u32>>,
    /// Lower clamp applied on every width write.
    min_width: u32,
}

impl ColumnWidths {
    /// Construct over the given initial per-column widths. The column count is
    /// `widths.len()`. Uses [`DEFAULT_MIN_COL_WIDTH`] as the resize floor.
    #[must_use]
    pub fn new(widths: Vec<u32>) -> Self {
        Self { tag: None, widths: Signal::new(widths), min_width: DEFAULT_MIN_COL_WIDTH }
    }

    /// As [`new`](Self::new) but records the [`use_column_widths`] cache key,
    /// for symmetry with
    /// [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, widths: Vec<u32>) -> Self {
        Self { tag: Some(key), ..Self::new(widths) }
    }

    /// Override the minimum-width clamp (builder form).
    #[must_use]
    pub fn with_min_width(mut self, min_width: u32) -> Self {
        self.min_width = min_width;
        self
    }

    /// The [`use_column_widths`] cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// The resize floor (a width write clamps up to this).
    #[must_use]
    pub fn min_width(&self) -> u32 {
        self.min_width
    }

    /// Column count. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn col_count(&self) -> usize {
        self.widths.get().len()
    }

    /// Width of column `col` in logical pixels, or [`min_width`](Self::min_width)
    /// when out of range. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn width(&self, col: usize) -> u32 {
        self.widths.get().get(col).copied().unwrap_or(self.min_width)
    }

    /// A snapshot of every column width (cheap `Vec` clone). Subscribes when
    /// read inside a view-fn, so the paint layer re-runs on any width change.
    #[must_use]
    pub fn widths(&self) -> Vec<u32> {
        self.widths.get()
    }

    /// Sum of all column widths — the grid's intrinsic content width (what the
    /// R784 horizontal scroll measures against). Subscribes.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.widths.get().iter().copied().sum()
    }

    /// Set column `col`'s width, clamping up to [`min_width`](Self::min_width).
    /// A `Signal` write repaints every view that read a width. An out-of-range
    /// `col` is a silent no-op. Returns the resulting (clamped) width so the
    /// AI-first `set_col_width` path reports the applied value in one
    /// round-trip (the setter-returns-read-outcome contract) — `0` when the
    /// column is out of range.
    #[allow(
        clippy::must_use_candidate,
        reason = "the returned clamped width is the read-outcome the AI-first \
                  set_col_width path reports back in one round-trip; a \
                  fire-and-forget caller (a future drag handler) legitimately \
                  ignores it, so forcing `let _ = …` punishes that case. \
                  Mirrors ScrollState::set_max."
    )]
    pub fn set_width(&self, col: usize, width: u32) -> u32 {
        let clamped = width.max(self.min_width);
        let mut applied = 0;
        self.widths.set_with(|w| {
            let mut next = w.clone();
            if let Some(slot) = next.get_mut(col) {
                *slot = clamped;
                applied = clamped;
            }
            next
        });
        applied
    }

    /// Replace all column widths at once (admin / restore), clamping each up to
    /// [`min_width`](Self::min_width). Keeps the column count of the supplied
    /// vector — a caller restoring a malformed width set is its own concern.
    pub fn set_widths(&self, widths: Vec<u32>) {
        let min = self.min_width;
        self.widths.set(widths.into_iter().map(|w| w.max(min)).collect());
    }
}

/// R785 §5.27 — resolve the shared [`ColumnWidths`] for `key`, building it once
/// via `widths` (the initial per-column widths). Mirrors
/// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) /
/// [`use_grid_sort`](crate::widgets::grid_sort::use_grid_sort): the `External`
/// and the view both call this with the same `key` and receive the same `Rc`,
/// so the widths are one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_column_widths(
    key: &'static str,
    widths: impl FnOnce() -> Vec<u32>,
) -> Rc<ColumnWidths> {
    Owner::current()
        .expect("use_column_widths requires an active Owner scope")
        .cache(key, || ColumnWidths::with_tag(key, widths()))
}

/// Format the per-column widths as the comma-separated wire string
/// (`"130,90,200"`) the `widths` introspect slot reports and accepts.
#[must_use]
pub fn widths_str(widths: &[u32]) -> String {
    widths.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

/// Parse the comma-separated wire string (`"130,90,200"`) back into widths.
/// A malformed entry is skipped; an all-malformed string yields an empty vec,
/// which [`ColumnWidths::set_widths`] treats as a zero-column restore.
#[must_use]
pub fn widths_from_str(s: &str) -> Vec<u32> {
    s.split(',')
        .filter_map(|t| t.trim().parse::<u32>().ok())
        .collect()
}

/// Parse a `"<col>=<width>"` invoke payload into `(col, width)`.
fn parse_col_width(s: &str) -> Option<(usize, u32)> {
    let (col, width) = s.split_once('=')?;
    Some((col.trim().parse().ok()?, width.trim().parse().ok()?))
}

/// R785 §5.27 — thin `External` adapter over a shared [`ColumnWidths`], the
/// AI-first mutation surface for the column-width axis. A config holder (no
/// §5.20 intent): the width `Signal` write already repaints every subscribed
/// view, mirroring [`GridSortExternal`](crate::widgets::grid_sort::GridSortExternal).
#[derive(Debug)]
pub struct ColumnWidthExternal {
    state: Rc<ColumnWidths>,
}

impl ColumnWidthExternal {
    /// Wrap the shared [`ColumnWidths`] (from [`use_column_widths`]).
    #[must_use]
    pub fn new(state: Rc<ColumnWidths>) -> Self {
        Self { state }
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_column_widths`]).
    #[must_use]
    pub fn state(&self) -> &Rc<ColumnWidths> {
        &self.state
    }
}

impl External for ColumnWidthExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for ColumnWidthExternal {
    fn schema(&self) -> IntrospectSchema {
        // `widths`        — comma-separated per-column widths (query + intervene).
        // `total`         — sum of widths = content width (query only).
        // `cols`          — column count (query only).
        // `min_width`     — the resize floor (query only).
        // `width.<col>`   — one column's width (query only).
        // `set_col_width` — `"<col>=<width>"` invoke; returns the applied width.
        IntrospectSchema::new(&[
            ("widths", "string"),
            ("total", "int"),
            ("cols", "int"),
            ("min_width", "int"),
            ("width", "int"),
            ("set_col_width", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // `width.<col>` reads one column's width; an out-of-range column
        // reports its width as the min clamp (present-but-floored), never
        // absence.
        if let Some(rest) = path.strip_prefix("width.") {
            let col: usize = rest.parse().ok()?;
            return Some(IntrospectValue::Int(i64::from(self.state.width(col))));
        }
        match path {
            "widths" => Some(IntrospectValue::Text(widths_str(&self.state.widths()))),
            "total" => Some(IntrospectValue::Int(i64::from(self.state.total()))),
            "cols" => Some(IntrospectValue::Int(
                i64::try_from(self.state.col_count()).unwrap_or(i64::MAX),
            )),
            "min_width" => Some(IntrospectValue::Int(i64::from(self.state.min_width()))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: set the whole width vector from its CSV string.
            "widths" => match value {
                IntrospectValue::Text(ref s) => {
                    self.state.set_widths(widths_from_str(s));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "total" | "cols" | "min_width" | "width" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first column resize: a `"<col>=<width>"` payload sets that
            // column's width (clamped up to `min_width`) and returns the
            // applied width in one round-trip, so the agent learns the clamped
            // value without a follow-up query.
            "set_col_width" => match args {
                IntrospectValue::Text(ref s) => {
                    let (col, width) = parse_col_width(s).ok_or(InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Int(i64::from(self.state.set_width(col, width))))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths() -> ColumnWidths {
        ColumnWidths::new(vec![130, 90, 200])
    }

    #[test]
    fn width_and_total_read_back() {
        let w = widths();
        assert_eq!(w.col_count(), 3);
        assert_eq!(w.width(0), 130);
        assert_eq!(w.width(2), 200);
        assert_eq!(w.total(), 420);
        // Out-of-range column reads the min clamp, not a panic.
        assert_eq!(w.width(9), DEFAULT_MIN_COL_WIDTH);
    }

    #[test]
    fn set_width_clamps_to_min_and_returns_applied() {
        let w = widths();
        assert_eq!(w.set_width(1, 250), 250, "in-range set returns the applied width");
        assert_eq!(w.width(1), 250);
        assert_eq!(w.total(), 130 + 250 + 200);
        // Below the floor clamps up.
        assert_eq!(w.set_width(0, 5), DEFAULT_MIN_COL_WIDTH, "sub-min clamps to the floor");
        assert_eq!(w.width(0), DEFAULT_MIN_COL_WIDTH);
        // Out-of-range column is a no-op, returns 0.
        assert_eq!(w.set_width(9, 100), 0, "out-of-range set is a no-op");
        assert_eq!(w.col_count(), 3, "no phantom column appended");
    }

    #[test]
    fn with_min_width_overrides_floor() {
        let w = ColumnWidths::new(vec![100, 100]).with_min_width(80);
        assert_eq!(w.set_width(0, 10), 80, "custom floor applied");
    }

    #[test]
    fn widths_wire_round_trips() {
        assert_eq!(widths_str(&[130, 90, 200]), "130,90,200");
        assert_eq!(widths_from_str("130,90,200"), vec![130, 90, 200]);
        // Malformed entries are skipped.
        assert_eq!(widths_from_str("130, ,abc,90"), vec![130, 90]);
    }

    #[test]
    fn parse_col_width_payload() {
        assert_eq!(parse_col_width("2=180"), Some((2, 180)));
        assert_eq!(parse_col_width("bad"), None);
        assert_eq!(parse_col_width("2=x"), None);
    }

    #[test]
    fn external_invoke_set_col_width_returns_clamped() {
        let state = Rc::new(widths());
        let mut ext = ColumnWidthExternal::new(Rc::clone(&state));
        let out = ext.invoke("set_col_width", IntrospectValue::Text("1=300".into())).unwrap();
        assert_eq!(out, IntrospectValue::Int(300));
        assert_eq!(state.width(1), 300);
        // Sub-min clamps and the return reports the clamped value.
        let out = ext.invoke("set_col_width", IntrospectValue::Text("1=5".into())).unwrap();
        assert_eq!(out, IntrospectValue::Int(i64::from(DEFAULT_MIN_COL_WIDTH)));
    }

    #[test]
    fn external_query_exposes_widths_total_cols() {
        let ext = ColumnWidthExternal::new(Rc::new(widths()));
        assert_eq!(ext.query("widths"), Some(IntrospectValue::Text("130,90,200".into())));
        assert_eq!(ext.query("total"), Some(IntrospectValue::Int(420)));
        assert_eq!(ext.query("cols"), Some(IntrospectValue::Int(3)));
        assert_eq!(ext.query("width.2"), Some(IntrospectValue::Int(200)));
        assert_eq!(ext.query("min_width"), Some(IntrospectValue::Int(40)));
        assert_eq!(ext.query("nope"), None);
    }

    #[test]
    fn external_intervene_widths_restores_whole_vector() {
        let state = Rc::new(widths());
        let mut ext = ColumnWidthExternal::new(Rc::clone(&state));
        ext.intervene("widths", IntrospectValue::Text("60,60,60,60".into())).unwrap();
        assert_eq!(state.col_count(), 4, "restore replaces the whole vector");
        assert_eq!(state.total(), 240);
        assert_eq!(ext.intervene("total", IntrospectValue::Int(9)), Err(InterveneError::ReadOnly));
    }
}
