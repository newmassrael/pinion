// R837 §5.38 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (QtCharts, ModelMapper, CellValue, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-model-chart` — R1446: **a chart plots the field the user points it
//! at.** The forcing consumer for [`pinion_chart::ModelMapper`].
//!
//! ## What was missing
//!
//! Every other chart in the tree is fed a `Vec<Series>` built in code — the
//! charting crate and pinion's typed cell model had never met, so *what* a
//! chart plotted was a compile-time fact. This binding closes that: a
//! [`CellTable`] over a live `Vec<CellValue>` (the shape `hello-data-grid` and
//! `hello-property-grid` hold) is projected into series by a mapper whose x
//! field and y fields are **picker state**. Re-point the mapper and the same
//! data draws a different chart, with no rebuild.
//!
//! ## The two pickers (one skin, two selection models)
//!
//! * **x field** — a [`RadioGroupExternal`] (the primary), 1-of-N over
//!   `record #` plus one entry per column. `record #` is
//!   [`Field::Ordinal`]: an evenly-sampled block with no stored independent
//!   variable.
//! * **y fields** — one independent [`ToggleExternal`](pinion_core::widgets::toggle::ToggleExternal)
//!   per column ([`toggle_group`]), multi-select, one series each.
//!
//! Both rows paint through the same R756 [`chip_style`] skin — the chips
//! differ only in which selection model owns them, which is the honest
//! difference (an x axis has exactly one field; a chart has any number of
//! series). A column that is not a measure renders **muted**, so the reader
//! can see the mismatch before clicking it.
//!
//! The y-toggle extras are built here rather than through
//! [`toggle_group::extra_toggles`], which skips index 0 because it assumes the
//! first toggle is the binding's PRIMARY external. Here the primary is the x
//! radio group, so every toggle is an extra. Stated rather than worked around:
//! it is a real (narrow) limit of that helper, and one consumer is not the
//! gate for changing it.
//!
//! ## What this demo exists to show — the non-numeric column
//!
//! Qt reads a mapped model cell through `QVariant::toReal()`, which answers
//! `0.0` for anything that is not a number: point a y axis at a label column
//! and Qt draws a flat line **on the axis**, indistinguishable from measured
//! zeros. Toggle `Month` on here and the chart plots *nothing* for it, and the
//! status line says why — `Month: 8 text cells, not a measure`. The report is
//! scene data ([`Mapped::unreadable`] projected into the tagged status text),
//! so an AI client reads the diagnosis exactly as a sighted user does (§2 #7).
//!
//! ## Data input (§2 #2, AI-first)
//!
//! The cell block is writable over RPC:
//! `scene/intervene /records/value.<row>.<col>` takes a value of the column's
//! own [`CellKind`] (via [`CellKind::coerce`] — no silent coercion), and the
//! next frame re-maps, so the plotted series follows the edited cell. Typing
//! into a *grid* is `hello-data-grid`'s job; what was missing was never the
//! editor but the binding from cells to series, which is what R1446 added.
//!
//! ## Verification
//!
//! `tools/demos/r1446_model_chart.py`: boot → re-point x → toggle a measure on
//! and off → toggle the TEXT column on (the chart stays empty, the status names
//! the reason) → write a cell over RPC and watch the polyline move. All read as
//! scene-as-data, no pixels.

use pinion_a11y::{
    AccessNode, RadioCell, ToggleSegment, WidgetA11y, radiogroup_radio_nodes,
    toggle_button_group_nodes,
};
use pinion_chart::{CellTable, ChartStyle, Field, LineChart, Mapped, ModelMapper, numeric};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::interaction::InteractionState;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{CellKind, CellValue, Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::chip::{CHIP_HEIGHT, chip_layout, chip_style, selection_border};
use pinion_widget_paint::radio_composite as rc;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloModelChartRenderer, HelloModelChartRendererError);

const WIN_W: u32 = 940;
const WIN_H: u32 = 560;
const THEME_TAG: &str = "app";

// ── the cell model ────────────────────────────────────────────────────────

const NCOLS: usize = 4;
const NROWS: usize = 8;

/// Column headers. A `CellValue` block is values only — a column's name is the
/// binding's metadata, which is why [`ModelMapper::with_series`] takes one.
const COL_NAMES: [&str; NCOLS] = ["Month", "Revenue", "Cost", "Units"];

/// Per-column type. `Month` is deliberately `Text`: it is the witness that a
/// mapping never invents a number for a cell that has none.
const COL_KINDS: [CellKind; NCOLS] = [
    CellKind::Text,
    CellKind::Float,
    CellKind::Float,
    CellKind::Int,
];

