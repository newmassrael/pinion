// R990 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (ToolbarExternal, aria-colcount, aria-disabled, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-column-visibility` — R990 §5.27 §5.38 §5.40 **data-grid column
//! hide / show**.
//!
//! ## Why this binding exists
//!
//! Phase B common-catalog / DCC axis: the **column chooser** every data grid,
//! asset browser, and inspector ships — toggle which columns are shown, and the
//! grid (and its AT tree) reflects the change. This is the first pinion consumer
//! of column visibility, built as **binding-level composition** of the existing
//! [`view_table`] paint:
//!
//! * **the model** — a shared `Signal<Vec<bool>>` ([`use_visibility`]), one bit
//!   per *source* column. The chooser, the view, and the reducer all read it.
//! * **the chooser** — the R692 [`ToolbarExternal`] (the *primary* external), one
//!   control per column. Its `aria-pressed` *and* its disabled state are both
//!   **reflective** (read from the model each frame, like `hello-textarea`'s
//!   formatting toggles): `pressed[i] = visible[i]`, and the sole remaining
//!   visible column's control is **disabled** — you cannot hide every column.
//!   This is the *second* consumer of the R989 reflective toolbar disabled axis.
//! * **the grid** — the binding filters the headers + each row's cells to the
//!   visible columns and hands the projection to [`view_table`]; the paint and
//!   the [`grid_table_nodes`] a11y tree then carry only the visible columns
//!   (`aria-colcount` drops as columns hide). No grid-paint change was needed —
//!   the visual<->source column mapping lives here (a 2nd column-visibility
//!   consumer would lift it to a substrate, per [[abstraction-needs-second-consumer]]).
//!
//! ## AI clients
//!
//! `query("/external/pressed.<i>")` is not used (the chooser is reflective);
//! instead the model is observable as scene-as-data — the count readout
//! (`colvis_count`) and `scene/access` (the grid's `columnheader` count = the
//! visible column count; the toolbar controls' `aria-pressed` / `aria-disabled`).
//! A left click on `cols#<i>` (or `invoke("send", "<i>:PointerUp")`) toggles
//! column `i`'s visibility; the reducer drops the toggle when it would hide the
//! last column.

use pinion_a11y::{
    grid_table_nodes, toolbar_button_nodes, AccessAction, AccessFocus, AccessNode, GridCell,
    GridColumn, GridRow, ToolbarControl, WidgetA11y,
};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, FlexDirection, LayoutStyle, TextStyle};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::toolbar::{read_roving_focus, ToolItem, ToolbarExternal};
use pinion_core::{Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::{view_table, TableData, TableSelection, TableStyle};
use pinion_widget_paint::toolbar::{composite_item_tag as col_ctl_tag, view_toolbar, ToolbarStyle};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloColumnVisibilityRenderer, HelloColumnVisibilityRendererError);

const WIN_W: u32 = 620;
const WIN_H: u32 = 420;
const THEME_TAG: &str = "app";

/// The column-chooser [`ToolbarExternal`] scope tag (the primary external):
/// controls paint as `cols#<i>` and `cols.command` is the toggle intent.
const COLS_TAG: &str = "cols";
/// The grid paint root: header cells paint `grid_ch<vp>`, strips `grid_row<r>`,
/// data cells `grid#<r>_<vp>` (vp = the *visual* column position).
const GRID_TAG: &str = "grid";
/// The scene-as-data count readout tag.
const COUNT_TAG: &str = "colvis_count";

/// The grid's source columns (a file-browser-ish schema).
const HEADERS: [&str; 5] = ["Name", "Type", "Size", "Modified", "Owner"];
/// Source column count.
const NCOLS: usize = HEADERS.len();
/// Data-row count.
const NROWS: usize = 6;
/// The toggle intent the chooser emits, dotted with its scope tag (R51.173).
const COMMAND_INTENT_TAG_FULL: &str = pinion_core::intent_tag!("cols", "command");
/// The [`use_visibility`] holder cache key.
const VIS_KEY: &str = "column_visibility.bits";

const NAMES: [&str; NROWS] =
    ["report.pdf", "photo.png", "notes.txt", "build.rs", "data.csv", "movie.mp4"];
const TYPES: [&str; NROWS] = ["PDF", "Image", "Text", "Rust", "CSV", "Video"];
const SIZES: [&str; NROWS] = ["2.1 MB", "880 KB", "4 KB", "1 KB", "32 KB", "1.4 GB"];
const MODIFIED: [&str; NROWS] =
    ["2026-06-01", "2026-05-30", "2026-06-10", "2026-06-18", "2026-04-22", "2026-03-09"];
const OWNERS: [&str; NROWS] = ["coin", "coin", "alex", "coin", "alex", "guest"];

