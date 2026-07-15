//! R785 §5.27 — **per-column width model for a Model/View data grid**.
//!
//! R775–R784 land a virtualized data grid (sort / filter / select / multi /
//! keyboard nav + horizontal scroll). Every column was a uniform
//! [`TableStyle::col_width`](crate::widgets::table) — fine for a uniform
//! grid, but a DCC / IDE inspector sizes each column to its content and lets
//! the user (or an AI agent) widen the one they care about. This module adds
//! the **column-width axis**: a reactive `Vec<u32>` of per-column widths,
//! held once in a [`ColumnWidths`] (the
//! [`ScrollState`] /
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

use std::borrow::Cow;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::composite_tag::split_send_payload;
use crate::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::input::{DragCalibration, PointerWireEvent};
use crate::intent::Intent;
use crate::reactive::{Owner, Signal};
use crate::widget_core::ExtraExternal;
use crate::widgets::scroll::ScrollState;

/// Default minimum column width (logical pixels). A [`set_width`](ColumnWidths::set_width)
/// below this clamps up, so a column can never be dragged / set to a sliver
/// that hides its content or vanishes entirely.
pub const DEFAULT_MIN_COL_WIDTH: u32 = 40;

/// (R785.1) The single clamp the floor invariant flows through — every entry
/// raised to at least `min`. Shared by construction
/// ([`ColumnWidths::new`]), floor change ([`ColumnWidths::with_min_width`]),
/// and whole-vector restore ([`ColumnWidths::set_widths`]) so the
/// "width ≥ `min_width`" invariant has one enforcement site.
fn clamp_widths(widths: Vec<u32>, min: u32) -> Vec<u32> {
    widths.into_iter().map(|w| w.max(min)).collect()
}

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
    /// Construct over the given initial per-column widths, **clamped up to**
    /// [`DEFAULT_MIN_COL_WIDTH`]. The column count is `widths.len()`.
    ///
    /// (R785.1 audit-correction) The "every width ≥ `min_width`" invariant
    /// holds from construction, not just after a mutation — clamping the
    /// initial widths the same way [`set_width`](Self::set_width) /
    /// [`set_widths`](Self::set_widths) do (the Qt / AG-Grid contract: a
    /// column can never be *narrower* than its minimum, however it was sized).
    #[must_use]
    pub fn new(widths: Vec<u32>) -> Self {
        let widths = clamp_widths(widths, DEFAULT_MIN_COL_WIDTH);
        Self {
            tag: None,
            widths: Signal::new(widths),
            min_width: DEFAULT_MIN_COL_WIDTH,
        }
    }

    /// As [`new`](Self::new) but records the [`use_column_widths`] cache key,
    /// for symmetry with
    /// [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, widths: Vec<u32>) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(widths)
        }
    }

    /// Override the minimum-width clamp (builder form), **re-clamping** any
    /// already-stored width up to the new floor so the "width ≥ `min_width`"
    /// invariant holds after the floor moves too (R785.1 audit-correction).
    #[must_use]
    pub fn with_min_width(mut self, min_width: u32) -> Self {
        self.min_width = min_width;
        // `set_with` reads the current vector by reference (no subscription)
        // and equality-skips, so a no-op floor change does not churn.
        self.widths
            .set_with(|w| w.iter().map(|&x| x.max(min_width)).collect());
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
        self.widths
            .get()
            .get(col)
            .copied()
            .unwrap_or(self.min_width)
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
        self.widths.set(clamp_widths(widths, self.min_width));
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
pub fn use_column_widths(key: &'static str, widths: impl FnOnce() -> Vec<u32>) -> Rc<ColumnWidths> {
    Owner::current()
        .expect("use_column_widths requires an active Owner scope")
        .cache(key, || ColumnWidths::with_tag(key, widths()))
}

/// Format the per-column widths as the comma-separated wire string
/// (`"130,90,200"`) the `widths` introspect slot reports and accepts.
#[must_use]
pub fn widths_str(widths: &[u32]) -> String {
    widths
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
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

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first column resize: a `"<col>=<width>"` payload sets that
            // column's width (clamped up to `min_width`) and returns the
            // applied width in one round-trip, so the agent learns the clamped
            // value without a follow-up query.
            "set_col_width" => match args {
                IntrospectValue::Text(ref s) => {
                    let (col, width) = parse_col_width(s).ok_or(InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Int(i64::from(
                        self.state.set_width(col, width),
                    )))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ============================================================================
// R786 §5.27 — live-drag column resize (the pointer-wire consumer)
// ============================================================================

/// Clamp a logical-pixel width (computed in `f64`) into the `u32` width domain.
/// [`ColumnWidths::set_width`] then raises the result to `min_width`, so the
/// only job here is to keep the cast in range (a drag dragged far left yields a
/// negative width → `0` → floored to the minimum).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "px is clamped to [0, u32::MAX] before the cast; set_width then \
              raises sub-min results to min_width, so neither truncation nor a \
              negative input can reach the column model"
)]
fn px_to_width(px: f64) -> u32 {
    px.clamp(0.0, f64::from(u32::MAX)) as u32
}

/// R786 §5.27 — the **live-drag pointer-wire consumer** of the R785
/// [`ColumnWidths`] model: one `External` per resizable column border (the
/// R780 → R781 model-then-interaction split's interaction half — R785 landed the
/// model + the AI-first `set_col_width` RPC path, this is the clicked-and-
/// dragged border).
///
/// It is the column analogue of
/// [`SplitterExternal`](../../../pinion_widget_paint/splitter/struct.SplitterExternal.html):
/// `wants_pointer_capture` opts into the R51.34 capture lock so a drag survives
/// the cursor straying past the column edge, and
/// [`capture_normalize`](External::capture_normalize) names the grid
/// **viewport** (the horizontal scroll node) as the normalization rect — not the
/// grabbed cell. The cell is what the drag resizes, so its width moves every
/// frame; the viewport does not resize when a column does, so it is the stable
/// pixel reference (exactly as the splitter normalizes against its stable pane
/// container, not the moving handle). The cursor-fraction delta across that
/// fixed-width viewport is the pixel travel: `new = width_at_press + (x_rel −
/// press_x_rel) · viewport_w`, where `viewport_w` is read from the same shared
/// [`ScrollState`] the grid scrolls with. This holds for both the batched
/// `scene/drag` arc (paint frozen mid-RPC) and a live native drag (paint
/// re-runs each frame), because the basis never moves.
///
/// The grabber strip the [`view_virtual_table`](../../../pinion_widget_paint/table/fn.view_virtual_table.html)
/// header paints is tagged `"<tag>_ch<col>#resize"`, so the router's `'#'`-split
/// routes its capture to the external registered at the primary `"<tag>_ch<col>"`
/// — exactly the per-column tag this `External` is registered under (see
/// [`column_resize_externals`]). `PointerUp` / `PointerCancel` arrive through
/// the `invoke("send", …)` channel and clear the calibration so the next drag
/// recalibrates.
pub struct ColumnResizeExternal {
    state: Rc<ColumnWidths>,
    col: usize,
    /// The grid's horizontal [`ScrollState`]; its measured viewport width is the
    /// stable pixel reference the cursor-fraction delta scales by.
    h_scroll: Rc<ScrollState>,
    /// The viewport tag the captured cursor is normalized against (the
    /// horizontal scroll node's tag) — see [`External::capture_normalize`].
    viewport_tag: Cow<'static, str>,
    /// R914 — the press-anchored capture-drag calibration ([`DragCalibration`]):
    /// the first move snapshots the column's `width_at_press` and the cursor's
    /// anchor fraction, each later move yields the fraction delta the pixel
    /// travel scales from. Held until `PointerUp` tears it down.
    resize: DragCalibration<u32>,
    /// R1347 §5.20 — pending intents for the `"width_committed"` drag-end
    /// channel, the column analogue of the splitter's `"ratio_committed"`
    /// (R1346). Plain `VecDeque`, not `RefCell<VecDeque<_>>`: every enqueue
    /// reaches here from `invoke(&mut self)`, so there is no `&self` producer
    /// and no borrow-panic surface (the same call the dock Externals need
    /// interior mutability for — `begin_drag(&self)` — this widget does not
    /// have).
    pending_intents: VecDeque<Intent>,
}

impl core::fmt::Debug for ColumnResizeExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ColumnResizeExternal")
            .field("col", &self.col)
            .field("viewport_tag", &self.viewport_tag)
            .field("is_dragging", &self.is_dragging())
            .finish_non_exhaustive()
    }
}

