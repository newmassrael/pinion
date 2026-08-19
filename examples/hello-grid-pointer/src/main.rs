// R1008 §5.16 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-grid-pointer` — R1008 §5.41 §5.49 **pointer→cell hit-test (R1.8)**.
//!
//! A full-window [`Scene::TextGrid`] (80 × 24 cells at the 8×16 baseline
//! [`CellMetric`], so the window *is* the grid) whose pointer [`External`]
//! resolves the cursor's logical-pixel position into the `(col, row)` cell it
//! falls in. This is the geometry an xterm mouse-reporting terminal needs: a
//! click in the grid maps to a `(row, col)` the host forwards to the PTY (the
//! sprag pane's mouse path). The cell resolution is the **new substrate**
//! [`CellMetric::px_to_cell`] (R1.8) — the unsigned floor inverse of
//! [`CellMetric::cell_to_px`], sharing the same `whole_cells` SSOT the winsize
//! `cols_for` / `rows_for` counts use, so a hit and the PTY dimensions can
//! never round differently.
//!
//! ## Why the window is the grid
//!
//! The `InputRouter` hands a pointer hook a `(x_rel, y_rel)`
//! **fraction** normalised over the hit widget's rect, not absolute pixels. The
//! single full-window grid makes the primary external's rect equal the grid
//! rect, so the binding reconstructs grid-relative pixels by `x_rel ·
//! width_px` and calls `px_to_cell` directly — exactly how sprag's single pane
//! (its external fills the window) resolves a click. A multi-pane host gives
//! each pane its own external rect and reads its own fraction; the math is the
//! same per pane.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `query("cell")` is the cell under the pointer (`"col,row"` or `"none"`),
//! `query("cell_col")` / `query("cell_row")` its axes; `query("cols")` /
//! `query("rows")` the grid's winsize dimensions. `invoke "cell_at" "x,y"` is a
//! pure `px_to_cell` oracle — window pixels → the clamped in-grid cell — so the
//! whole hit-test is observable and driveable without a pixel (see
//! `tools/demos/r1008_grid_pointer.py`).

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaField,
    ThreadOwnership,
};
use pinion_core::input::PointerReading;
use pinion_core::scene::{ContainerNode, TextGridNode};
use pinion_core::style::{AlignItems, FlexDirection, LayoutStyle, Size};
use pinion_core::{CellMetric, Frame, GridBuffer, Scene, TermCell, TermColor, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridPointerRenderer, HelloGridPointerRendererError);

/// The grid's paint tag **and** the primary external's `tag()` — the router
/// dispatches a pointer event on the painted node to the state external sharing
/// its tag (the `hello-cell-select` primary-grid pattern), so the click lands on
/// [`GridPointerExternal::pointer_move`] normalised over the grid's own rect.
const ROOT_TAG: &str = "grid_pointer";
const COLS: u16 = 80;
const ROWS: u16 = 24;
/// The window *is* the grid: `80 × 8 = 640`, `24 × 16 = 384`, so a pointer
/// fraction over the window is grid-relative.
const WIN_W: u32 = 640;
const WIN_H: u32 = 384;

/// R1008 §5.41 §5.49 — the pointer coordinator: resolves the cursor's
/// window-fraction into the `(col, row)` cell under it via
/// [`CellMetric::px_to_cell`], and exposes it (plus a pure `cell_at` oracle) to
/// `scene/query` / `scene/invoke`.
#[derive(Debug, Clone)]
struct GridPointerExternal {
    metric: CellMetric,
    width_px: u32,
    height_px: u32,
    cols: u16,
    rows: u16,
    /// The cell under the pointer, or `None` before the first move.
    cell: Option<(u16, u16)>,
    /// The last key the binding's `apply_key` forwarded (R1009): the W3C
    /// `KeyboardEvent.key` string a terminal pane would encode for its PTY.
    /// `None` before any key.
    last_key: Option<String>,
}

impl GridPointerExternal {
    fn new(metric: CellMetric, width_px: u32, height_px: u32) -> Self {
        Self {
            metric,
            width_px,
            height_px,
            cols: metric.cols_for(width_px),
            rows: metric.rows_for(height_px),
            cell: None,
            last_key: None,
        }
    }

    /// Window-relative pixels → the **in-grid** cell: [`px_to_cell`] then clamp
    /// to the grid's `(cols, rows)` (a pixel past the right/bottom edge lands on
    /// the last cell), the binding's strict-hit policy over the pure unbounded
    /// geometry primitive.
    ///
    /// [`px_to_cell`]: CellMetric::px_to_cell
    fn cell_at_px(&self, x_px: u32, y_px: u32) -> (u16, u16) {
        let (c, r) = self.metric.px_to_cell(x_px, y_px);
        (
            c.min(self.cols.saturating_sub(1)),
            r.min(self.rows.saturating_sub(1)),
        )
    }

