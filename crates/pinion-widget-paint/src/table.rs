//! R707 §5.50 — backend-agnostic data-table paint composition.
//!
//! Composes the data-grid visual: a header band (one
//! [`columnheader`](pinion_core::scene) cell per column) above a body of
//! data rows, each row a horizontal strip of cells. The selected row is
//! washed with an accent tint; odd rows carry a subtle
//! [`ColorRole::SurfaceContainer`] zebra stripe; the active-descendant
//! cell gets the shell's R694 focus-ring treatment.
//!
//! ## Naming
//!
//! Mirrors [`crate::datepicker`]: a [`TableStyle`] carrier struct with
//! [`TableStyle::m3`] defaults and a [`view_table`] fn that produces a
//! [`Scene`] fragment the binding's outer view-fn wraps in its root
//! container.
//!
//! ## Structure (tags)
//!
//! - The header band is tagged `"<tag>_hrow"` (the header `row` node);
//!   each column header inside it is a presentational cell tagged
//!   `"<tag>_ch<col>"` (column `0..cols`) so the binding's `access_node`
//!   walker can attach the header `row` + `columnheader` nodes.
//! - Each data row is a strip tagged `"<tag>_row<row>"` so the
//!   `access_node` walker can attach the `row` node (and so the strip
//!   carries the selected / zebra fill).
//! - Each real cell is a hit-test target tagged `"<tag>#<row>_<col>"`
//!   (the R51.41 composite paint convention → the
//!   [`InputRouter`](pinion_runtime::InputRouter) `'#'`-split routes a
//!   click on cell `(r, c)` to the table's `"<r>_<c>:<EventName>"` send).

use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::radio::RadioState;
use pinion_core::Scene;

/// R707 §5.50 — Material-3 data-table paint dimensions. Mirrors the
/// [`crate::datepicker::DatePickerStyle`] carrier pattern so binding
/// callers see a uniform `Style` surface across the widget catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableStyle {
    /// Uniform column width in logical pixels (default 120). Per-column
    /// widths / column resizing are deferred axes; v1 tiles columns at a
    /// fixed width.
    pub col_width: u32,
    /// Data-row height in logical pixels (default 36 ≈ M3 dense list
    /// row).
    pub row_height: u32,
    /// Header-band height in logical pixels (default 40).
    pub header_height: u32,
    /// Cell label font size in logical pixels (default 14 ≈ M3
    /// `body-medium`).
    pub label_size_px: u32,
    /// Header label font size in logical pixels (default 14 ≈ M3
    /// `title-small`).
    pub header_size_px: u32,
    /// Horizontal text inset inside each cell in logical pixels
    /// (default 12).
    pub cell_pad_x: u32,
    /// Outer block padding on all four sides in logical pixels
    /// (default 8).
    pub block_pad: u32,
    /// Outer block corner radius in logical pixels (default 12 = M3
    /// large shape token).
    pub corner_radius: u32,
}

impl TableStyle {
    /// R707 §5.50 — Material-3 data-table defaults.
    #[must_use]
    pub const fn m3() -> Self {
        Self {
            col_width: 120,
            row_height: 36,
            header_height: 40,
            label_size_px: 14,
            header_size_px: 14,
            cell_pad_x: 12,
            block_pad: 8,
            corner_radius: 12,
        }
    }
}

impl Default for TableStyle {
    fn default() -> Self {
        Self::m3()
    }
}