impl ColumnResizeExternal {
    /// Wrap the shared [`ColumnWidths`] for the column at `col`, the grid's
    /// horizontal [`ScrollState`] (the stable pixel reference), and the
    /// `viewport_tag` the cursor normalizes against (the horizontal scroll
    /// node's tag). The same `ColumnWidths` `Rc` the view reads via
    /// [`use_column_widths`] and the [`ColumnWidthExternal`] mutates, so a drag
    /// and an RPC `set_col_width` are one source of truth.
    #[must_use]
    pub fn new(
        state: Rc<ColumnWidths>,
        col: usize,
        h_scroll: Rc<ScrollState>,
        viewport_tag: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            state,
            col,
            h_scroll,
            viewport_tag: viewport_tag.into(),
            resize: DragCalibration::new(),
            pending_intents: VecDeque::new(),
        }
    }

    /// The column index this handle resizes.
    #[must_use]
    pub fn col(&self) -> usize {
        self.col
    }

    /// `true` between the press-time calibration frame and the `PointerUp`
    /// teardown (diagnostic / a future drag-highlight surface).
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.resize.is_active()
    }

    /// Reset the calibration snapshot **silently** — the `PointerCancel` half
    /// of the release pair. The width the in-flight drag already applied to the
    /// shared model stays applied (the R51.93 §5.35 family invariant: the OS
    /// revoked the *commit* signal, not the user's in-flight updates; width is a
    /// continuous-domain sidecar, not a commit-bound enum). A binding that
    /// persists only on [`Self::commit_drag`] therefore keeps the pre-drag
    /// width on disk, which is what "the resize was aborted" should mean.
    fn clear_drag(&self) {
        self.resize.end();
    }

    /// R1347 §5.20 — the `PointerUp` half: tear the calibration down and, when
    /// the column actually settled on a **new** width, queue a
    /// `"width_committed"` intent carrying it (`IntrospectValue::Int`).
    ///
    /// This is the column peer of the splitter's `commit_drag_state` (R1346)
    /// and carries the identical subtlety: the gate is "did the width change
    /// since the press", NOT [`DragCalibration::end`]'s bool. On a press over a
    /// capture widget the router forwards a press-time `pointer_move` to that
    /// widget (R51.35), so `end()` is `true` for a bare click on the grabber —
    /// gating on it would emit a spurious persist write for a click that
    /// resized nothing. So compare the settled width against the
    /// [`DragCalibration::end_payload`] press snapshot and stay silent when they
    /// agree (a click, a drag that returned home, a drag pinned at `min_width`).
    ///
    /// `&mut self` because the queue push needs it; reached only from
    /// `invoke(&mut self)`, so no interior mutability is required.
    fn commit_drag(&mut self) {
        let Some(width_at_press) = self.resize.end_payload() else {
            return;
        };
        let settled = self.state.width(self.col);
        if settled == width_at_press {
            return;
        }
        self.pending_intents.push_back(Intent::new_static(
            "width_committed",
            IntrospectValue::Int(i64::from(settled)),
        ));
    }
}