    fn cell_text(&self) -> IntrospectValue {
        match self.cell {
            Some((c, r)) => IntrospectValue::Text(format!("{c},{r}")),
            None => IntrospectValue::Text("none".to_owned()),
        }
    }

    fn last_key_text(&self) -> IntrospectValue {
        IntrospectValue::Text(self.last_key.clone().unwrap_or_else(|| "none".to_owned()))
    }
}

impl External for GridPointerExternal {
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

    /// Take pointer capture on a press so the router's R51.35 "click-to-position"
    /// forwards the press cursor as the initial [`pointer_move`](Self::pointer_move)
    /// — a click (no drag) seeds the cell at the click point, and a press-drag
    /// keeps resolving the cell under the cursor (the mouse-report gesture).
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// The router calls this with a `[0, 1]` fraction over the grid's rect — on
    /// a click (the capture click-to-position forward) and on every move while
    /// the press is held. Reconstruct grid pixels and resolve the cell under the
    /// cursor: the live R1.8 hit-test.
    /// R1656 §5.15 — the shell's resize notification. `width_px` / `height_px`
    /// are the basis `pointer_move`'s fraction is a fraction of, and nothing
    /// updated them: `External::on_resize` was a declared §5.15 arm with no
    /// call site anywhere in the workspace until R1656 wired it, so this grid
    /// resolved a cell against the size it was constructed with for as long as
    /// the window was not that size.
    fn on_resize(&mut self, width: u32, height: u32) {
        self.width_px = width.max(1);
        self.height_px = height.max(1);
        self.cols = self.metric.cols_for(self.width_px);
        self.rows = self.metric.rows_for(self.height_px);
    }

    fn pointer_move(&mut self, at: PointerReading) {
        let x_px = CellMetric::frac_to_px(at.u(), self.width_px);
        let y_px = CellMetric::frac_to_px(at.v(), self.height_px);
        self.cell = Some(self.cell_at_px(x_px, y_px));
    }
}

impl ExternalIntrospect for GridPointerExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("cell", "string"),
                    SchemaField::new("cell_col", "int"),
                    SchemaField::new("cell_row", "int"),
                    SchemaField::new("cols", "int"),
                    SchemaField::new("rows", "int"),
                    SchemaField::action("cell_at", "string"),
                    // R1009 — the last key apply_key forwarded (read) + the record
                    // channel apply_key writes through (invoke).
                    SchemaField::new("last_key", "string"),
                    SchemaField::action("key", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "cell" => Ok(self.cell_text()),
            "cell_col" => Ok(self.cell.map_or(IntrospectValue::Null, |(c, _)| {
                IntrospectValue::Int(i64::from(c))
            })),
            "cell_row" => Ok(self.cell.map_or(IntrospectValue::Null, |(_, r)| {
                IntrospectValue::Int(i64::from(r))
            })),
            "cols" => Ok(IntrospectValue::Int(i64::from(self.cols))),
            "rows" => Ok(IntrospectValue::Int(i64::from(self.rows))),
            "last_key" => Ok(self.last_key_text()),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "cell" | "cell_col" | "cell_row" | "cols" | "rows" | "last_key" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Pure px→cell oracle: "x,y" window pixels → the clamped in-grid
            // "col,row". Deterministic (no pointer state), so an AI client reads
            // the hit-test geometry directly.
            "cell_at" => match args {
                IntrospectValue::Text(ref s) => {
                    let (px_x, px_y) = parse_xy(s).ok_or(InvokeError::TypeMismatch)?;
                    let (col, row) = self.cell_at_px(px_x, px_y);
                    Ok(IntrospectValue::Text(format!("{col},{row}")))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R1009 — the apply_key record channel: the binding forwards every
            // key it receives here, so an AI client reads the last key (and the
            // winit→named_key_str path now delivers Backspace/Delete/F1-F12).
            "key" => match args {
                IntrospectValue::Text(k) => {
                    self.last_key = Some(k.clone());
                    Ok(IntrospectValue::Text(k))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Parse a `"x,y"` unsigned-pixel pair (the `cell_at` argument wire form).
fn parse_xy(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// The single ASCII digit for `n` (`n < 10`).
fn digit(n: u16) -> String {
    char::from(b'0' + u8::try_from(n).unwrap_or(0)).to_string()
}

/// A coordinate-ruler buffer so the rendered grid shows its cell lattice: row 0
/// is the column ruler (each cell its column's last digit), column 0 the row
/// ruler, the interior a middle-dot. Purely visual — the hit-test reads the
/// metric, not the cells.
fn ruler_buffer() -> GridBuffer {
    let mut buf = GridBuffer::new(COLS, ROWS);
    for r in 0..ROWS {
        buf = buf.with_row(
            r,
            (0..COLS).map(move |c| {
                let ch = if r == 0 {
                    digit(c % 10)
                } else if c == 0 {
                    digit(r % 10)
                } else {
                    "\u{00B7}".to_owned()
                };
                TermCell::new(ch, TermColor::Default, TermColor::Default)
            }),
        );
    }
    buf
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    // Tag the grid with ROOT_TAG (= the state external's tag): the router
    // matches the painted node's tag to the external and dispatches the pointer
    // there, normalised over THIS node's rect — so the grid filling the window
    // makes the fraction grid-relative.
    let grid = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(ROOT_TAG)
            .with_cells(ruler_buffer())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, 0)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_focusable(true),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![grid])
            // Pin the root to the window so the absolutely-positioned grid (out
            // of flow) does not collapse the flex root.
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

struct GridPointerView;

impl WidgetCore for GridPointerView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        ROOT_TAG
    }

    /// The pointer coordinator is the primary external — its rect is the full
    /// window (= the grid), so its `pointer_move` fraction is grid-relative.
    fn create_external() -> Box<dyn External> {
        Box::new(GridPointerExternal::new(CellMetric::DEFAULT, WIN_W, WIN_H))
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// R1009 §5.13 — a content-surface terminal pane: while focused it consumes
    /// every key the shell forwards (a real terminal hands them all to its
    /// child), recording the W3C string a sprag pane would encode for its PTY.
    /// The R1009 `named_key_str` widening is what makes Backspace / Delete /
    /// Insert / F1-F12 reach here on the winit path (the RPC path always did).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(ROOT_TAG) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let _ = intro.invoke("key", IntrospectValue::Text(key.to_owned()));
        true
    }

    fn title() -> &'static str {
        "pinion hello-grid-pointer (R1008 §5.41 pointer-to-cell hit-test)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (cell-under-pointer in the coordinator)".to_string()
    }
}

impl WidgetA11y for GridPointerView {
    /// A single `group` region for the terminal grid — the cell under the
    /// pointer is conveyed through the proxy's `cell` query, not the static AT
    /// structure (an 80×24 per-cell tree would be noise; the live cursor cell
    /// is the meaningful datum).
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        vec![AccessNode::new(ROOT_TAG, AriaRole::Group).with_name("Pointer cell grid")]
    }
}

impl WidgetView for GridPointerView {
    type Renderer = HelloGridPointerRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }

    /// R1010 §5.39 — a terminal pane owns its focus indicator (the text cursor),
    /// so it suppresses the framework focus ring while still taking focus for
    /// `apply_key` (R1009). Without this the whole viewport would carry a blue
    /// ring that fights the terminal's own cursor.
    fn focus_ring_style(_focused_tag: &str) -> Option<pinion_shell::FocusRingStyle> {
        None
    }
}

