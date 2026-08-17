// Prose mentions of widget type names (TextGrid, …) read fine un-backticked in a
// test binding — the same looser doc-markdown lid the hello-* bindings use.
#![allow(clippy::doc_markdown)]

//! R1036 §5.16 §5.7 §2 #7 — `scene/render_fidelity` AI-first detection, end-to-end
//! forcing consumer (PR-17).
//!
//! Reproduces the PR-17 divergence HEADLESSLY and proves it is detectable in
//! DATA through the RPC surface — closing the doc's "data is not enough"
//! exception. A single-pane terminal view whose inked-row count is a mutable
//! mock-PTY global stands in for the async reflow: we
//!
//!   1. paint a STALE frame (3 prompt rows, mid-reflow) and record it as
//!      presented (`record_presented_frame`, the winit-paint-path SSOT), then
//!   2. settle the producer to 1 row WITHOUT repainting (the missed-settle-paint
//!      / stale-present condition), then
//!   3. call `scene/render_fidelity` and assert it reports the displayed frame
//!      (3 rows) diverging from current state (1 row) — `diverged: true`.
//!
//! This is exactly what `scene/snapshot {from:"paint"}` cannot show: its
//! `last_paint_scene` is overwritten by the R684 post-dispatch finalize with a
//! query-time recompute, so it would report the settled 1 and hide the
//! divergence. The render-fidelity record is written only on the paint path, so
//! it stays the uncontaminated displayed-frame truth.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::TextGridNode;
use pinion_core::term_grid::{TermCell, TermColor};
use pinion_core::{CellMetric, Frame, GridBuffer, Scene, WidgetCore};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU16, Ordering};

/// The 8×16 baseline metric.
const CELL: CellMetric = CellMetric::DEFAULT;
const GRID_TAG: &str = "term#grid";
/// The single-window key `compute_paint_scene` / `dispatch_rpc` resolve to
/// (`pinion_runtime::DEFAULT_WINDOW`).
const MAIN_WINDOW: &str = "main";
/// Fixed grid dims — the bug is a *content/used-row* divergence at constant
/// dims, so the buffer is `40 × 10` regardless of the inked-row count.
const COLS: u16 = 40;
const ROWS: u16 = 10;

/// Mock-PTY inked-row count the view reads — the stand-in for an async terminal
/// reflow settling. Tests set it before paint / before the fidelity read.
static PROMPT_ROWS: AtomicU16 = AtomicU16::new(1);
/// Serialises tests: `PROMPT_ROWS` is process-global.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn cell(ch: char) -> TermCell {
    TermCell::new(ch.to_string(), TermColor::Default, TermColor::Default)
}

/// A `COLS × ROWS` grid with `"coin@coin"` written into the first
/// [`PROMPT_ROWS`] rows — `used_rows` tracks the mock-PTY state.
fn term_grid() -> Scene {
    let inked = PROMPT_ROWS.load(Ordering::SeqCst).min(ROWS);
    let mut buf = GridBuffer::new(COLS, ROWS);
    for r in 0..inked {
        buf = buf.with_row(r, "coin@coin".chars().map(cell));
    }
    Scene::TextGrid(TextGridNode::new(CELL).with_tag(GRID_TAG).with_cells(buf))
}

/// A no-op primary external — the seam runs through the TextGrid, but
/// `WidgetCore` requires a primary external to carry the state scene.
#[derive(Debug, Default)]
struct StubExternal;

impl External for StubExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
}

struct TermView;

impl WidgetCore for TermView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        "term"
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }

    fn read_state(_scene: &Scene) {}

    fn view(_state: (), _frame: &Frame) -> Scene {
        term_grid()
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion render-fidelity seam (R1036)"
    }
}

impl WidgetA11y for TermView {}

impl WidgetView for TermView {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: 400,
            height: 200,
        }
    }
}

/// Dispatch `scene/render_fidelity` and return the `result` object.
fn render_fidelity(core: &mut ShellCore<TermView>) -> serde_json::Value {
    let mut no_resize = |_: u32, _: u32| {};
    let req = r#"{"jsonrpc":"2.0","method":"scene/render_fidelity","id":1}"#;
    let resp = core.dispatch_rpc(req, &mut no_resize).expect("response");
    let body: serde_json::Value = serde_json::from_str(&resp).expect("JSON");
    body.get("result")
        .unwrap_or_else(|| panic!("dispatch error, no result: {body}"))
        .clone()
}

/// R1709 — a window that is presenting normally. These tests are about the
/// displayed-vs-state divergence, so the recovery ladder is at rest for all
/// of them; the presentability axis has its own coverage in `pinion-gpu`
/// and in the round's demo.
fn healthy() -> pinion_runtime::PresentHealth {
    pinion_runtime::PresentHealth::default()
}