/// Cell text for source `(row, col)`.
fn cell_text(row: usize, col: usize) -> String {
    let table = [NAMES, TYPES, SIZES, MODIFIED, OWNERS];
    table[col][row].to_string()
}

fn table_style() -> TableStyle {
    TableStyle { col_width: 110, ..TableStyle::m3() }
}

/// The shared per-column visibility model (one bit per source column). The
/// chooser, the view, and the reducer all reach the same `Rc` through this hook.
fn use_visibility() -> Rc<Signal<Vec<bool>>> {
    Owner::current()
        .expect("use_visibility requires an active Owner scope")
        .cache(VIS_KEY, || Signal::new(vec![true; NCOLS]))
}

/// The source columns currently visible, in source order.
fn visible_cols(vis: &[bool]) -> Vec<usize> {
    (0..NCOLS).filter(|&c| vis.get(c).copied().unwrap_or(false)).collect()
}

/// The count of visible columns.
fn visible_count(vis: &[bool]) -> usize {
    vis.iter().filter(|&&v| v).count()
}

/// R989 reflective disabled axis (2nd consumer): the chooser control for the
/// **sole** remaining visible column is disabled — hiding it would leave the
/// grid with no columns. A hidden column is never disabled (you can always show
/// it back). This is the single source the view paints *and* the reducer gates
/// on, so a greyed control is genuinely inoperable.
fn col_disabled(vis: &[bool]) -> Vec<bool> {
    let only_one = visible_count(vis) == 1;
    (0..NCOLS).map(|c| vis.get(c).copied().unwrap_or(false) && only_one).collect()
}

// ----------------------------------------------------------------------------
// View
// ----------------------------------------------------------------------------

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ColsState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let tstyle = table_style();
    let vis = use_visibility().get();
    let visible = visible_cols(&vis);
    let disabled = col_disabled(&vis);

    // The column chooser: one control per source column, reflective
    // pressed = visible, reflective disabled = sole-visible.
    let chooser = view_toolbar(
        COLS_TAG,
        &HEADERS,
        &vis,
        &disabled,
        state.cols_focus,
        state.cols_focused,
        &theme,
        &ToolbarStyle::m3_default().with_focusable(true),
    );

    let count = Scene::Text(
        TextNode::styled(
            format!("{} of {NCOLS} columns shown", visible.len()),
            Rect::default(),
            TextStyle::new().with_size_px(14).with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(COUNT_TAG),
    );

    // Project the visible columns into the eager `view_table` paint. The owned
    // cell text outlives the borrowed `&[&[&str]]` the paint takes.
    let headers_v: Vec<&str> = visible.iter().map(|&c| HEADERS[c]).collect();
    let cell_owned: Vec<Vec<String>> = (0..NROWS)
        .map(|r| visible.iter().map(|&c| cell_text(r, c)).collect())
        .collect();
    let cell_refs: Vec<Vec<&str>> =
        cell_owned.iter().map(|row| row.iter().map(String::as_str).collect()).collect();
    let rows_slices: Vec<&[&str]> = cell_refs.iter().map(Vec::as_slice).collect();
    let row_states = [RadioState::Idle; NROWS];
    let row_sel = [false; NROWS];
    let grid = view_table(
        GRID_TAG,
        TableData { headers: &headers_v, rows: &rows_slices, row_ids: &[] },
        TableSelection { rows: &row_sel, cells: None },
        &row_states,
        None,
        &theme,
        &tstyle,
    );

    Scene::Container(
        ContainerNode::new(vec![chooser, count, grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(12)
                    .with_padding(Rect::new(14, 14, 14, 14)),
            ),
    )
}

// ----------------------------------------------------------------------------
// Binding
// ----------------------------------------------------------------------------

/// Copy projection of the chooser toolbar's roving focus posture. The
/// visibility model is read from [`use_visibility`] in the view + a11y, not
/// projected here.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
struct ColsState {
    cols_focus: usize,
    cols_focused: bool,
}

struct ColumnVisibilityView;

impl WidgetCore for ColumnVisibilityView {
    type State = ColsState;
    type Event = ();

    /// Primary = the column chooser (one command control per source column).
    fn create_external() -> Box<dyn External> {
        Box::new(ToolbarExternal::new(vec![ToolItem::Command; NCOLS]))
    }

    fn tag() -> &'static str {
        COLS_TAG
    }

    /// Project the chooser's roving cursor + group-focus posture off the
    /// primary external.
    fn read_state(scene: &Scene) -> ColsState {
        let mut out = ColsState::default();
        // R990.1 — the chooser's roving cursor + group focus via the lifted
        // `read_roving_focus` reader (the toolbar peer of `read_selected`).
        if let Some(intro) = scene.find_external_with_tag(COLS_TAG).and_then(|n| n.handle.introspect()) {
            (out.cols_focus, out.cols_focused) = read_roving_focus(intro);
        }
        out
    }

    fn view(state: ColsState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-column-visibility (R990 §5.27 §5.38 §5.40 column hide/show)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, modifiers: Modifiers) -> bool {
        if focused != Some(COLS_TAG) {
            return false;
        }
        pinion_core::input::forward_key_to_field(scene, COLS_TAG, key, modifiers)
    }

    /// The chooser's `"command"` toggles that column's visibility on the shared
    /// model (a reactive side effect, no `Command` emitted). The reducer is the
    /// operable barrier: it gates on the *same* reflective [`col_disabled`] mask
    /// the view paints, so the sole-visible column cannot be hidden however the
    /// toggle is driven (the toolbar emits regardless — it cannot see the mask).
    fn update(_state: ColsState, intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == COMMAND_INTENT_TAG_FULL {
            if let IntrospectValue::Text(idx) = &intent.payload {
                if let Ok(col) = idx.parse::<usize>() {
                    let vis_rc = use_visibility();
                    let snap = vis_rc.get();
                    if col_disabled(&snap).get(col) == Some(&false) {
                        vis_rc.set_with(|prev| {
                            let mut next = prev.clone();
                            if let Some(b) = next.get_mut(col) {
                                *b = !*b;
                            }
                            next
                        });
                    }
                }
            }
        }
        Vec::new()
    }

    fn fmt_state_log(state: &ColsState) -> String {
        format!("cols_focus={} cols_focused={}", state.cols_focus, state.cols_focused)
    }
}