fn main() {
    pinion_shell::run::<GridPointerView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext() -> GridPointerExternal {
        GridPointerExternal::new(CellMetric::DEFAULT, WIN_W, WIN_H)
    }

    #[test]
    fn grid_dims_derive_from_the_metric() {
        let e = ext();
        assert_eq!(e.cols, 80, "640 / 8 = 80 columns");
        assert_eq!(e.rows, 24, "384 / 16 = 24 rows");
    }

    #[test]
    fn cell_at_px_floors_pixel_into_its_cell() {
        let e = ext();
        assert_eq!(e.cell_at_px(0, 0), (0, 0));
        assert_eq!(e.cell_at_px(7, 15), (0, 0), "any pixel inside cell (0,0)");
        assert_eq!(e.cell_at_px(8, 16), (1, 1), "first pixel of the next cell");
        assert_eq!(e.cell_at_px(24, 32), (3, 2));
    }

    #[test]
    fn cell_at_px_clamps_off_grid_pixels_to_the_last_cell() {
        let e = ext();
        // One cell past each edge → clamped to (79, 23), not the raw (80, 24).
        assert_eq!(e.cell_at_px(640, 384), (79, 23));
        assert_eq!(e.cell_at_px(10_000, 10_000), (79, 23));
    }

    #[test]
    fn pointer_move_resolves_the_cell_under_the_fraction() {
        let mut e = ext();
        assert_eq!(e.cell, None, "no cell before the first move");
        // The centre of the window: 0.5 × 640 = 320 px = col 40; 0.5 × 384 = 192
        // px = row 12.
        e.pointer_move(PointerReading::over_unit((0.5, 0.5)));
        assert_eq!(e.cell, Some((40, 12)));
        // The far corner fraction lands on the last cell, never one past it.
        e.pointer_move(PointerReading::over_unit((1.0, 1.0)));
        assert_eq!(e.cell, Some((79, 23)));
        // The origin.
        e.pointer_move(PointerReading::over_unit((0.0, 0.0)));
        assert_eq!(e.cell, Some((0, 0)));
    }

    #[test]
    fn pointer_move_resolves_every_cell_boundary_pixel() {
        // R1011 — mirror the router: a click at integer pixel `px` arrives as
        // the f32 fraction `normalize_cursor` computes. Reconstructing the pixel
        // from that lossy f32 and flooring mis-resolves a cell's leading-edge
        // pixel (the fraction rounds just under, flooring into the PREVIOUS
        // cell). The painter places the cell with `cell_to_px`, so the reported
        // cell must equal the painted one at every boundary pixel.
        let mut e = ext();
        for col in 0..COLS {
            let px = u32::from(col) * 8; // leading-edge pixel of column `col`
            #[allow(clippy::cast_possible_truncation)]
            let frac = (f64::from(px) / f64::from(WIN_W)) as f32;
            e.pointer_move(PointerReading::over_unit((frac, 0.0)));
            assert_eq!(e.cell, Some((col, 0)), "column {col} leading pixel {px}");
        }
        for row in 0..ROWS {
            let py = u32::from(row) * 16; // leading-edge pixel of row `row`
            #[allow(clippy::cast_possible_truncation)]
            let frac = (f64::from(py) / f64::from(WIN_H)) as f32;
            e.pointer_move(PointerReading::over_unit((0.0, frac)));
            assert_eq!(e.cell, Some((0, row)), "row {row} leading pixel {py}");
        }
    }

    #[test]
    fn query_surfaces_cell_and_dims() {
        let mut e = ext();
        assert_eq!(e.query("cell"), Ok(IntrospectValue::Text("none".into())));
        assert_eq!(e.query("cell_col"), Ok(IntrospectValue::Null));
        assert_eq!(e.query("cols"), Ok(IntrospectValue::Int(80)));
        assert_eq!(e.query("rows"), Ok(IntrospectValue::Int(24)));
        e.pointer_move(PointerReading::over_unit((0.5, 0.5)));
        assert_eq!(e.query("cell"), Ok(IntrospectValue::Text("40,12".into())));
        assert_eq!(e.query("cell_col"), Ok(IntrospectValue::Int(40)));
        assert_eq!(e.query("cell_row"), Ok(IntrospectValue::Int(12)));
        assert_eq!(e.query("nope"), Err(ReadRefusal::UnknownPath));
    }

    #[test]
    fn invoke_cell_at_is_a_pure_px_to_cell_oracle() {
        let mut e = ext();
        assert_eq!(
            e.invoke("cell_at", IntrospectValue::Text("0,0".into())),
            Ok(IntrospectValue::Text("0,0".into()))
        );
        assert_eq!(
            e.invoke("cell_at", IntrospectValue::Text("320,192".into())),
            Ok(IntrospectValue::Text("40,12".into()))
        );
        assert_eq!(
            e.invoke("cell_at", IntrospectValue::Text("639,383".into())),
            Ok(IntrospectValue::Text("79,23".into()))
        );
        // Off-grid clamps to the last cell.
        assert_eq!(
            e.invoke("cell_at", IntrospectValue::Text("640,384".into())),
            Ok(IntrospectValue::Text("79,23".into()))
        );
        assert_eq!(
            e.invoke("cell_at", IntrospectValue::Int(3)),
            Err(InvokeError::TypeMismatch)
        );
        assert_eq!(
            e.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn intervene_guards_readonly() {
        let mut e = ext();
        assert_eq!(
            e.intervene("cell", IntrospectValue::Text("1,1".into())),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            e.intervene("last_key", IntrospectValue::Text("x".into())),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            e.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath)
        );
    }

    #[test]
    fn terminal_pane_suppresses_the_focus_ring() {
        // R1010 — the content-surface opt-out: the pane owns its cursor, so no
        // framework ring (the default would be Some(FocusRingStyle::default())).
        assert!(GridPointerView::focus_ring_style(ROOT_TAG).is_none());
    }

    #[test]
    fn key_invoke_records_the_last_forwarded_key() {
        let mut e = ext();
        assert_eq!(
            e.query("last_key"),
            Ok(IntrospectValue::Text("none".into())),
            "no key yet"
        );
        // The editing + function keys R1009 added to named_key_str arrive here
        // as their W3C strings (the binding's apply_key forwards them).
        e.invoke("key", IntrospectValue::Text("Backspace".into()))
            .unwrap();
        assert_eq!(
            e.query("last_key"),
            Ok(IntrospectValue::Text("Backspace".into()))
        );
        e.invoke("key", IntrospectValue::Text("F5".into())).unwrap();
        assert_eq!(e.query("last_key"), Ok(IntrospectValue::Text("F5".into())));
        assert_eq!(
            e.invoke("key", IntrospectValue::Int(3)),
            Err(InvokeError::TypeMismatch)
        );
    }
}