/// R707 §5.50 — the tabular data a [`view_table`] call renders.
///
/// Groups the column headers and the row-major cell text into one unit
/// so the paint signature stays under the readable argument budget
/// (mirror of [`crate::datepicker::DisplayedMonth`]). Cell text is
/// borrowed: the binding owns an immutable dataset and forwards slices.
#[derive(Clone, Copy, Debug)]
pub struct TableData<'a> {
    /// Column header labels; `headers.len()` is the column count.
    pub headers: &'a [&'a str],
    /// Row-major cell text **in visual (display) order**. Each inner slice
    /// is one visual row; a cell beyond a row's length renders blank.
    pub rows: &'a [&'a [&'a str]],
    /// R730 §5.40 — the data-row index for each visual row (the table's
    /// [`order()`](pinion_core::widgets::table::TableExternal::order)
    /// permutation): `row_ids[v]` is the data row whose cells are in
    /// `rows[v]`. Cells / strips tag by `row_ids[v]` and the selection /
    /// per-row state lookups index by it, so the table's data-indexed
    /// selection survives a sort with no remap. An empty slice (or a
    /// too-short one) falls back to identity (`row_ids[v] == v`) — the
    /// unsorted R707 behaviour.
    pub row_ids: &'a [usize],
}

/// R707 §5.50 — row-strip fill for `state` + `selected` + zebra parity
/// via the M3 state-layer overlay matrix.
///
/// A selected row washes [`ColorRole::Surface`] toward
/// [`ColorRole::Accent`] (0.16 — an M3 "selected container" tint, since
/// the palette has no dedicated `secondaryContainer` role); an
/// unselected odd row carries a subtle [`ColorRole::SurfaceContainer`]
/// zebra stripe; an unselected even row is transparent (shows the block
/// fill). Hover (0.08) / pressed (0.12) overlays lerp toward
/// [`ColorRole::OnSurface`]; disabled fades toward
/// [`ColorRole::Surface`] (0.38). Identical weights to
/// [`crate::datepicker::day_cell_fill`] so the catalog's interactive
/// surfaces share one state-layer language.
#[must_use]
pub fn row_fill(theme: &Theme, state: RadioState, selected: bool, row_index: usize) -> Color {
    let base = if selected {
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16)
    } else if row_index % 2 == 1 {
        theme.resolve(ColorRole::SurfaceContainer)
    } else {
        // Transparent so an unselected even row shows the block fill.
        Color::rgba(0, 0, 0, 0)
    };
    crate::state_layer::state_layer(base, state, theme)
}

/// One cell: a left-aligned label inside a fixed-size box. `tag` is the
/// composite hit-test tag (`"<root>#<row>_<col>"`) for a data cell, or a
/// presentational header tag (`"<root>_ch<col>"`). The keyboard-focus
/// ring is the shell's job (R694 `paint_focus_ring` over the
/// active-descendant cell), so no focus state is threaded here.
fn cell(tag: &str, text: &str, fg: Color, size_px: u32, style: &TableStyle, height: u32) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(
            TextNode::styled(
                text.to_string(),
                Rect::default(),
                TextStyle::new().with_size_px(size_px).with_fg(fg),
            )
            .with_role(TextRole::Presentational),
        )])
        .with_tag(tag.to_string())
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Start)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(style.col_width, height))
                .with_padding(Rect::new(style.cell_pad_x, 0, style.cell_pad_x, 0)),
        ),
    )
}

/// R730 §5.50 — sort-direction indicator glyphs shown on the active sort
/// column's header (U+25B2 BLACK UP-POINTING TRIANGLE / U+25BC down).
/// Named consts + escapes per the non-ASCII-source rule.
const ASC_GLYPH: &str = "\u{25B2}";
const DESC_GLYPH: &str = "\u{25BC}";

