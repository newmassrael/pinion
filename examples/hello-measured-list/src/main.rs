//! `hello-measured-list` — R1194 §5.27 **measured variable-height
//! virtualization consumer**.
//!
//! The R745 sibling [`hello-variable-list`] virtualizes rows whose heights
//! the caller already knows (a prefix-sum `RowOffsets` table built from an
//! explicit height slice). Real data rarely offers that: a wrapped log line,
//! a variable document paragraph, an asset thumbnail — their height is only
//! known *after* the row is laid out. R1194 adds that missing mode: a
//! **measured** list windows against a
//! [`MeasuredRowState`]
//! that starts from a single `EST` estimate and refines each row as the
//! runtime layout pass harvests its laid-out height back in — the
//! "layout-pass measurement round-trip"
//! (`TanStack Virtual` `measureElement` / `react-virtualized` `CellMeasurer`).
//!
//! To make the round-trip *observable* rather than dependent on a font
//! shaper, each row is a stack of `lines(i)` fixed-height strips, so its
//! natural height is exactly `lines(i) · STRIP_H` — but that height is **not
//! passed to the windowing**; it is discovered by laying the row out. The
//! estimate `EST` is deliberately wrong for most rows, so the total content
//! height (and the scrollbar thumb) visibly refine as rows are measured.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! Two independent witnesses, no pixels required (see
//! `tools/demos/r1194_measured_list.py`):
//!
//! - **Scene**: `scene/snapshot` reports only the windowed row nodes; each
//!   rendered `measured-row:<i>` slot's laid-out height equals the row's
//!   modeled height (the harvest read the *real* content height, not the
//!   estimate), and adjacent slot tops differ by the upper row's measured
//!   height (the refined offsets drive geometry).
//! - **Introspection**: the primary [`MeasuredListExternal`] exposes
//!   `measured_count` / `total_height` / `is_fully_measured` / `exact_total`
//!   so an agent can watch the estimate converge to the exact sum as it
//!   scrolls the whole list.

use std::rc::Rc;

use pinion_a11y::{AccessNode, WidgetA11y, windowed_list_nodes};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, SchemaArg, SchemaField, ThreadOwnership,
    int_of,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::measured_rows::{MeasuredRowState, use_measured_rows};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range_variable;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::virtual_list::view_measured_list;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloMeasuredListRenderer, HelloMeasuredListRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 520;
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key.
const THEME_TAG: &str = "app";
/// Total dataset size. Small enough that the demo can scroll through the
/// whole list and witness convergence to the exact content height, while
/// still far larger than the rendered window.
const N: usize = 120;
/// Height of one content strip (logical pixels). A row of `lines(i)` strips
/// is `lines(i) · STRIP_H` tall.
const STRIP_H: u32 = 22;
/// Distinct line counts a row cycles through (`1..=MAX_LINES`), so heights
/// span `STRIP_H..=MAX_LINES · STRIP_H` (22..=110 px).
const MAX_LINES: usize = 5;
/// Estimated per-row height for a not-yet-measured row. Deliberately `≠`
/// *every* tier height (22/44/66/88/110) **and** `≠` the dataset's average
/// row height (66), so (a) every row's first measurement genuinely changes
/// its height and (b) the total content height visibly refines from the
/// all-estimate baseline (`N · EST = 5760`) toward the exact sum
/// (`exact_total = 7920`) as rows are measured — the convergence witness.
const EST: u32 = 48;
/// Extra rows built above + below the strict visible window.
const OVERSCAN: usize = 2;
/// Scroll viewport width (frames + wraps each row).
const VIEWPORT_W: u32 = 300;
/// Scroll viewport height.
const VIEWPORT_H: u32 = 330;
/// Paint-root + a11y `list` container tag, and the primary External's tag.
const LIST_TAG: &str = "mlist";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "mlist_scroll";
/// Cache key for the reactive `MeasuredRowState`.
const MEASURED_KEY: &str = "mlist_measured";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "mlist_scrollbar";

/// Number of content lines (hence strips) in row `index` — cycles
/// `1..=MAX_LINES`. The *model*: the harvested height must equal
/// `lines(index) · STRIP_H`, but this count is never handed to the
/// windowing (that is the point — the height is discovered by layout).
fn lines(index: usize) -> usize {
    1 + index % MAX_LINES
}

/// The modeled natural height of row `index` (`lines · STRIP_H`). Mirrored by
/// the verification harness; the layout pass must measure exactly this.
fn model_height(index: usize) -> u32 {
    u32::try_from(lines(index)).unwrap_or(0) * STRIP_H
}

/// The exact total content height of the whole dataset once every row has
/// been measured — the value `MeasuredRowState::total_height` converges to.
fn exact_total() -> u32 {
    (0..N).map(model_height).sum()
}