impl WidgetA11y for ColumnVisibilityView {
    /// The forest: the chooser `toolbar` (each control carrying reflective
    /// `aria-pressed` = visible + `aria-disabled` on the sole-visible column) +
    /// the `grid` projected to the visible columns (so `aria-colcount` drops as
    /// columns hide). Two independent roots.
    fn access_node(state: &ColsState, focused: Option<&str>) -> Vec<AccessNode> {
        let vis = use_visibility().get();
        let visible = visible_cols(&vis);
        let disabled = col_disabled(&vis);

        // Chooser toolbar — reflective pressed + disabled.
        let ctl_tags: Vec<String> = (0..NCOLS).map(|i| col_ctl_tag(COLS_TAG, i)).collect();
        let controls: Vec<ToolbarControl<'_>> = (0..NCOLS)
            .map(|i| ToolbarControl {
                tag: &ctl_tags[i],
                name: Some(HEADERS[i]),
                checked: Some(vis.get(i).copied().unwrap_or(false)),
                disabled: disabled[i],
            })
            .collect();
        let cols_focus = state.cols_focused.then_some(state.cols_focus);
        let mut nodes = toolbar_button_nodes(COLS_TAG, "Columns", &controls, cols_focus);

        // Grid projected to the visible columns (vp = visual position).
        let columns: Vec<GridColumn> = visible
            .iter()
            .enumerate()
            .map(|(vp, &src)| GridColumn {
                tag: format!("{GRID_TAG}_ch{vp}"),
                label: HEADERS[src].to_owned(),
                sort: None,
            })
            .collect();
        let rows: Vec<GridRow> = (0..NROWS)
            .map(|r| GridRow {
                tag: format!("{GRID_TAG}_row{r}"),
                selected: false,
                state: RadioState::Idle,
                cells: visible
                    .iter()
                    .enumerate()
                    .map(|(vp, &src)| GridCell {
                        tag: format!("{GRID_TAG}#{r}_{vp}"),
                        name: format!("{}: {}", HEADERS[src], cell_text(r, src)),
                        focused: false,
                        selected: None,
                    })
                    .collect(),
            })
            .collect();
        nodes.extend(grid_table_nodes(
            GRID_TAG,
            "Files",
            false,
            &format!("{GRID_TAG}_hrow"),
            &columns,
            &rows,
        ));
        let _ = focused;
        nodes
    }

    /// Roving active descendant for the chooser when it owns focus.
    fn access_focus_target(state: &ColsState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(COLS_TAG) {
            return Some(AccessFocus::composite(
                COLS_TAG,
                col_ctl_tag(COLS_TAG, state.cols_focus.min(NCOLS - 1)),
            ));
        }
        focused.map(AccessFocus::atomic)
    }