/// `Owner::cache` key for the shared cell block.
const CELLS_KEY: &str = "model_chart.cells";

/// The seeded records — eight months of a small ledger.
fn boot_cells() -> Vec<CellValue> {
    const MONTHS: [&str; NROWS] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug"];
    const REVENUE: [f64; NROWS] = [120.0, 138.0, 131.0, 155.0, 149.0, 172.0, 168.0, 190.0];
    const COST: [f64; NROWS] = [88.0, 94.0, 91.0, 102.0, 99.0, 110.0, 108.0, 118.0];
    const UNITS: [i64; NROWS] = [30, 34, 33, 39, 37, 43, 42, 47];

    let mut cells = Vec::with_capacity(NROWS * NCOLS);
    for r in 0..NROWS {
        cells.push(CellValue::Text(MONTHS[r].to_owned()));
        cells.push(CellValue::Float(REVENUE[r]));
        cells.push(CellValue::Float(COST[r]));
        cells.push(CellValue::Int(UNITS[r]));
    }
    cells
}

/// The shared, mutable cell block — the one source of truth the view maps and
/// the [`RecordsExternal`] writes into.
fn use_cells() -> Rc<Signal<Vec<CellValue>>> {
    Owner::current()
        .expect("view / external run inside the root owner")
        .cache(CELLS_KEY, || Signal::new(boot_cells()))
}

// ── tags ──────────────────────────────────────────────────────────────────

/// The x-field radio group (PRIMARY external). Its cells are `x_field#<i>`,
/// which the R51.42 `'#'`-split routes back to this one coordinator.
const X_TAG: &str = "x_field";

/// One independent toggle per column (extras).
const Y_TAGS: [&str; NCOLS] = ["y_field_0", "y_field_1", "y_field_2", "y_field_3"];

/// The WAI-ARIA `group` label parent for the y toggles (a11y only).
const Y_GROUP_TAG: &str = "y_fields";

/// The cell block's RPC surface.
const RECORDS_TAG: &str = "records";

/// The status line — the SSOT for both the painted text and the `role=status`
/// live region.
const STATUS_TAG: &str = "status";

/// x options: index `0` is the record ordinal, index `i > 0` is column `i - 1`.
const X_OPTIONS: usize = NCOLS + 1;

/// Boot: x is the record ordinal, y is Revenue + Cost (the two measures that
/// share a magnitude, so the boot frame reads as a chart rather than as one
/// line and a flat one).
const BOOT_X: usize = 0;
const BOOT_Y: [bool; NCOLS] = [false, true, true, false];

/// Window-absolute plot region (the `pinion-chart` `build` contract).
const CHART_RECT: Rect = Rect::new(250, 46, WIN_W - 270, 320);

const TITLE_FONT_PX: u32 = 18;
const BODY_FONT_PX: u32 = 13;
const ROW_PITCH: u32 = 18;

// ── the mapping ───────────────────────────────────────────────────────────

/// Human name of an x option.
fn x_label(option: usize) -> &'static str {
    if option == 0 {
        "record #"
    } else {
        COL_NAMES[option - 1]
    }
}

/// The mapper the picker state describes — the whole of "what is plotted".
fn mapper(x_option: usize, y_on: [bool; NCOLS]) -> ModelMapper {
    let x = if x_option == 0 {
        Field::Ordinal
    } else {
        Field::At(x_option - 1)
    };
    let mut m = ModelMapper::new(x);
    for (col, on) in y_on.iter().enumerate() {
        if *on {
            m = m.with_series(col, COL_NAMES[col]);
        }
    }
    m
}

/// Whether column `col` currently reads as a measure — every one of its cells
/// answers [`pinion_chart::numeric`]. Asked of the DATA, not of
/// [`COL_KINDS`], so a cell written over RPC that does not read as a number
/// mutes its chip too.
fn is_measure(cells: &[CellValue], col: usize) -> bool {
    let table = CellTable::new(cells, NCOLS);
    (0..table.nrows()).all(|r| table.get(r, col).is_some_and(|c| numeric(c).is_some()))
}

/// The status line — one string for the painted text and the AT live region.
/// States what is plotted, how much of it, and (the point of the demo) every
/// column that could not be read, by name and by kind.
fn status_line(mapped: &Mapped, x_option: usize, y_on: [bool; NCOLS]) -> String {
    let plotted: usize = mapped.series.iter().map(|s| s.points.len()).sum();
    let names: Vec<&str> = (0..NCOLS)
        .filter(|c| y_on[*c])
        .map(|c| COL_NAMES[c])
        .collect();
    let y = if names.is_empty() {
        "(none)".to_owned()
    } else {
        names.join(", ")
    };
    let mut line = format!(
        "x = {}  |  y = {y}  |  {plotted} points plotted",
        x_label(x_option)
    );
    for note in unreadable_notes(mapped) {
        line.push_str("  |  ");
        line.push_str(&note);
    }
    line
}