/// The single grid's `used_rows` in a `displayed` / `state` array.
fn used_rows(arr: &serde_json::Value) -> u64 {
    let grids = arr.as_array().expect("grid array");
    assert_eq!(grids.len(), 1, "one grid in {arr}");
    grids[0]
        .get("used_rows")
        .and_then(serde_json::Value::as_u64)
        .expect("used_rows")
}

#[test]
fn render_fidelity_detects_stale_displayed_vs_settled_state() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // (1) Paint a STALE frame: 3 prompt rows (mid-reflow residue) and record it
    //     as the presented frame — the winit-paint-path SSOT.
    PROMPT_ROWS.store(3, Ordering::SeqCst);
    let mut core = ShellCore::<TermView>::new();
    let stale = core.compute_paint_scene(400, 200);
    core.record_presented_frame(MAIN_WINDOW, true, (400, 200), &stale, healthy());

    // (2) The producer SETTLES to 1 row, but no repaint happens (the missed
    //     settle-paint / stale-present condition).
    PROMPT_ROWS.store(1, Ordering::SeqCst);

    // (3) The fidelity read shows the displayed frame (3) diverging from current
    //     state (1) — the §2 #7 violation, in DATA, with no pixels.
    let result = render_fidelity(&mut core);
    assert_eq!(
        used_rows(result.get("displayed").expect("displayed")),
        3,
        "displayed = the recorded stale frame, NOT a query-time recompute",
    );
    assert_eq!(
        used_rows(result.get("state").expect("state")),
        1,
        "state = current settled producer",
    );
    assert_eq!(
        result.get("diverged").and_then(serde_json::Value::as_bool),
        Some(true),
        "render fidelity flags displayed != state: {result}",
    );
    let divs = result
        .get("divergences")
        .and_then(serde_json::Value::as_array)
        .expect("divergences");
    assert_eq!(divs.len(), 1, "one diverging grid");
    assert_eq!(
        divs[0].get("tag").and_then(serde_json::Value::as_str),
        Some(GRID_TAG),
    );
}

#[test]
fn render_fidelity_reports_no_divergence_when_settled() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // Paint and record a settled 2-row frame; the producer does not change, so
    // displayed == state and the verdict is "no divergence".
    PROMPT_ROWS.store(2, Ordering::SeqCst);
    let mut core = ShellCore::<TermView>::new();
    let scene = core.compute_paint_scene(400, 200);
    core.record_presented_frame(MAIN_WINDOW, true, (400, 200), &scene, healthy());

    let result = render_fidelity(&mut core);
    assert_eq!(used_rows(result.get("displayed").expect("displayed")), 2);
    assert_eq!(used_rows(result.get("state").expect("state")), 2);
    assert_eq!(
        result.get("diverged").and_then(serde_json::Value::as_bool),
        Some(false),
        "settled frame: no divergence: {result}",
    );
}

#[test]
fn paint_seq_advances_once_per_recorded_present() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    PROMPT_ROWS.store(1, Ordering::SeqCst);
    let mut core = ShellCore::<TermView>::new();
    let scene = core.compute_paint_scene(400, 200);

    core.record_presented_frame(MAIN_WINDOW, true, (400, 200), &scene, healthy());
    let first = render_fidelity(&mut core);
    assert_eq!(
        first.get("paint_seq").and_then(serde_json::Value::as_u64),
        Some(1),
        "first present is seq 1",
    );
    assert_eq!(
        first.get("present_ok").and_then(serde_json::Value::as_bool),
        Some(true),
    );

    core.record_presented_frame(MAIN_WINDOW, true, (400, 200), &scene, healthy());
    let second = render_fidelity(&mut core);
    assert_eq!(
        second.get("paint_seq").and_then(serde_json::Value::as_u64),
        Some(2),
        "each present advances paint_seq — an AI reads it to confirm a new frame",
    );
}

#[test]
fn render_fidelity_unavailable_before_first_paint() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mut core = ShellCore::<TermView>::new();
    let mut no_resize = |_: u32, _: u32| {};
    let req = r#"{"jsonrpc":"2.0","method":"scene/render_fidelity","id":1}"#;
    let resp = core.dispatch_rpc(req, &mut no_resize).expect("response");
    // A never-painted window has no record → typed `RenderFidelityUnavailable`
    // in `error.data`, so an AI distinguishes "not painted yet" from "diverged".
    assert!(
        resp.contains("RenderFidelityUnavailable"),
        "expected RenderFidelityUnavailable before first paint: {resp}",
    );
}