impl External for ColumnResizeExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Opt into the capture lock so the drag survives the cursor straying past
    /// the column edge (the canonical drag-to-resize UX — the same stance the
    /// splitter and slider take).
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// Normalize the captured cursor against the grid **viewport** (the
    /// horizontal scroll node), not the grabbed cell or strip. The cell is what
    /// the drag resizes — its width moves every frame — so it cannot be the
    /// pixel basis; the viewport width is stable across the drag.
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Tag(self.viewport_tag.as_ref())
    }

    /// Translate the captured cursor into a column width through the
    /// [`DragCalibration`] substrate.
    ///
    /// `x_rel` is the cursor's fraction across the (stable) viewport. The first
    /// move snapshots `width_at_press` and the anchor fraction and does not
    /// mutate (the user has not dragged yet); each later move yields the
    /// fraction delta, which `· viewport_w` recovers as pixel travel and the
    /// drag applies as `width_at_press + travel_px` — a 1:1 pixel drag anchored
    /// on the press width, so a clamp at the floor un-clamps cleanly when the
    /// cursor returns. `set_width` floors the result at `min_width`, so the
    /// column can never collapse to a sliver. `viewport_w` is the same width the
    /// grid's horizontal scroll measures (`measured_viewport`), so the drag and
    /// the scroll agree on the pixel scale.
    ///
    /// `y_rel` is ignored (column width is the horizontal axis only).
    fn pointer_move(&mut self, x_rel: f32, _y_rel: f32) {
        if let Some((width_at_press, delta)) = self
            .resize
            .drive(f64::from(x_rel), || Some(self.state.width(self.col)))
        {
            let (viewport_w, _) = self.h_scroll.measured_viewport();
            let next = f64::from(width_at_press) + delta * f64::from(viewport_w);
            self.state.set_width(self.col, px_to_width(next.round()));
        }
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    /// R1347 §5.20 — `true` while a `"width_committed"` intent is queued.
    fn is_dirty(&self) -> bool {
        !self.pending_intents.is_empty()
    }

    /// R1347 §5.20 — flush the drag-end commit channel. Drained by
    /// `walk_scene_and_drain` from `CoreShell::tail`, i.e. within the same
    /// input dispatch that delivered the `PointerUp` — no follow-up frame.
    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        while let Some(intent) = self.pending_intents.pop_front() {
            sink(intent);
        }
    }
}