/// One note per column that held cells the mapping could not read. This is
/// where Qt would have drawn zeros and said nothing.
fn unreadable_notes(mapped: &Mapped) -> Vec<String> {
    (0..NCOLS)
        .filter_map(|col| {
            let n = mapped.unreadable_in_col(col);
            (n > 0).then(|| {
                format!(
                    "{}: {n} {} cells, not a measure",
                    COL_NAMES[col],
                    COL_KINDS[col].name()
                )
            })
        })
        .collect()
}

// ── paint ─────────────────────────────────────────────────────────────────

fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        legend: true,
        x_ticks: 7,
        y_ticks: 5,
        ..ChartStyle::default()
    }
}

/// One picker chip. Generic over the interaction state because the two rows
/// are owned by two different selection models (`RadioState` / `ToggleState`)
/// but wear one skin; `measure` mutes a column that is not numeric, so the
/// mismatch is visible before it is clicked.
///
/// `focusable` is where the two rows genuinely part. A radio group is ONE Tab
/// stop with a roving active descendant (WAI-ARIA: the group owns the stop,
/// arrows move within it), so its chips are hit targets but not stops — the
/// strip container carries the stop. Independent toggles are each their own
/// stop, because Tab is how you reach the fourth one without touching the
/// other three.
fn field_chip<S: InteractionState + Copy>(
    tag: String,
    label: &str,
    selected: bool,
    measure: bool,
    focusable: bool,
    state: S,
    theme: &Theme,
) -> Scene {
    let base = if selected {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::rgba(0, 0, 0, 0)
    };
    let border = selection_border(theme, selected);
    let ink = if selected {
        theme.resolve(ColorRole::OnAccent)
    } else if measure {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    let text = Scene::Text(TextNode::styled(
        label.to_owned(),
        Rect::default(),
        TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(ink),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(tag)
            .with_style(chip_style(base, border, state, theme))
            .with_layout(chip_layout(Size::px(92, CHIP_HEIGHT), None).with_focusable(focusable)),
    )
}

/// A labelled row of chips at an absolute y.
///
/// `group_tag` names the chips' own container. The x row passes [`X_TAG`] so
/// the composite paint root is addressable as a whole (the R55.G.18 §5.49
/// convention `hello-radio-group` follows): `{path: "x_field"}` reaches the
/// picker, and AT bounds attach to the chip strip rather than the window. The
/// caption stays OUTSIDE that container so the composite's surface is the
/// chips, not the label beside them.
///
/// `group_focusable` makes that container the row's single Tab stop — true for
/// the x radio group (whose chips are not stops), false for the measures
/// (whose chips are).
fn chip_row(
    label: &str,
    y: u32,
    group_tag: Option<&'static str>,
    group_focusable: bool,
    chips: Vec<Scene>,
    theme: &Theme,
) -> Scene {
    let caption = Scene::Text(TextNode::styled(
        label.to_owned(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    let mut strip = ContainerNode::new(chips).with_layout(
        LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_align_items(AlignItems::Center)
            .with_gap(8)
            .with_focusable(group_focusable),
    );
    if let Some(tag) = group_tag {
        strip = strip.with_tag(tag);
    }
    Scene::Container(
        ContainerNode::new(vec![caption, Scene::Container(strip)]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(8)
                .with_absolute_position(18, y),
        ),
    )
}

/// The records readout: a header line plus one tagged line per record, so the
/// reader (and the demo) can see the data the chart is drawn from.
fn records_readout(cells: &[CellValue], theme: &Theme) -> Scene {
    let table = CellTable::new(cells, NCOLS);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let ink = theme.resolve(ColorRole::OnSurface);

    let mut lines = vec![Scene::Text(
        TextNode::styled(
            COL_NAMES.join("  "),
            Rect::default(),
            TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(muted),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 46)),
    )];
    for r in 0..table.nrows() {
        let row: Vec<String> = (0..NCOLS)
            .map(|c| table.get(r, c).map_or_else(String::new, CellValue::display))
            .collect();
        lines.push(Scene::Text(
            TextNode::styled(
                row.join("  "),
                Rect::default(),
                TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(ink),
            )
            .with_tag(format!("records.row.{r}"))
            .with_layout(LayoutStyle::new().with_absolute_position(
                18,
                46 + ROW_PITCH + u32::try_from(r).unwrap_or(0) * ROW_PITCH,
            )),
        ));
    }
    Scene::Container(ContainerNode::new(lines))
}

/// view-fn (§6.3): pure sync `PickerState -> Scene`. The mapping is re-run
/// every frame over the live cells, so both a picker change and a cell write
/// land the same way.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: PickerState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let cells = use_cells().get();
    let y_on = state.y_on();
    let mapped = mapper(state.x_selected, y_on).map(&CellTable::new(&cells, NCOLS));

    let title = Scene::Text(
        TextNode::styled(
            "Ledger — pick the x field and the measures; a text column plots nothing",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 14)),
    );

    let chart = LineChart::new(mapped.series.clone()).build(CHART_RECT, &chart_style(&theme));

    let x_chips: Vec<Scene> = (0..X_OPTIONS)
        .map(|i| {
            field_chip(
                format!("{X_TAG}#{i}"),
                x_label(i),
                state.x_selected == i,
                i == 0 || is_measure(&cells, i - 1),
                false, // the strip is the group's single Tab stop
                state.x_rows[i].0,
                &theme,
            )
        })
        .collect();
    let y_chips: Vec<Scene> = (0..NCOLS)
        .map(|c| {
            field_chip(
                Y_TAGS[c].to_owned(),
                COL_NAMES[c],
                y_on[c],
                is_measure(&cells, c),
                true, // each toggle is its own Tab stop
                state.y_rows[c].0,
                &theme,
            )
        })
        .collect();

    let status = Scene::Text(
        TextNode::styled(
            status_line(&mapped, state.x_selected, y_on),
            Rect::default(),
            TextStyle::new()
                .with_size_px(BODY_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(18, WIN_H - 26)),
    );

    Scene::Container(
        ContainerNode::new(vec![
            chart,
            records_readout(&cells, &theme),
            chip_row("x field", 400, Some(X_TAG), true, x_chips, &theme),
            chip_row("measures", 448, Some(Y_GROUP_TAG), false, y_chips, &theme),
            title,
            status,
        ])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
        .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

// ── the cell block's RPC surface ──────────────────────────────────────────

/// Field declarations for [`RecordsExternal`], hoisted so `schema()` stays a
/// one-liner (the `hello-data-grid` convention).
const RECORDS_SCHEMA_FIELDS: &[SchemaField] = &[
    SchemaField::new("row_count", "int"),
    SchemaField::new("col_count", "int"),
    SchemaField::parametric(
        "col_name.<col>",
        "string",
        const { &[SchemaArg::index("col", "col_count")] },
    ),
    SchemaField::parametric(
        "col_kind.<col>",
        "string",
        const { &[SchemaArg::index("col", "col_count")] },
    ),
    SchemaField::parametric(
        "value.<row>.<col>",
        "json",
        const {
            &[
                SchemaArg::index("row", "row_count"),
                SchemaArg::index("col", "col_count"),
            ]
        },
    ),
];

/// The data-input surface: read and WRITE the cell block that feeds the chart
/// (§2 #2). Writes go through [`CellKind::coerce`], so a value of the wrong
/// type is rejected rather than silently coerced — the same strictness the
/// mapper applies when reading.
struct RecordsExternal {
    cells: Rc<Signal<Vec<CellValue>>>,
}

impl RecordsExternal {
    /// Parse a `"<row>.<col>"` suffix into indices inside the block.
    fn cell_index(suffix: &str) -> Option<(usize, usize)> {
        let (row, col) = suffix.split_once('.')?;
        let row: usize = row.parse().ok()?;
        let col: usize = col.parse().ok()?;
        (row < NROWS && col < NCOLS).then_some((row, col))
    }
}

impl core::fmt::Debug for RecordsExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RecordsExternal").finish_non_exhaustive()
    }
}

impl External for RecordsExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
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

impl ExternalIntrospect for RecordsExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(RECORDS_SCHEMA_FIELDS)
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "row_count" => {
                return Some(IntrospectValue::Int(
                    i64::try_from(NROWS).unwrap_or(i64::MAX),
                ));
            }
            "col_count" => {
                return Some(IntrospectValue::Int(
                    i64::try_from(NCOLS).unwrap_or(i64::MAX),
                ));
            }
            _ => {}
        }
        if let Some(col) = path.strip_prefix("col_name.") {
            let col: usize = col.parse().ok()?;
            return COL_NAMES
                .get(col)
                .map(|n| IntrospectValue::Text((*n).to_owned()));
        }
        if let Some(col) = path.strip_prefix("col_kind.") {
            let col: usize = col.parse().ok()?;
            return COL_KINDS
                .get(col)
                .map(|k| IntrospectValue::Text(k.name().to_owned()));
        }
        let (row, col) = Self::cell_index(path.strip_prefix("value.")?)?;
        let cells = self.cells.get();
        CellTable::new(&cells, NCOLS)
            .get(row, col)
            .map(CellValue::to_introspect)
    }

    /// Write one cell. The value must match the column's declared
    /// [`CellKind`]; anything else is an [`InterveneError::TypeMismatch`],
    /// never a coercion.
    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        let suffix = path
            .strip_prefix("value.")
            .ok_or(InterveneError::UnknownPath)?;
        let (row, col) = Self::cell_index(suffix).ok_or(InterveneError::OutOfRange)?;
        let next = COL_KINDS[col].coerce(value)?;
        let mut cells = self.cells.get();
        cells[row * NCOLS + col] = next;
        self.cells.set(cells);
        Ok(())
    }

    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        Err(InvokeError::UnknownPath)
    }
}