    /// AT child activation for a chooser control (`cols#<i>`): `Click` toggles
    /// the column; `Focus` records the AT-side active descendant.
    fn access_child_invoke(scene: &mut Scene, parent_tag: &str, sub_tag: &str, action: AccessAction) -> bool {
        if parent_tag != COLS_TAG {
            return false;
        }
        let Some(intro) = scene
            .find_external_with_tag_mut(COLS_TAG)
            .and_then(|n| n.handle.introspect_mut())
        else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                let _ = intro.invoke("send", IntrospectValue::Text(format!("{sub_tag}:PointerUp")));
                true
            }
            AccessAction::Focus => {
                if let Ok(i) = sub_tag.parse::<i64>() {
                    let _ = intro.intervene("focus", IntrospectValue::Int(i));
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for ColumnVisibilityView {
    type Renderer = HelloColumnVisibilityRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<ColumnVisibilityView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;

    fn by_tag<'a>(nodes: &'a [AccessNode], tag: &str) -> &'a AccessNode {
        nodes.iter().find(|n| n.tag == tag).expect("node present")
    }

    fn set_vis(bits: [bool; NCOLS]) {
        use_visibility().set(bits.to_vec());
    }

    // ----- the visibility model + disabled mask -----

    #[test]
    fn disabled_only_the_sole_visible_column() {
        // Two visible -> none disabled.
        assert_eq!(col_disabled(&[true, true, false, false, false]), [false; NCOLS]);
        // One visible -> only that column disabled (cannot hide the last).
        assert_eq!(
            col_disabled(&[false, false, true, false, false]),
            [false, false, true, false, false]
        );
        // A hidden column is never disabled even when one is visible.
        assert_eq!(visible_count(&[false, false, true, false, false]), 1);
    }

    // ----- the reducer toggles + gate -----

    fn cmd(col: usize) -> Intent {
        Intent::new_static("cols.command", IntrospectValue::Text(col.to_string()))
    }

    #[test]
    fn command_toggles_column_visibility() {
        Owner::new().run(|| {
            let vis = use_visibility();
            set_vis([true; NCOLS]);
            let _ = ColumnVisibilityView::update(ColsState::default(), &cmd(1)); // hide Type
            assert!(!vis.get()[1], "column 1 hidden");
            assert_eq!(visible_count(&vis.get()), NCOLS - 1, "one fewer visible");
            let _ = ColumnVisibilityView::update(ColsState::default(), &cmd(1)); // show Type
            assert!(vis.get()[1], "column 1 shown again");
        });
    }

    #[test]
    fn cannot_hide_the_last_visible_column() {
        Owner::new().run(|| {
            let vis = use_visibility();
            // Only column 0 visible; hiding it is a no-op (disabled).
            set_vis([true, false, false, false, false]);
            let _ = ColumnVisibilityView::update(ColsState::default(), &cmd(0));
            assert!(vis.get()[0], "the sole visible column cannot be hidden");
            assert_eq!(visible_count(&vis.get()), 1, "still one visible column");
        });
    }

    // ----- a11y: grid colcount + toolbar reflective pressed/disabled -----

    #[test]
    fn a11y_grid_columnheaders_track_visibility() {
        let nodes = Owner::new().run(|| {
            set_vis([true; NCOLS]);
            ColumnVisibilityView::access_node(&ColsState::default(), Some(COLS_TAG))
        });
        let ch = nodes.iter().filter(|n| n.role == AriaRole::ColumnHeader).count();
        assert_eq!(ch, NCOLS, "all columns are columnheaders when all visible");

        let hidden = Owner::new().run(|| {
            set_vis([true, false, true, false, true]);
            ColumnVisibilityView::access_node(&ColsState::default(), Some(COLS_TAG))
        });
        let ch2 = hidden.iter().filter(|n| n.role == AriaRole::ColumnHeader).count();
        assert_eq!(ch2, 3, "hiding two columns drops the columnheader count");
        // The visible headers are Name / Size / Owner (cols 0, 2, 4).
        let names: Vec<&str> = hidden
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .filter_map(|n| n.name.as_deref())
            .collect();
        assert_eq!(names, vec!["Name", "Size", "Owner"], "the visible headers, in order");
    }

    #[test]
    fn a11y_toolbar_reflects_pressed_and_disabled() {
        let nodes = Owner::new().run(|| {
            set_vis([true, false, false, false, false]); // only Name visible
            ColumnVisibilityView::access_node(&ColsState::default(), Some(COLS_TAG))
        });
        let name_ctl = by_tag(&nodes, &col_ctl_tag(COLS_TAG, 0));
        assert_eq!(name_ctl.state.checked, Some(true), "Name control is aria-pressed (visible)");
        assert!(name_ctl.state.disabled, "the sole visible column's control is disabled");
        let type_ctl = by_tag(&nodes, &col_ctl_tag(COLS_TAG, 1));
        assert_eq!(type_ctl.state.checked, Some(false), "Type control is not pressed (hidden)");
        assert!(!type_ctl.state.disabled, "a hidden column's control stays operable");
    }
}