impl ExternalIntrospect for ColumnResizeExternal {
    fn schema(&self) -> IntrospectSchema {
        // `col`     — the column this handle resizes (query only).
        // `dragging`— live drag-in-progress flag (query only).
        // `send`    — framework synthetic pointer-event channel (PointerUp /
        //             PointerCancel teardown).
        IntrospectSchema::new(&[("col", "int"), ("dragging", "bool"), ("send", "string")])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "col" => Some(IntrospectValue::Int(
                i64::try_from(self.col).unwrap_or(i64::MAX),
            )),
            "dragging" => Some(IntrospectValue::Bool(self.is_dragging())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Both are framework-owned: `col` is construction-fixed and
            // `dragging` is set by the pointer arc. A client widens a column
            // through the `ColumnWidthExternal` `set_col_width` path, not here.
            "col" | "dragging" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// R51.41 §5.35 — framework synthetic pointer-event channel. The router
    /// calls `invoke("send", Text("<sub>:PointerUp"))` /
    /// `"<sub>:PointerCancel"` when the user releases or the OS cancels the
    /// capture span (the strip is a composite `"<tag>_ch<col>#resize"` target,
    /// so the payload carries the `"resize"` sub-index as the first segment).
    /// On `PointerUp` the handle commits the settled width (R1347
    /// `"width_committed"`, `Self::commit_drag`); on `PointerCancel` it tears
    /// down silently (`Self::clear_drag`). Either way the calibration is
    /// dropped so the next press recalibrates. `PointerDown` / `PointerEnter` /
    /// `PointerLeave` arrive too but need no reaction (calibration happens in
    /// the first `pointer_move`).
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path != "send" {
            return Err(InvokeError::UnknownPath);
        }
        let raw = args.as_str().ok_or(InvokeError::TypeMismatch)?;
        // Composite payload: "<sub>:<Event>[:<mods>]" — decoded through the
        // R880.1 `:` grammar SSOT; `None` covers the bare "<Event>" wire
        // (the documented non-composite decode contract of the splitter).
        let event = split_send_payload(raw).map_or(raw, |(_, event, _)| event);
        match PointerWireEvent::from_wire_name(event) {
            // R1347 §5.20 — the release pair diverges only in the commit
            // channel: `Up` settles the gesture and emits `"width_committed"`,
            // `Cancel` tears down silently (R51.93 §5.35).
            Some(PointerWireEvent::Up) => {
                self.commit_drag();
                Ok(IntrospectValue::Null)
            }
            Some(PointerWireEvent::Cancel) => {
                self.clear_drag();
                Ok(IntrospectValue::Null)
            }
            Some(PointerWireEvent::Down | PointerWireEvent::Enter | PointerWireEvent::Leave) => {
                Ok(IntrospectValue::Null)
            }
            None => Err(InvokeError::UnknownPath),
        }
    }
}

/// R786 §5.27 — build one [`ColumnResizeExternal`] per column, each registered
/// under the header-cell tag the [`view_virtual_table`](../../../pinion_widget_paint/table/fn.view_virtual_table.html)
/// paints (`"<table_tag>_ch<col>"`). The grabber strip the header paints
/// (`"<table_tag>_ch<col>#resize"`) routes its capture to the matching primary
/// half, so a drag on column `c`'s border drives this slice's column `c`.
///
/// `h_scroll` is the grid's horizontal [`ScrollState`] and `viewport_tag` is
/// that scroll node's tag — the stable rect every handle normalizes its drag
/// against (the dragged cell resizes, so it cannot be the pixel basis).
///
/// The binding returns this from `create_extra_externals` alongside the
/// [`ColumnWidthExternal`] — all over the **same** shared [`ColumnWidths`] (so a
/// live drag and an RPC `set_col_width` agree). The `"<table_tag>_ch<col>"` tag
/// is the established header-cell convention table.rs paints and the a11y
/// `columnheader` walker reconstructs (R707); this is a third consumer of that
/// convention, keyed by the same `format!` so it cannot drift.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set when `widths.col_count()` is read
/// outside any reactive scope — call from within `create_extra_externals`
/// (which runs inside a `root_owner.run`).
#[must_use]
pub fn column_resize_externals(
    table_tag: &str,
    widths: &Rc<ColumnWidths>,
    h_scroll: &Rc<ScrollState>,
    viewport_tag: impl Into<Cow<'static, str>>,
) -> Vec<ExtraExternal> {
    let viewport_tag = viewport_tag.into();
    (0..widths.col_count())
        .map(|col| {
            ExtraExternal::new(
                crate::composite_tag::GridTag::col_header(table_tag, col),
                Box::new(ColumnResizeExternal::new(
                    Rc::clone(widths),
                    col,
                    Rc::clone(h_scroll),
                    viewport_tag.clone(),
                )),
            )
        })
        .collect()
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
        assert_eq!(
            w.set_width(1, 250),
            250,
            "in-range set returns the applied width"
        );
        assert_eq!(w.width(1), 250);
        assert_eq!(w.total(), 130 + 250 + 200);
        // Below the floor clamps up.
        assert_eq!(
            w.set_width(0, 5),
            DEFAULT_MIN_COL_WIDTH,
            "sub-min clamps to the floor"
        );
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
    fn min_width_invariant_holds_from_construction() {
        // R785.1 — the "width >= min_width" invariant holds at construction,
        // not just after a mutation: a sub-floor initial width clamps up.
        let w = ColumnWidths::new(vec![10, 200, 5]);
        assert_eq!(
            w.width(0),
            DEFAULT_MIN_COL_WIDTH,
            "sub-min initial width clamped"
        );
        assert_eq!(w.width(1), 200, "above-min initial width untouched");
        assert_eq!(
            w.width(2),
            DEFAULT_MIN_COL_WIDTH,
            "sub-min initial width clamped"
        );
    }

    #[test]
    fn with_min_width_reclamps_existing_widths() {
        // R785.1 — raising the floor re-clamps already-stored widths, so the
        // invariant holds after the floor moves (not only on the next write).
        let w = ColumnWidths::new(vec![60, 200]).with_min_width(100);
        assert_eq!(
            w.width(0),
            100,
            "stored width below the new floor re-clamped"
        );
        assert_eq!(
            w.width(1),
            200,
            "stored width above the new floor untouched"
        );
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
        let out = ext
            .invoke("set_col_width", IntrospectValue::Text("1=300".into()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Int(300));
        assert_eq!(state.width(1), 300);
        // Sub-min clamps and the return reports the clamped value.
        let out = ext
            .invoke("set_col_width", IntrospectValue::Text("1=5".into()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Int(i64::from(DEFAULT_MIN_COL_WIDTH)));
    }

    #[test]
    fn external_query_exposes_widths_total_cols() {
        let ext = ColumnWidthExternal::new(Rc::new(widths()));
        assert_eq!(
            ext.query("widths"),
            Some(IntrospectValue::Text("130,90,200".into()))
        );
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
        ext.intervene("widths", IntrospectValue::Text("60,60,60,60".into()))
            .unwrap();
        assert_eq!(state.col_count(), 4, "restore replaces the whole vector");
        assert_eq!(state.total(), 240);
        assert_eq!(
            ext.intervene("total", IntrospectValue::Int(9)),
            Err(InterveneError::ReadOnly)
        );
    }

    // ---- R786 live-drag column resize ----

    /// A 500px-wide viewport (the stable normalization basis): a cursor moving
    /// `f` fraction of the viewport is an `f * 500` px column-width delta.
    const VP_W: u32 = 500;

    fn resize_ext(state: &Rc<ColumnWidths>, col: usize) -> ColumnResizeExternal {
        let h = Rc::new(ScrollState::with_tag("vp"));
        h.set_measured_viewport(VP_W, 400);
        ColumnResizeExternal::new(Rc::clone(state), col, h, "vp")
    }

    #[test]
    fn resize_calibration_frame_does_not_mutate() {
        // The press-time `pointer_move` snapshots the grab but leaves the width
        // untouched (the user has not dragged yet) — the splitter calibration
        // contract.
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 1);
        assert!(!ext.is_dragging());
        ext.pointer_move(0.5, 0.5); // grab somewhere mid-viewport
        assert!(ext.is_dragging(), "first move arms the drag");
        assert_eq!(state.width(1), 90, "calibration frame is non-mutating");
    }

    #[test]
    fn resize_drag_tracks_cursor_in_pixels() {
        // The width delta is the cursor's fraction-of-viewport travel times the
        // viewport width (the splitter formula, stable basis). Column 1 = 90px.
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 1);
        ext.pointer_move(0.5, 0.0); // calibrate: press_x_rel = 0.5
        // +0.2 viewport-fraction = +100px (0.2 * 500).
        ext.pointer_move(0.7, 0.0);
        assert_eq!(state.width(1), 190, "drag widened by +0.2*VP = +100px");
        // The delta is always measured from the PRESS fraction (fixed basis),
        // so an intermediate move re-derives from 90, never compounding.
        ext.pointer_move(0.6, 0.0); // +0.1*500 = +50
        assert_eq!(
            state.width(1),
            140,
            "delta re-derived from the press anchor"
        );
        // Drag far left, past the floor: clamps to min_width.
        ext.pointer_move(0.0, 0.0); // -0.5*500 = -250 -> 90-250 < 0 -> floored
        assert_eq!(
            state.width(1),
            DEFAULT_MIN_COL_WIDTH,
            "sub-min clamps to the floor"
        );
        // Returning right un-clamps cleanly (anchored on the press width, 90).
        ext.pointer_move(0.7, 0.0);
        assert_eq!(
            state.width(1),
            190,
            "un-clamps from the press anchor, not the floor"
        );
    }

    #[test]
    fn resize_grab_offset_carries_no_jump() {
        // Grabbing anywhere (not exactly the border) does not jump: the first
        // move is calibration-only, so the width holds until the cursor travels.
        let state = Rc::new(ColumnWidths::new(vec![100]));
        let mut ext = resize_ext(&state, 0);
        ext.pointer_move(0.42, 0.0); // grabbed at an arbitrary fraction
        assert_eq!(state.width(0), 100, "no jump on the calibration frame");
        ext.pointer_move(0.46, 0.0); // +0.04*500 = +20px
        assert_eq!(state.width(0), 120);
    }

    #[test]
    fn resize_pointer_up_clears_calibration() {
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 0);
        ext.pointer_move(0.5, 0.0);
        assert!(ext.is_dragging());
        // The strip is a composite "<tag>_ch0#resize" target, so the wire
        // payload carries the "resize" sub-index segment.
        ext.invoke("send", IntrospectValue::Text("resize:PointerUp".into()))
            .unwrap();
        assert!(!ext.is_dragging(), "PointerUp tears down the drag");
        assert_eq!(ext.query("dragging"), Some(IntrospectValue::Bool(false)));
        // A fresh press recalibrates from the new width.
        let _ = state.set_width(0, 160);
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.54, 0.0); // +0.04*500 = +20 -> 180
        assert_eq!(
            state.width(0),
            180,
            "recalibrated against the post-drag width"
        );
    }

    /// R1347 §5.20 — drain the External's pending intents into a plain Vec.
    fn harvest(ext: &mut ColumnResizeExternal) -> Vec<Intent> {
        let mut out = Vec::new();
        ext.drain_intents(&mut |i| out.push(i));
        out
    }

    #[test]
    fn r1347_resize_drag_end_emits_width_committed_with_settled_width() {
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 1); // column 1 = 90px
        ext.pointer_move(0.5, 0.0); // calibrate
        ext.pointer_move(0.7, 0.0); // +0.2*500 = +100 -> 190
        assert!(!ext.is_dirty(), "an in-flight drag commits nothing");

        ext.invoke("send", IntrospectValue::Text("resize:PointerUp".into()))
            .unwrap();
        assert!(ext.is_dirty(), "drag end arms the commit channel");
        let harvested = harvest(&mut ext);
        assert_eq!(harvested.len(), 1, "exactly one commit per drag");
        assert_eq!(harvested[0].tag_str(), "width_committed");
        assert_eq!(
            harvested[0].payload,
            IntrospectValue::Int(190),
            "payload is the settled width",
        );
        assert!(!ext.is_dirty(), "drain clears the queue");
    }

    #[test]
    fn r1347_resize_bare_click_commits_nothing() {
        // The load-bearing case: the router forwards a press-time `pointer_move`
        // to every capture widget (R51.35), so a bare click arms the anchor and
        // `DragCalibration::end()` is `true`. A click resized nothing, so it
        // must NOT reach the persistence channel.
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 1);
        ext.pointer_move(0.5, 0.0); // the press-time forward: calibration only
        assert_eq!(state.width(1), 90, "the click moved no width");
        ext.invoke("send", IntrospectValue::Text("resize:PointerUp".into()))
            .unwrap();
        assert!(
            !ext.is_dirty(),
            "a click that resized nothing must not commit"
        );
        assert!(harvest(&mut ext).is_empty());
    }

    #[test]
    fn r1347_resize_drag_returning_to_press_width_commits_nothing() {
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 1); // 90px
        ext.pointer_move(0.5, 0.0); // calibrate at 90
        ext.pointer_move(0.7, 0.0); // -> 190
        ext.pointer_move(0.5, 0.0); // back to 90
        assert_eq!(state.width(1), 90, "returned to the press width");
        ext.invoke("send", IntrospectValue::Text("resize:PointerUp".into()))
            .unwrap();
        assert!(
            !ext.is_dirty(),
            "a drag that returned to its press width settled nothing",
        );
    }

    #[test]
    fn r1347_resize_drag_pinned_at_floor_commits_nothing() {
        // Column already at the floor; shove further left every frame -> the
        // clamp holds it at min_width, so the width never leaves the press
        // snapshot and there is nothing new to persist.
        let state = Rc::new(ColumnWidths::new(vec![DEFAULT_MIN_COL_WIDTH]));
        let mut ext = resize_ext(&state, 0);
        ext.pointer_move(0.5, 0.0); // calibrate at min
        ext.pointer_move(0.2, 0.0); // -0.3*500 -> floored at min
        ext.pointer_move(0.0, 0.0); // still floored
        assert_eq!(state.width(0), DEFAULT_MIN_COL_WIDTH, "pinned at floor");
        ext.invoke("send", IntrospectValue::Text("resize:PointerUp".into()))
            .unwrap();
        assert!(
            !ext.is_dirty(),
            "a drag pinned at the floor commits nothing"
        );
    }

    #[test]
    fn r1347_resize_pointer_cancel_keeps_width_but_suppresses_commit() {
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 1); // 90px
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.7, 0.0); // -> 190
        ext.invoke("send", IntrospectValue::Text("resize:PointerCancel".into()))
            .unwrap();
        // R51.93 §5.35 family invariant: the in-flight width stays applied, only
        // the commit is suppressed.
        assert_eq!(
            state.width(1),
            190,
            "in-flight width stays applied across PointerCancel",
        );
        assert!(
            !ext.is_dirty(),
            "PointerCancel must not fire width_committed"
        );
        assert!(harvest(&mut ext).is_empty());
    }

    #[test]
    fn r1347_resize_consecutive_drags_each_commit_once() {
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 1); // 90px
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.7, 0.0); // -> 190
        ext.invoke("send", IntrospectValue::Text("resize:PointerUp".into()))
            .unwrap();
        assert_eq!(harvest(&mut ext).len(), 1);
        // Second drag recalibrates from 190.
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.4, 0.0); // -0.1*500 = -50 -> 140
        ext.invoke("send", IntrospectValue::Text("resize:PointerUp".into()))
            .unwrap();
        let second = harvest(&mut ext);
        assert_eq!(second.len(), 1, "each drag commits exactly once");
        assert_eq!(second[0].payload, IntrospectValue::Int(140));
    }

    #[test]
    fn r882_1_resize_send_decodes_through_the_grammar_ssot() {
        // R882 moved this arm onto `split_send_payload` (the R880.1
        // `:` grammar SSOT). Pin the decode contract incl. the edge
        // the swap changed: a wire with a MALFORMED modifier token is
        // rejected whole (fail-closed — `split_send_payload` returns
        // `None`, the fallback is not a recognised event name, so the
        // drag stays armed and the caller sees `UnknownPath`) instead
        // of the old hand-roll's salvage of the middle segment. The
        // router is the wire's only legitimate emitter and never
        // produces a bad token; a hand-written `scene/invoke` typo
        // must surface loudly, not half-apply.
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 0);
        ext.pointer_move(0.5, 0.0);
        assert!(ext.is_dragging());
        // Well-formed three-segment wire (held modifiers) decodes.
        ext.invoke("send", IntrospectValue::Text("resize:PointerUp:c".into()))
            .unwrap();
        assert!(
            !ext.is_dragging(),
            "the R781 modifier segment decodes through the SSOT"
        );
        // Malformed modifier token → reject-loudly, no teardown.
        ext.pointer_move(0.5, 0.0);
        assert!(ext.is_dragging());
        let err = ext.invoke("send", IntrospectValue::Text("resize:PointerUp:zz".into()));
        assert!(
            err.is_err(),
            "a malformed modifier token rejects the whole wire"
        );
        assert!(ext.is_dragging(), "a rejected wire must not half-apply");
        // Bare (non-composite) event name still decodes via the
        // documented `None` fallback.
        ext.invoke("send", IntrospectValue::Text("PointerCancel".into()))
            .unwrap();
        assert!(!ext.is_dragging());
    }

    #[test]
    fn resize_ignores_non_teardown_pointer_events() {
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 0);
        ext.pointer_move(0.5, 0.0);
        // Enter / Down do not tear down the in-flight calibration.
        ext.invoke("send", IntrospectValue::Text("resize:PointerEnter".into()))
            .unwrap();
        ext.invoke("send", IntrospectValue::Text("resize:PointerDown".into()))
            .unwrap();
        assert!(
            ext.is_dragging(),
            "non-teardown events leave the drag armed"
        );
        // PointerCancel does tear down (OS-cancelled capture).
        ext.invoke("send", IntrospectValue::Text("resize:PointerCancel".into()))
            .unwrap();
        assert!(!ext.is_dragging());
    }

    #[test]
    fn resize_normalizes_against_the_viewport_tag() {
        let state = Rc::new(widths());
        let ext = resize_ext(&state, 0);
        assert_eq!(
            ext.capture_normalize(),
            CaptureNormalize::Tag("vp"),
            "drags normalize against the viewport",
        );
        assert!(ext.wants_pointer_capture(), "opts into the capture lock");
    }

    #[test]
    fn resize_query_and_intervene_surface() {
        let state = Rc::new(widths());
        let mut ext = resize_ext(&state, 2);
        assert_eq!(ext.col(), 2);
        assert_eq!(ext.query("col"), Some(IntrospectValue::Int(2)));
        assert_eq!(ext.query("dragging"), Some(IntrospectValue::Bool(false)));
        assert_eq!(ext.query("nope"), None);
        assert_eq!(
            ext.intervene("col", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            ext.intervene("dragging", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text("bogus".into())),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn column_resize_externals_one_per_column_at_header_tags() {
        let state = Rc::new(widths()); // 3 columns
        let h = Rc::new(ScrollState::with_tag("grid_h"));
        let exts = column_resize_externals("grid", &state, &h, "grid_h");
        assert_eq!(exts.len(), 3, "one resize external per column");
        assert_eq!(exts[0].tag, "grid_ch0");
        assert_eq!(exts[1].tag, "grid_ch1");
        assert_eq!(exts[2].tag, "grid_ch2");
    }
}
