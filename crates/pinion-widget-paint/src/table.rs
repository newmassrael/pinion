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
    /// Row-major cell text. Each inner slice is one row; a cell beyond a
    /// row's length renders blank.
    pub rows: &'a [&'a [&'a str]],
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
    match state {
        RadioState::Idle => base,
        RadioState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        RadioState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
        RadioState::Disabled => base.lerp(theme.resolve(ColorRole::Surface), 0.38),
    }
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

/// The header band: one `columnheader` cell per column, tagged
/// `"<tag>_ch<col>"`, on a raised [`ColorRole::SurfaceContainerHigh`]
/// surface.
fn header_row(tag: &str, headers: &[&str], theme: &Theme, style: &TableStyle) -> Scene {
    let fg = theme.resolve(ColorRole::OnSurface);
    let cells: Vec<Scene> = headers
        .iter()
        .enumerate()
        .map(|(col, label)| {
            cell(
                &format!("{tag}_ch{col}"),
                label,
                fg,
                style.header_size_px,
                style,
                style.header_height,
            )
        })
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
    row: usize,
    cells_text: &[&str],
    cols: usize,
    fill: Color,
    fg: Color,
    style: &TableStyle,
) -> Scene {
    let cells: Vec<Scene> = (0..cols)
        .map(|col| {
            let text = cells_text.get(col).copied().unwrap_or("");
            cell(
                &format!("{tag}#{row}_{col}"),
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
            .with_tag(format!("{tag}_row{row}"))
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
/// - `selected_row` — the selected row index, or `None`. The selected
///   row strip is washed with the accent tint.
/// - `row_states` — per-row [`RadioState`] interaction projections,
///   indexed by row (the binding reads them from the table's per-row
///   `state.<r>` introspect slots). A row outside the slice bounds
///   defaults to [`RadioState::Idle`].
/// - `theme` — current [`Theme`] palette; the binding resolves it via
///   [`pinion_core::theme::use_theme`] and forwards.
/// - `style` — [`TableStyle`] dimension carrier.
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
    selected_row: Option<usize>,
    row_states: &[RadioState],
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let cols = data.headers.len();
    let header = header_row(tag, data.headers, theme, style);
    let mut children: Vec<Scene> = Vec::with_capacity(data.rows.len() + 1);
    children.push(header);
    for (row, cells_text) in data.rows.iter().enumerate() {
        let state = row_states.get(row).copied().unwrap_or(RadioState::Idle);
        let selected = selected_row == Some(row);
        let fill = row_fill(theme, state, selected, row);
        let fg = row_fg(theme, state);
        children.push(data_row(tag, row, cells_text, cols, fill, fg, style));
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
                TableData { headers: &headers, rows: &rows },
                None,
                &all_idle(),
                &light(),
                &TableStyle::m3(),
            )
        });
        assert!(scene.contains_tag("table"), "composite root tag present");
        assert!(scene.contains_tag("table_hrow"), "header row strip present");
        for col in 0..3 {
            assert!(
                scene.contains_tag(&format!("table_ch{col}")),
                "column header {col} present",
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