/// R730 — one clickable column header. The outer container keeps the
/// presentational `"<tag>_ch<col>"` tag (the binding's `access_node`
/// walker attaches the `columnheader` node + bounds there, unchanged
/// since R707); it wraps an inner container carrying the **composite**
/// `"<tag>#h<col>"` tag so a click on the header routes through the
/// R51.42 `'#'`-split to the table's `"h<col>:<EventName>"` sort wire.
/// When `col` is the active sort key the inner row also paints a sort
/// glyph (▲ ascending / ▼ descending) after the label.
fn header_cell(
    tag: &str,
    col: usize,
    label: &str,
    sort: Option<(usize, bool)>,
    fg: Color,
    style: &TableStyle,
) -> Scene {
    let label_node = Scene::Text(
        TextNode::styled(
            label.to_string(),
            Rect::default(),
            TextStyle::new().with_size_px(style.header_size_px).with_fg(fg),
        )
        .with_role(TextRole::Presentational),
    );
    let mut inner_children = vec![label_node];
    if let Some((c, ascending)) = sort {
        if c == col {
            let glyph = if ascending { ASC_GLYPH } else { DESC_GLYPH };
            inner_children.push(Scene::Text(
                TextNode::styled(
                    glyph.to_string(),
                    Rect::default(),
                    TextStyle::new().with_size_px(style.header_size_px).with_fg(fg),
                )
                .with_role(TextRole::Presentational),
            ));
        }
    }
    let inner = Scene::Container(
        ContainerNode::new(inner_children)
            .with_tag(format!("{tag}#h{col}"))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_gap(4)
                    .with_size(Size::px(style.col_width, style.header_height))
                    .with_padding(Rect::new(style.cell_pad_x, 0, style.cell_pad_x, 0)),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![inner])
            .with_tag(format!("{tag}_ch{col}"))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    )
}