// ── binding ───────────────────────────────────────────────────────────────

/// Cached projection of both pickers. `Copy` per the `WidgetCore::State`
/// bound — the cells themselves live in the shared `Signal` the view reads.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PickerState {
    /// Per-x-option `(interaction state, selected)`.
    x_rows: [(RadioState, bool); X_OPTIONS],
    /// The selected x option (the radio group's 1-of-N invariant, resolved).
    x_selected: usize,
    /// The AT-side roving descendant of the x group.
    x_focused: Option<usize>,
    /// Per-column `(interaction state, on)`.
    y_rows: [(ToggleState, bool); NCOLS],
}

impl PickerState {
    fn idle() -> Self {
        Self {
            x_rows: [(RadioState::Idle, false); X_OPTIONS],
            x_selected: BOOT_X,
            x_focused: None,
            y_rows: [(ToggleState::Idle, false); NCOLS],
        }
    }

    fn y_on(self) -> [bool; NCOLS] {
        let mut on = [false; NCOLS];
        for (i, slot) in on.iter_mut().enumerate() {
            *slot = self.y_rows[i].1;
        }
        on
    }
}

/// Which x option a navigation `key` targets, for the shared roving-key shell.
fn resolve_x_target(intro: Option<&dyn ExternalIntrospect>, key: &str) -> Option<usize> {
    match key {
        "ArrowRight" | "ArrowDown" => Some(rc::step(intro, 1, X_OPTIONS)),
        "ArrowLeft" | "ArrowUp" => Some(rc::step(intro, -1, X_OPTIONS)),
        "Home" => Some(0),
        "End" => Some(X_OPTIONS - 1),
        _ => None,
    }
}