/// One measured row: a **height-auto** column of `lines(index)` fixed-height
/// strips, so its natural height is `lines(index) · STRIP_H` — discovered by
/// the layout pass, not declared to the windowing. Each strip is tinted by
/// its tier so the variability reads at a glance. The slot wrapper
/// (`view_measured_list`) tags this row `measured-row:<index>` and leaves its
/// height free so the harvest reads the true content height.
fn build_row(index: usize, theme: &Theme) -> Scene {
    let n = lines(index);
    let fill = match index % MAX_LINES {
        0 => theme.resolve(ColorRole::SurfaceContainerLow),
        1 => theme.resolve(ColorRole::SurfaceContainer),
        2 => theme.resolve(ColorRole::SurfaceContainerHigh),
        3 => theme.resolve(ColorRole::SurfaceContainerHighest),
        _ => theme.resolve(ColorRole::ErrorContainer),
    };
    let strips: Vec<Scene> = (0..n)
        .map(|line| {
            let label = Scene::Text(TextNode::styled(
                strip_label(index, line, n),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(13)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            ));
            Scene::Container(
                ContainerNode::new(vec![label])
                    .with_style(BoxStyle::filled(fill))
                    .with_layout(
                        LayoutStyle::new()
                            .flex(FlexDirection::Row)
                            .with_align_items(AlignItems::Center)
                            // Height fixed, width auto → stretches to the slot
                            // width; the row column sums these to its natural
                            // height (no gap / padding on the column, so the
                            // sum is exactly `lines · STRIP_H`).
                            .with_size(Size::height_px(STRIP_H))
                            .with_padding(Rect::new(12, 0, 12, 0)),
                    ),
            )
        })
        .collect();
    // Height-auto column: its height resolves to the sum of the strips.
    Scene::Container(
        ContainerNode::new(strips).with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

/// Synthetic strip content: the row index, the line within the row, and the
/// row's line count, so a `scene/snapshot` readout makes the per-row height
/// legible.
fn strip_label(index: usize, line: usize, total_lines: usize) -> String {
    format!("Row {index:04} \u{00B7} line {}/{total_lines}", line + 1)
}

/// view-fn (§6.3): pure sync `() -> Scene`. `view_measured_list` invokes
/// [`build_row`] only for the windowed indices; the runtime harvests their
/// laid-out heights back into the shared [`MeasuredRowState`].
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let measured = use_measured_rows(MEASURED_KEY, N, EST);
    let theme = use_theme(THEME_TAG).theme_animated();

    // The windowed measured list. Reads `scroll_state`'s offset + the
    // refined `measured` heights, windows via `compute_visible_range_variable`,
    // builds only those rows, and carries `measured` so the layout pass finds
    // the harvest target.
    let list = view_measured_list(
        &scroll_state,
        &measured,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        OVERSCAN,
        |index| build_row(index, &theme),
    );

    // Scrollbar peer sized against the total extent (the sizer height the
    // layout pass wrote into `ScrollState::max_y`), which refines as rows are
    // measured. Shares the same `Rc<ScrollState>`.
    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    let list_root = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    Scene::Container(
        ContainerNode::new(vec![list_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// R1194 §5.27 — the primary, queryable External anchor. Display-only (no
/// paint, no intervene), but its `query` channel is the AI-first witness for
/// the measurement round-trip: it holds the same owner-cached
/// [`MeasuredRowState`] the view fn windows against, so an agent reads the
/// live measured-count / total-height / convergence without pixels.
#[derive(Debug)]
struct MeasuredListExternal {
    measured: Rc<MeasuredRowState>,
}

impl External for MeasuredListExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
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

impl ExternalIntrospect for MeasuredListExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("item_count", "int"),
                    SchemaField::new("estimated", "int"),
                    SchemaField::new("measured_count", "int"),
                    SchemaField::new("is_fully_measured", "bool"),
                    SchemaField::new("total_height", "int"),
                    SchemaField::new("exact_total", "int"),
                    SchemaField::parametric(
                        "model_height.<row>",
                        "int",
                        const { &[SchemaArg::open("row", "int")] },
                    ),
                    SchemaField::parametric(
                        "measured_height.<row>",
                        "int",
                        const { &[SchemaArg::open("row", "int")] },
                    ),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "item_count" => Some(IntrospectValue::Int(int_of(self.measured.item_count()))),
            "estimated" => Some(IntrospectValue::Int(i64::from(EST))),
            "measured_count" => Some(IntrospectValue::Int(int_of(self.measured.measured_count()))),
            "is_fully_measured" => Some(IntrospectValue::Bool(self.measured.is_fully_measured())),
            "total_height" => Some(IntrospectValue::Int(i64::from(
                self.measured.total_height(),
            ))),
            "exact_total" => Some(IntrospectValue::Int(i64::from(exact_total()))),
            _ => {
                // `model_height.<row>` / `measured_height.<row>` — the modeled
                // vs (nullable) measured height of a single row.
                if let Some(row) = path.strip_prefix("model_height.").and_then(parse_index) {
                    return Some(IntrospectValue::Int(i64::from(model_height(row))));
                }
                if let Some(row) = path.strip_prefix("measured_height.").and_then(parse_index) {
                    return Some(match self.measured.measured_height(row) {
                        Some(h) => IntrospectValue::Int(i64::from(h)),
                        None => IntrospectValue::Null,
                    });
                }
                None
            }
        }
    }

    /// Display-only: no writable state.
    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::UnknownPath)
    }
}