/// The header band: one clickable `columnheader` cell per column ([
/// `header_cell`]) on a raised [`ColorRole::SurfaceContainerHigh`]
/// surface. `sort` drives the active column's sort glyph.
fn header_row(
    tag: &str,
    headers: &[&str],
    sort: Option<(usize, bool)>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let fg = theme.resolve(ColorRole::OnSurface);
    let cells: Vec<Scene> = headers
        .iter()
        .enumerate()
        .map(|(col, label)| header_cell(tag, col, label, sort, fg, style))
        .collect();
    Scene::Container(
        ContainerNode::new(cells)
            // Tagged `"<tag>_hrow"` so the binding's `access_node` walker
            // can attach the header `row` node (and resolve its bounds);
            // not a composite `'#'` tag, so it is inert to hit routing.
            .with_tag(format!("{tag}_hrow"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    )
}

/// One data row: a horizontal strip tagged `"<tag>_row<row>"`, filled
/// `fill`, holding one cell per column tagged `"<tag>#<row>_<col>"` with
/// foreground `fg`. The caller precomputes `fill` ([`row_fill`]) and `fg`
/// from the row's interaction state so this stays under the argument
/// budget.
fn data_row(
    tag: &str,
    data_id: usize,
    cells_text: &[&str],
    cols: usize,
    fill: Color,
    fg: Color,
    style: &TableStyle,
) -> Scene {
    // R730 §5.40 — cells / strip are tagged by **data-row id** (not the
    // visual position), so a click on a sorted row routes to the right
    // data row's `"<data_id>_<col>"` send wire and the table's
    // data-indexed selection stays correct without a remap. When unsorted
    // (`data_id == visual`) the tags are identical to the R707 scheme.
    let cells: Vec<Scene> = (0..cols)
        .map(|col| {
            let text = cells_text.get(col).copied().unwrap_or("");
            cell(
                &format!("{tag}#{data_id}_{col}"),
                text,
                fg,
                style.label_size_px,
                style,
                style.row_height,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(cells)
            .with_tag(format!("{tag}_row{data_id}"))
            .with_style(BoxStyle::filled(fill))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    )
}

/// Foreground color for a row's cells given its interaction `state`:
/// muted when disabled, the standard on-surface color otherwise.
fn row_fg(theme: &Theme, state: RadioState) -> Color {
    if matches!(state, RadioState::Disabled) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    }
}

/// R707 §5.50 — compose the M3 data-table paint scene fragment.
///
/// # Arguments
///
/// - `tag` — composite paint-root tag; header cells are `"<tag>_ch<col>"`,
///   row strips `"<tag>_row<row>"`, and data cells `"<tag>#<row>_<col>"`.
/// - `data` — the [`TableData`] (column headers + row-major cell text);
///   paired with
///   [`TableExternal`](pinion_core::widgets::table::TableExternal)'s
///   `rows` / `cols` / `cell.<r>.<c>` introspect slots.
/// - `row_selected` — per-row selection bitmap, indexed by **data** row
///   (parallel to `row_states`). A `true` row strip is washed with the
///   accent tint; a row outside the slice defaults to unselected. One
///   path serves both single-select (one bit set) and R735 multi-select.
/// - `row_states` — per-row [`RadioState`] interaction projections,
///   indexed by row (the binding reads them from the table's per-row
///   `state.<r>` introspect slots). A row outside the slice bounds
///   defaults to [`RadioState::Idle`].
/// - `theme` — current [`Theme`] palette; the binding resolves it via
///   [`pinion_core::theme::use_theme`] and forwards.
/// - `style` — [`TableStyle`] dimension carrier.
///
/// # R730 sort
///
/// `data.row_ids` carries each visual row's data-row index (the table's
/// [`order()`](pinion_core::widgets::table::TableExternal::order)
/// permutation) so cells / strips tag by data id and the data-indexed
/// selection survives a sort with no remap (identity fallback when
/// empty). `sort` is the active sort key `(col, ascending)` for the
/// header glyph.
///
/// # Returns
///
/// A [`Scene::Container`] (Column) holding the header band and the data
/// rows, carrying `tag` on the outer block so the composite root is
/// paint-addressable.
#[must_use]
pub fn view_table(
    tag: &str,
    data: TableData<'_>,
    row_selected: &[bool],
    row_states: &[RadioState],
    sort: Option<(usize, bool)>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let cols = data.headers.len();
    let header = header_row(tag, data.headers, sort, theme, style);
    let mut children: Vec<Scene> = Vec::with_capacity(data.rows.len() + 1);
    children.push(header);
    for (visual, cells_text) in data.rows.iter().enumerate() {
        // Data-row id for this visual position (identity fallback).
        let data_id = data.row_ids.get(visual).copied().unwrap_or(visual);
        let state = row_states.get(data_id).copied().unwrap_or(RadioState::Idle);
        let selected = row_selected.get(data_id).copied().unwrap_or(false);
        // Zebra parity is **visual** so the stripe pattern stays stable
        // across re-sorts; selection / state are **data-indexed**.
        let fill = row_fill(theme, state, selected, visual);
        let fg = row_fg(theme, state);
        children.push(data_row(tag, data_id, cells_text, cols, fill, fg, style));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_string())
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(style.corner_radius),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_padding(Rect::new(
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                    )),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    const HEADERS: [&str; 3] = ["Name", "Round", "Status"];
    const ROWS: [[&str; 3]; 3] = [
        ["Tabs", "R690", "Done"],
        ["Menu", "R691", "Done"],
        ["Table", "R707", "Active"],
    ];

    fn light() -> Theme {
        Theme::light()
    }

    fn data() -> ([&'static str; 3], Vec<&'static [&'static str]>) {
        let rows: Vec<&'static [&'static str]> = ROWS.iter().map(|r| &r[..]).collect();
        (HEADERS, rows)
    }

    fn all_idle() -> Vec<RadioState> {
        vec![RadioState::Idle; 3]
    }

    #[test]
    fn r707_style_m3_constants() {
        let s = TableStyle::m3();
        assert_eq!(s.col_width, 120);
        assert_eq!(s.row_height, 36);
        assert_eq!(s.header_height, 40);
        assert_eq!(s.corner_radius, 12);
    }

    #[test]
    fn r707_scene_carries_root_header_and_cell_tags() {
        let (headers, rows) = data();
        let scene = Owner::new().run(|| {
            view_table(
                "table",
                TableData { headers: &headers, rows: &rows, row_ids: &[] },
                &[],
                &all_idle(),
                None,
                &light(),
                &TableStyle::m3(),
            )
        });
        assert!(scene.contains_tag("table"), "composite root tag present");
        assert!(scene.contains_tag("table_hrow"), "header row strip present");
        for col in 0..3 {
            assert!(
                scene.contains_tag(&format!("table_ch{col}")),
                "column header {col} present (presentational, a11y bounds)",
            );
            assert!(
                scene.contains_tag(&format!("table#h{col}")),
                "column header {col} clickable sort tag present",
            );
        }
        for row in 0..3 {
            assert!(
                scene.contains_tag(&format!("table_row{row}")),
                "row strip {row} present",
            );
            for col in 0..3 {
                assert!(
                    scene.contains_tag(&format!("table#{row}_{col}")),
                    "cell {row}_{col} present",
                );
            }
        }
        // No phantom 4th row / column.
        assert!(!scene.contains_tag("table_row3"));
        assert!(!scene.contains_tag("table#0_3"));
    }

    fn collect_text(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for child in &c.children {
                    collect_text(child, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn r730_sort_glyph_only_on_active_column() {
        let (headers, rows) = data();
        let theme = light();
        let unsorted = Owner::new().run(|| {
            view_table(
                "table",
                TableData { headers: &headers, rows: &rows, row_ids: &[] },
                &[],
                &all_idle(),
                None,
                &theme,
                &TableStyle::m3(),
            )
        });
        let mut t = Vec::new();
        collect_text(&unsorted, &mut t);
        assert!(!t.iter().any(|s| s == ASC_GLYPH || s == DESC_GLYPH), "no glyph when unsorted");

        let sorted = Owner::new().run(|| {
            view_table(
                "table",
                TableData { headers: &headers, rows: &rows, row_ids: &[] },
                &[],
                &all_idle(),
                Some((1, true)),
                &theme,
                &TableStyle::m3(),
            )
        });
        let mut t2 = Vec::new();
        collect_text(&sorted, &mut t2);
        assert_eq!(t2.iter().filter(|s| *s == ASC_GLYPH).count(), 1, "one ascending glyph");
        assert!(!t2.iter().any(|s| s == DESC_GLYPH), "no descending glyph for ascending sort");
    }

    #[test]
    fn r730_row_ids_tag_strips_by_data_id_in_visual_order() {
        // Caller passes rows already in visual (sorted) order with the
        // parallel data-id permutation; strips tag by data id.
        let (headers, all) = data();
        // Visual order = data rows [2, 0, 1].
        let reordered: Vec<&[&str]> = vec![all[2], all[0], all[1]];
        let scene = Owner::new().run(|| {
            view_table(
                "table",
                TableData { headers: &headers, rows: &reordered, row_ids: &[2, 0, 1] },
                &[],
                &all_idle(),
                Some((0, true)),
                &light(),
                &TableStyle::m3(),
            )
        });
        let Scene::Container(root) = &scene else { panic!("root container") };
        // children[0] = header band; children[1..] = data rows in visual order.
        let strip_tag = |i: usize| {
            let Scene::Container(c) = &root.children[i] else { panic!("row strip") };
            c.tag.clone().unwrap()
        };
        assert_eq!(strip_tag(1), "table_row2", "1st visual row = data row 2");
        assert_eq!(strip_tag(2), "table_row0", "2nd visual row = data row 0");
        assert_eq!(strip_tag(3), "table_row1", "3rd visual row = data row 1");
    }

    #[test]
    fn r707_selected_row_fill_differs_from_unselected() {
        let theme = light();
        let unselected = row_fill(&theme, RadioState::Idle, false, 0);
        let selected = row_fill(&theme, RadioState::Idle, true, 0);
        assert_ne!(selected, unselected, "selected row washes differently");
        // Even unselected resting row is transparent (shows block fill).
        assert_eq!(unselected, Color::rgba(0, 0, 0, 0));
    }

    #[test]
    fn r707_zebra_stripe_on_odd_rows() {
        let theme = light();
        let even = row_fill(&theme, RadioState::Idle, false, 0);
        let odd = row_fill(&theme, RadioState::Idle, false, 1);
        assert_eq!(even, Color::rgba(0, 0, 0, 0), "even row transparent");
        assert_eq!(
            odd,
            theme.resolve(ColorRole::SurfaceContainer),
            "odd row carries the zebra stripe",
        );
    }

    #[test]
    fn r707_hover_overlay_lerps_toward_on_surface() {
        let theme = light();
        let expected = theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16)
            .lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(row_fill(&theme, RadioState::Hover, true, 0), expected);
    }
}