struct ModelChartView;

impl WidgetCore for ModelChartView {
    type State = PickerState;
    // Every change arrives through `apply_key` or the input router's per-chip
    // pointer dispatch — never the enum keybinding channel.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut group = RadioGroupExternal::new(X_OPTIONS);
        group.send(
            BOOT_X,
            pinion_core::widgets::radio::RadioEvent::PointerEnter,
        );
        group.send(BOOT_X, pinion_core::widgets::radio::RadioEvent::PointerDown);
        group.send(BOOT_X, pinion_core::widgets::radio::RadioEvent::PointerUp);
        group.send(
            BOOT_X,
            pinion_core::widgets::radio::RadioEvent::PointerLeave,
        );
        Box::new(group)
    }

    /// The y toggles plus the cell block's RPC surface. Built by hand rather
    /// than through `toggle_group::extra_toggles`, which skips index 0 on the
    /// assumption that the first toggle is the primary external (see the
    /// module docs) — here the primary is the x radio group.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let mut extras: Vec<ExtraExternal> = (0..NCOLS)
            .map(|i| {
                ExtraExternal::new(
                    Y_TAGS[i],
                    Box::new(toggle_group::boot_toggle(BOOT_Y[i])) as Box<dyn External>,
                )
            })
            .collect();
        extras.push(ExtraExternal::new(
            RECORDS_TAG,
            Box::new(RecordsExternal { cells: use_cells() }),
        ));
        extras
    }

    fn tag() -> &'static str {
        X_TAG
    }

    fn read_state(scene: &Scene) -> PickerState {
        let mut out = PickerState::idle();
        if let Some(node) = scene.find_external_with_tag(X_TAG) {
            if let Some(intro) = node.handle.introspect() {
                rc::read_rows(intro, &mut out.x_rows);
                out.x_focused = rc::focused_index(intro);
                out.x_selected = rc::selected_index(intro).unwrap_or(BOOT_X);
            }
        }
        for (i, slot) in out.y_rows.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, Y_TAGS[i]);
        }
        out
    }

    fn view(state: PickerState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-model-chart (R1446 ModelMapper: a chart over a cell model)"
    }

    /// Two keymaps, one per picker. `toggle_group::apply_key` returns `false`
    /// unless one of `Y_TAGS` owns focus and the x branch returns `false`
    /// unless `X_TAG` does, so exactly one can consume a key.
    ///
    /// The x branch addresses the group BY TAG rather than calling
    /// `rc::roving_key`, which requires the External to be the scene root —
    /// true only for a binding with no extras.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if toggle_group::apply_key(scene, focused, key, &Y_TAGS) {
            return true;
        }
        if focused != Some(X_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(X_TAG) else {
            return false;
        };
        let Some(idx) = resolve_x_target(node.handle.introspect(), key) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        rc::drive_activate(intro, idx);
        true
    }

    fn fmt_state_log(state: &PickerState) -> String {
        let y = (0..NCOLS)
            .map(|c| {
                format!(
                    "{}{}",
                    COL_NAMES[c],
                    if state.y_rows[c].1 { "+" } else { "-" }
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        format!("x={} y=[{y}]", x_label(state.x_selected))
    }
}

impl WidgetA11y for ModelChartView {
    /// The x group as a WAI-ARIA `radiogroup`, the measures as a `group` of
    /// `button[aria-pressed]`, and the status as a live region — so the
    /// "not a measure" diagnosis is heard, not only seen.
    fn access_node(state: &PickerState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(X_TAG);
        let active = rc::active_index(&state.x_rows, state.x_focused);
        let tags: Vec<String> = (0..X_OPTIONS).map(|i| format!("{X_TAG}#{i}")).collect();
        let cells: Vec<RadioCell<'_>> = (0..X_OPTIONS)
            .map(|i| RadioCell {
                tag: &tags[i],
                label: Some(x_label(i)),
                state: state.x_rows[i].0,
                selected: state.x_rows[i].1,
                focused: group_focused && i == active,
            })
            .collect();
        let mut nodes = radiogroup_radio_nodes(X_TAG, "Independent variable", &cells);

        let segments: Vec<ToggleSegment<'_>> = (0..NCOLS)
            .map(|c| ToggleSegment {
                tag: Y_TAGS[c],
                label: COL_NAMES[c],
                state: state.y_rows[c].0,
                on: state.y_rows[c].1,
            })
            .collect();
        nodes.extend(toggle_button_group_nodes(
            Y_GROUP_TAG,
            "Measures",
            &segments,
            focused,
        ));

        let cells = use_cells().get();
        let mapped = mapper(state.x_selected, state.y_on()).map(&CellTable::new(&cells, NCOLS));
        nodes.push(
            AccessNode::new(STATUS_TAG, pinion_a11y::AriaRole::Status).with_name(status_line(
                &mapped,
                state.x_selected,
                state.y_on(),
            )),
        );
        nodes
    }
}

impl WidgetView for ModelChartView {
    type Renderer = HelloModelChartRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ModelChartView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::PathCommand;

    fn state_with(x: usize, y: [bool; NCOLS]) -> PickerState {
        let mut s = PickerState::idle();
        s.x_selected = x;
        s.x_rows[x].1 = true;
        for (i, slot) in s.y_rows.iter_mut().enumerate() {
            slot.1 = y[i];
        }
        s
    }

    fn render(x: usize, y: [bool; NCOLS]) -> Scene {
        Owner::new().run(|| view(state_with(x, y), &Frame::new()))
    }

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        if let Scene::Container(c) = scene {
            return c.children.iter().find_map(|ch| find(ch, tag));
        }
        None
    }

    fn text_of(scene: &Scene, tag: &str) -> String {
        match find(scene, tag) {
            Some(Scene::Text(t)) => t.content.clone(),
            other => panic!("{tag} is a text node, got {other:?}"),
        }
    }

    /// The largest y-tick label value on the chart. `format_si` renders a
    /// kilo tick as `1k`, so a bare `parse::<f64>()` would silently read the
    /// axis as empty exactly when it has grown past 1000 — the case a
    /// "did the axis re-scale?" assertion cares most about.
    fn max_y_tick(scene: &Scene) -> f64 {
        let mut best = 0.0_f64;
        for k in 0..8 {
            let Some(Scene::Text(t)) = find(scene, &format!("chart.label.y.{k}")) else {
                continue;
            };
            let raw = t.content.trim();
            let (num, mul) = raw.strip_suffix('k').map_or((raw, 1.0), |n| (n, 1000.0));
            if let Ok(v) = num.trim().parse::<f64>() {
                best = best.max(v.abs() * mul);
            }
        }
        best
    }

    /// Vertex count of series `i`'s polyline, or 0 when it draws none.
    fn series_vertices(scene: &Scene, i: usize) -> usize {
        match find(scene, &format!("chart.series.{i}")) {
            Some(Scene::Path(p)) => p
                .commands
                .iter()
                .filter(|c| matches!(c, PathCommand::MoveTo(_) | PathCommand::LineTo(_)))
                .count(),
            _ => 0,
        }
    }

    #[test]
    fn boot_plots_the_two_measures_against_the_record_ordinal() {
        let scene = render(BOOT_X, BOOT_Y);
        assert_eq!(series_vertices(&scene, 0), NROWS);
        assert_eq!(series_vertices(&scene, 1), NROWS);
        let status = text_of(&scene, STATUS_TAG);
        assert!(status.contains("x = record #"), "{status}");
        assert!(status.contains("y = Revenue, Cost"), "{status}");
        assert!(status.contains("16 points plotted"), "{status}");
        assert!(
            !status.contains("not a measure"),
            "a clean mapping reports nothing: {status}"
        );
    }

    /// The headline claim. Qt would draw a flat line on zero here.
    #[test]
    fn a_text_measure_plots_nothing_and_the_status_names_the_reason() {
        let scene = render(BOOT_X, [true, false, false, false]);
        assert_eq!(
            series_vertices(&scene, 0),
            0,
            "the text column contributes no vertices — no zeros are invented"
        );
        let status = text_of(&scene, STATUS_TAG);
        assert!(status.contains("0 points plotted"), "{status}");
        assert!(
            status.contains("Month: 8 text cells, not a measure"),
            "the status names the column, the count and the kind: {status}"
        );
    }

    #[test]
    fn re_pointing_x_at_a_column_re_domains_the_same_series() {
        // x = Units (col 2 -> option 4) instead of the record ordinal. Same
        // number of vertices, different x extent: the first x is 30, not 0.
        let ordinal = render(0, [false, true, false, false]);
        let by_units = render(4, [false, true, false, false]);
        assert_eq!(series_vertices(&ordinal, 0), NROWS);
        assert_eq!(series_vertices(&by_units, 0), NROWS);
        assert!(text_of(&by_units, STATUS_TAG).contains("x = Units"));
        assert!(
            text_of(&by_units, STATUS_TAG).contains("8 points plotted"),
            "a numeric x keeps every record"
        );
    }

    /// x pointed at the TEXT column: every record loses its x, so nothing
    /// plots — and the report counts each bad x cell ONCE even with two
    /// series mapped (the `ModelMapper::map` de-duplication).
    #[test]
    fn a_text_x_drops_every_record_and_is_reported_once() {
        let scene = render(1, [false, true, true, false]);
        assert_eq!(series_vertices(&scene, 0), 0);
        assert_eq!(series_vertices(&scene, 1), 0);
        let status = text_of(&scene, STATUS_TAG);
        assert!(
            status.contains("Month: 8 text cells"),
            "8 bad x cells, not 16: {status}"
        );
    }

    #[test]
    fn no_measures_selected_plots_nothing_but_keeps_every_chip() {
        let scene = render(BOOT_X, [false; NCOLS]);
        assert_eq!(series_vertices(&scene, 0), 0);
        assert!(text_of(&scene, STATUS_TAG).contains("y = (none)"));
        for tag in Y_TAGS {
            assert!(find(&scene, tag).is_some(), "{tag} is the way back");
        }
    }

    /// The two rows carry the two WAI-ARIA composite models: the radio group
    /// is ONE Tab stop (the strip) with its chips as hit targets only, and the
    /// measures are N independent stops. Getting this backwards is invisible
    /// to the eye and breaks every keyboard user, so it is pinned exactly.
    #[test]
    fn the_x_group_is_one_tab_stop_and_each_measure_is_its_own() {
        let scene = render(BOOT_X, BOOT_Y);
        let mut expected = vec![X_TAG.to_owned()];
        expected.extend(Y_TAGS.iter().map(|t| (*t).to_owned()));
        assert_eq!(
            scene.collect_focusable_tags(),
            expected,
            "the x strip is one stop; each measure toggle is its own"
        );
        for i in 0..X_OPTIONS {
            let tag = format!("{X_TAG}#{i}");
            let Some(Scene::Container(c)) = find(&scene, &tag) else {
                panic!("{tag} is a container")
            };
            assert!(
                !c.layout.focusable,
                "{tag} is a click target, not a Tab stop — the group owns the stop"
            );
        }
    }

    #[test]
    fn the_readout_shows_the_records_the_chart_is_drawn_from() {
        let scene = render(BOOT_X, BOOT_Y);
        assert!(text_of(&scene, "records.row.0").starts_with("Jan"));
        assert!(text_of(&scene, "records.row.7").starts_with("Aug"));
    }

    #[test]
    fn a_written_cell_moves_the_plotted_series() {
        // The §2 #2 data-input path end to end: intervene a cell, and the very
        // next view maps the new value.
        let owner = Owner::new();
        owner.run(|| {
            let picker = state_with(0, [false, true, false, false]);
            let before = view(picker, &Frame::new());
            assert!(
                max_y_tick(&before) < 400.0,
                "the seeded Revenue column tops out in the hundreds, got {}",
                max_y_tick(&before)
            );

            let mut ext = RecordsExternal { cells: use_cells() };
            ext.intervene("value.0.1", IntrospectValue::Float(999.0))
                .expect("a Float into a Float column is accepted");

            let after = view(picker, &Frame::new());
            assert!(
                text_of(&after, "records.row.0").contains("999"),
                "the readout shows the written value"
            );
            assert!(
                max_y_tick(&after) >= 900.0,
                "the axis re-scaled to the written value, got {}",
                max_y_tick(&after)
            );
        });
    }

    #[test]
    fn a_write_of_the_wrong_type_is_rejected_not_coerced() {
        let owner = Owner::new();
        owner.run(|| {
            let mut ext = RecordsExternal { cells: use_cells() };
            assert!(
                ext.intervene("value.0.1", IntrospectValue::Text("999".to_owned()))
                    .is_err(),
                "a Text payload into a Float column is a type error"
            );
            assert!(
                ext.intervene("value.99.0", IntrospectValue::Text("x".to_owned()))
                    .is_err(),
                "an out-of-range row is rejected"
            );
            assert!(
                ext.intervene("nope", IntrospectValue::Int(1)).is_err(),
                "an unknown path is rejected"
            );
        });
    }

    #[test]
    fn the_records_surface_reads_back_shape_and_cells() {
        let owner = Owner::new();
        owner.run(|| {
            let ext = RecordsExternal { cells: use_cells() };
            assert_eq!(ext.query("row_count"), Some(IntrospectValue::Int(8)));
            assert_eq!(ext.query("col_count"), Some(IntrospectValue::Int(4)));
            assert_eq!(
                ext.query("col_name.1"),
                Some(IntrospectValue::Text("Revenue".to_owned()))
            );
            assert_eq!(
                ext.query("col_kind.0"),
                Some(IntrospectValue::Text("text".to_owned()))
            );
            assert!(ext.query("value.0.0").is_some());
            assert!(ext.query("value.0.9").is_none(), "out of range reads None");
        });
    }

    /// A column's chip is muted when the column is not a measure — asked of
    /// the DATA, so a write that breaks a column mutes it too.
    #[test]
    fn is_measure_follows_the_cells_not_the_declared_kind() {
        let cells = boot_cells();
        assert!(!is_measure(&cells, 0), "the Month column is text");
        assert!(is_measure(&cells, 1), "the Revenue column is numeric");
        assert!(is_measure(&cells, 3), "an Int column is a measure");
    }

    #[test]
    fn a11y_exposes_both_pickers_and_the_diagnosis() {
        let owner = Owner::new();
        let nodes = owner
            .run(|| ModelChartView::access_node(&state_with(0, [true, false, false, false]), None));
        let radios = nodes
            .iter()
            .filter(|n| n.role == pinion_a11y::AriaRole::RadioButton)
            .count();
        assert_eq!(radios, X_OPTIONS, "one radio per x option");
        let buttons = nodes
            .iter()
            .filter(|n| n.role == pinion_a11y::AriaRole::Button)
            .count();
        assert_eq!(buttons, NCOLS, "one aria-pressed button per measure");
        let status = nodes
            .iter()
            .find(|n| n.tag == STATUS_TAG)
            .expect("the diagnosis is in the a11y tree");
        assert!(
            status
                .name
                .as_deref()
                .is_some_and(|n| n.contains("not a measure")),
            "a screen reader hears WHY nothing plotted: {:?}",
            status.name
        );
    }

    #[test]
    fn boot_x_and_boot_y_are_the_seeded_picker() {
        assert_eq!(x_label(BOOT_X), "record #");
        assert_eq!(BOOT_Y, [false, true, true, false]);
    }

    #[test]
    fn view_carries_the_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ModelChartView>(
            PickerState::idle(),
            &Frame::new(),
        );
    }
}