/// Parse a `<row>` sub-path segment to an in-range index, or `None`.
fn parse_index(seg: &str) -> Option<usize> {
    let i = seg.parse::<usize>().ok()?;
    (i < N).then_some(i)
}

struct MeasuredListView;

impl WidgetCore for MeasuredListView {
    type State = ();
    type Event = ();

    /// The primary External is the queryable [`MeasuredListExternal`],
    /// capturing the same owner-cached `MeasuredRowState` the view fn uses.
    fn create_external() -> Box<dyn External> {
        Box::new(MeasuredListExternal {
            measured: use_measured_rows(MEASURED_KEY, N, EST),
        })
    }

    /// Sibling `ScrollBarExternal` sharing the list's `Rc<ScrollState>`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![scrollbar_extra_external(
            use_scroll_state(SCROLL_KEY),
            SCROLLBAR_TAG,
        )]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-measured-list (R1194 §5.27 measured variable-height virtualization)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for MeasuredListView {
    /// WAI-ARIA virtualized `list`: `aria-setsize = N` with one `listitem`
    /// per rendered row, windowed from the same `MeasuredRowState` the view
    /// fn uses, so the a11y tree and painted tree never diverge on which rows
    /// exist.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let measured = use_measured_rows(MEASURED_KEY, N, EST);
        let window = compute_visible_range_variable(
            scroll_state.offset_y(),
            VIEWPORT_H,
            &measured.offsets(),
            OVERSCAN,
        );
        windowed_list_nodes(
            LIST_TAG,
            "Measured variable-height item list",
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
        )
    }
}

impl WidgetView for MeasuredListView {
    type Renderer = HelloMeasuredListRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<MeasuredListView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    fn run_view() -> Scene {
        Owner::new().run(|| view((), &Frame::default()))
    }

    fn find_scroll(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
        match scene {
            Scene::Scroll(s) => Some(s),
            Scene::Container(c) => c.children.iter().find_map(find_scroll),
            _ => None,
        }
    }

    #[test]
    fn view_wraps_a_measured_scroll_node() {
        let scene = run_view();
        let scroll = find_scroll(&scene).expect("view contains a Scene::Scroll");
        assert_eq!(scroll.viewport.w, VIEWPORT_W);
        assert_eq!(scroll.viewport.h, VIEWPORT_H);
        assert!(scroll.state.is_some(), "scroll carries the offset state");
        assert!(
            scroll.measured_rows.is_some(),
            "a measured list carries the harvest target",
        );
    }

    #[test]
    fn height_model_cycles_and_estimate_differs_from_every_tier() {
        assert_eq!(model_height(0), STRIP_H, "1 line");
        assert_eq!(model_height(4), 5 * STRIP_H, "5 lines");
        assert_eq!(model_height(5), STRIP_H, "cycles every MAX_LINES");
        // The estimate misses every tier, so every first measurement changes
        // the height and the total refines away from the baseline.
        for i in 0..MAX_LINES {
            assert_ne!(model_height(i), EST, "tier {i} height must differ from EST");
        }
        // The all-estimate baseline is not the exact total, so convergence is
        // observable.
        assert_ne!(u32::try_from(N).unwrap() * EST, exact_total());
    }

    #[test]
    fn a11y_list_reports_full_setsize_with_windowed_items() {
        let nodes = Owner::new().run(|| MeasuredListView::access_node(&(), None));
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(nodes[0].size_of_set, Some(u32::try_from(N).unwrap()));
        assert!(
            nodes.len() - 1 < 40,
            "only the rendered window has listitem nodes"
        );
        for item in &nodes[1..] {
            assert_eq!(item.role, AriaRole::ListItem);
            assert!(item.position_in_set.is_some());
        }
    }

    #[test]
    fn external_query_reports_the_measurement_state() {
        // In an Owner scope so `use_measured_rows` resolves the cached state.
        Owner::new().run(|| {
            let ext = MeasuredListExternal {
                measured: use_measured_rows(MEASURED_KEY, N, EST),
            };
            assert_eq!(
                ext.query("item_count"),
                Some(IntrospectValue::Int(int_of(N)))
            );
            assert_eq!(
                ext.query("estimated"),
                Some(IntrospectValue::Int(i64::from(EST)))
            );
            assert_eq!(
                ext.query("measured_count"),
                Some(IntrospectValue::Int(0)),
                "nothing measured before a layout pass runs",
            );
            assert_eq!(
                ext.query("is_fully_measured"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                ext.query("total_height"),
                Some(IntrospectValue::Int(i64::from(
                    u32::try_from(N).unwrap() * EST
                ))),
                "the pre-measurement total is the all-estimate baseline",
            );
            assert_eq!(
                ext.query("exact_total"),
                Some(IntrospectValue::Int(i64::from(exact_total()))),
            );
            assert_eq!(
                ext.query("model_height.4"),
                Some(IntrospectValue::Int(i64::from(5 * STRIP_H)))
            );
            assert_eq!(
                ext.query("measured_height.4"),
                Some(IntrospectValue::Null),
                "unmeasured row reports Null, not the estimate",
            );
            assert_eq!(ext.query("bogus"), None);
        });
    }
}
