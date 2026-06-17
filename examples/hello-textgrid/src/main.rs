//! `hello-textgrid` — R972 §5.41 — the **cell-native `TextGrid` geometry
//! scaffold**, the first real consumer of the [`CellMetric`] coordinate
//! substrate (R968 ratify → R970 metric type → R971 authoritative dims).
//!
//! Two [`Scene::TextGrid`] nodes sit at fixed absolute positions:
//!
//!   * **`htg_default`** — the behaviour-preserving 8×16 bitmap baseline
//!     ([`CellMetric::DEFAULT`]), sized `640×384` → **80 × 24** cells;
//!   * **`htg_measured`** — a *measured* monospace metric sourced via
//!     [`CellMetric::new`]`(9, 18)` (the R968 font-derivation hook),
//!     sized `360×360` → **40 × 20** cells.
//!
//! ## What this scaffold proves (and what it defers)
//!
//! The grid is an **empty geometry scaffold**: it carries a node-local
//! [`CellMetric`] plus its layout-resolved pixel [`Rect`], and derives
//! its `(cols, rows)` from that rect via the metric — the R969
//! one-directional `(rows, cols)` SSOT (layout → dims, never fed back).
//! The cell **data model** (R969 cluster / fg / bg / attrs / cursor /
//! alt-screen / damage) and **paint** are deliberate follow-up rounds,
//! so the window renders only its surface background — the deliverable
//! here is the *coordinate space*, read as data, not pixels.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` reports each grid's `rect`, `cell_w` / `cell_h`, and
//! derived `cols` / `rows`. From those an AI client reconstructs the
//! whole cell↔pixel mapping with no OCR: the winsize round-trip is
//! `cols == floor(rect.w / cell_w)` (and the pixel extent the cells
//! span, `cols * cell_w`, fits within `rect.w`). The R972 demo
//! (`tools/r972_textgrid.py`) asserts this for both metrics.

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::{ContainerNode, TextGridNode};
use pinion_core::style::{BoxStyle, LayoutStyle, Size};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::{CellMetric, Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTextGridRenderer, HelloTextGridRendererError);

/// Window size — large enough to hold both grids with a margin.
const WIN_W: u32 = 680;
const WIN_H: u32 = 840;
/// Shared [`ThemeProvider`] cache key.
const THEME_TAG: &str = "app";
/// Paint-root + [`StubExternal`] anchor tag (the `V::tag()` the composite
/// paint-root convention attaches to the root container).
const ROOT_TAG: &str = "htg";

/// Tag of the 8×16-baseline grid, and its placement + extent.
const DEFAULT_TAG: &str = "htg_default";
const DEFAULT_POS: (u32, u32) = (16, 16);
const DEFAULT_SIZE: (u32, u32) = (640, 384); // 80 cols × 24 rows @ 8×16

/// Tag of the measured-metric (9×18) grid, and its placement + extent.
const MEASURED_TAG: &str = "htg_measured";
const MEASURED_POS: (u32, u32) = (16, 432);
const MEASURED_SIZE: (u32, u32) = (360, 360); // 40 cols × 20 rows @ 9×18
/// Measured monospace cell metric (advance width × line height). Sourced
/// through [`CellMetric::new`] — the R968 font-derivation hook this
/// scaffold first consumes (a real Vello font measurement lands later).
const MEASURED_CELL: (u32, u32) = (9, 18);

/// Build one absolutely-positioned grid: the absolute layout removes it
/// from flow and gives it exactly its own `Size`, so the layout pass
/// resolves a deterministic pixel `Rect` and the derived `(cols, rows)`
/// are stable regardless of window chrome.
fn grid(tag: &'static str, metric: CellMetric, pos: (u32, u32), size: (u32, u32)) -> Scene {
    Scene::TextGrid(
        TextGridNode::new(metric)
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(pos.0, pos.1)
                    .with_size(Size::px(size.0, size.1)),
            ),
    )
}

/// view-fn (§6.3): pure sync `() -> Scene`. A plain surface-filled root
/// (tagged [`ROOT_TAG`]) holding the two geometry grids.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let measured = CellMetric::new(MEASURED_CELL.0, MEASURED_CELL.1)
        .expect("measured monospace metric is non-zero");

    Scene::Container(
        ContainerNode::new(vec![
            grid(DEFAULT_TAG, CellMetric::DEFAULT, DEFAULT_POS, DEFAULT_SIZE),
            grid(MEASURED_TAG, measured, MEASURED_POS, MEASURED_SIZE),
        ])
        .with_tag(ROOT_TAG)
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface))),
    )
}

struct TextGridView;

impl WidgetCore for TextGridView {
    type State = ();
    type Event = ();

    /// Display-only geometry scaffold: the only addressable anchor is the
    /// no-op [`StubExternal`] at [`ROOT_TAG`]. The grids are paint-opaque
    /// geometry leaves (no interaction until the data-model round).
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-textgrid (R972 §5.41 cell-native TextGrid scaffold)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (geometry scaffold, no widget state)".to_string()
    }
}

impl WidgetA11y for TextGridView {
    /// No a11y nodes yet — an empty geometry grid carries no ARIA
    /// structure. The accessible cell/grid topology lands with the
    /// R969 cell data model (the content the a11y tree would convey),
    /// alongside paint and input. Returning empty is the honest state.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        Vec::new()
    }
}

impl WidgetView for TextGridView {
    type Renderer = HelloTextGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TextGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::Rect;

    /// The derived dims are a pure function of `rect` + metric — the
    /// R969 layout-derived SSOT. The running shell fills `rect` via the
    /// layout pass; here we set it directly to pin the winsize floor.
    #[test]
    fn default_grid_derives_80x24_from_its_rect() {
        let mut g = TextGridNode::new(CellMetric::DEFAULT);
        g.rect = Rect::new(DEFAULT_POS.0, DEFAULT_POS.1, DEFAULT_SIZE.0, DEFAULT_SIZE.1);
        assert_eq!(g.cell_metric(), CellMetric::DEFAULT);
        assert_eq!(g.cols(), 80);
        assert_eq!(g.rows(), 24);
    }

    #[test]
    fn measured_grid_derives_40x20_from_its_rect() {
        let metric = CellMetric::new(MEASURED_CELL.0, MEASURED_CELL.1).expect("non-zero");
        let mut g = TextGridNode::new(metric);
        g.rect = Rect::new(MEASURED_POS.0, MEASURED_POS.1, MEASURED_SIZE.0, MEASURED_SIZE.1);
        assert_eq!(g.cols(), 40); // 360 / 9
        assert_eq!(g.rows(), 20); // 360 / 18
    }

    #[test]
    fn empty_rect_has_zero_dims() {
        // Before the layout pass fills `rect`, a fresh grid is 0×0 — the
        // winsize is strictly layout-derived (no speculative default).
        let g = TextGridNode::new(CellMetric::DEFAULT);
        assert_eq!((g.cols(), g.rows()), (0, 0));
    }

    #[test]
    fn view_root_carries_the_paint_root_tag() {
        let scene = pinion_core::Owner::new().run(|| view((), &Frame::new()));
        assert!(scene.contains_tag(ROOT_TAG));
        assert!(scene.contains_tag(DEFAULT_TAG));
        assert!(scene.contains_tag(MEASURED_TAG));
    }
}
