//! R704 §5.38 §5.50 — backend-agnostic date-picker paint composition.
//!
//! Composes the inline date-picker visual: a header row (previous /
//! next-month navigation buttons flanking a centred "Month YYYY" label),
//! a weekday-header row (Su Mo Tu We Th Fr Sa), and a 6×7 day grid. The
//! grid carries leading blanks before the first weekday, the day numbers
//! `1..=days_in_month`, and trailing blanks to fill the final row. The
//! selected day is filled with the accent role; the focused day gets the
//! state-layer + focus-ring treatment (R694 posture).
//!
//! ## Naming
//!
//! Mirrors [`crate::disclosure`]: a [`DatePickerStyle`] carrier struct
//! with [`DatePickerStyle::m3`] defaults and a [`view_datepicker`] fn
//! that produces a [`Scene`] fragment the binding's outer view-fn wraps
//! in its root container.
//!
//! ## Structure (tags)
//!
//! - The header nav buttons are tagged `"<tag>_prev"` / `"<tag>_next"`
//!   (real [`ButtonExternal`](pinion_core::widgets::button::ButtonExternal) composite siblings the binding registers).
//! - Each weekday header is a presentational `TextNode` tagged
//!   `"<tag>_wh<col>"` (column 0..=6) so the binding's `access_node`
//!   walker can attach the `columnheader` accessible name.
//! - Each real day cell is a hit-test target tagged `"<tag>#<day>"`
//!   (the R51.41 composite paint convention → the
//!   `InputRouter` `'#'`-split routes a
//!   click on day `d` to the picker's `"<d>:<EventName>"` send). Blank
//!   cells are untagged, non-interactive spacers.

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::datepicker::{CivilDate, days_in_month, weekday_of_first};
use pinion_core::widgets::radio::RadioState;

/// R704 §5.50 — full weekday names (Sunday-first), used by the binding's
/// `access_node` walker for each `columnheader`'s accessible name.
pub const WEEKDAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// R704 §5.50 — short weekday labels painted in the weekday-header row.
pub const WEEKDAY_SHORT: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/// R704 §5.50 — full month names, used for the header label and the
/// binding's per-cell `gridcell` accessible names.
pub const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// R704 §5.50 — Material-3 date-picker paint dimensions. Mirrors the
/// [`crate::disclosure::DisclosureStyle`] carrier pattern so binding
/// callers see a uniform `Style` surface across the widget catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DatePickerStyle {
    /// Single day-cell side length in logical pixels (cells are square).
    /// Default 40 (M3 calendar cell).
    pub cell_size: u32,
    /// Gap between grid cells in logical pixels (default 2).
    pub cell_gap: u32,
    /// Day-cell corner radius in logical pixels (default 20 = a circle
    /// at the default 40-px cell, the M3 selected-day shape).
    pub cell_radius: u32,
    /// Header / weekday / day-cell label font size in logical pixels
    /// (default 14 ≈ M3 `body-medium`).
    pub label_size_px: u32,
    /// Header "Month YYYY" label font size in logical pixels
    /// (default 16 ≈ M3 `title-medium`).
    pub header_size_px: u32,
    /// Nav-button side length in logical pixels (default 32).
    pub nav_size: u32,
    /// Outer block padding on all four sides in logical pixels
    /// (default 12).
    pub block_pad: u32,
    /// Outer block corner radius in logical pixels (default 12 = M3
    /// large shape token).
    pub corner_radius: u32,
    /// (R1020 §5.39) Keyboard focus stop. When `true`, [`view_datepicker`]
    /// marks both the outer block Container (the picker root, `PRIMARY`
    /// tag) and each nav-button Container (`prev` / `next`)
    /// `.with_focusable(true)` so the scene-derived §5.39 enumeration
    /// collects their tags as Tab stops.
    ///
    /// Default `true` (R1030 fail-safe, web native-element model), mirroring
    /// [`ButtonStyle::focusable`](crate::button::ButtonStyle::focusable): a
    /// stand-alone picker is a Tab stop by default; a picker embedded as a
    /// modal-popup member opts out with `.with_focusable(false)` so it stays
    /// out of the base Tab order until the popup opens.
    pub focusable: bool,
}

impl DatePickerStyle {
    /// R704 §5.50 — Material-3 date-picker defaults.
    #[must_use]
    pub const fn m3() -> Self {
        Self {
            cell_size: 40,
            cell_gap: 2,
            cell_radius: 20,
            label_size_px: 14,
            header_size_px: 16,
            nav_size: 32,
            block_pad: 12,
            corner_radius: 12,
            focusable: true,
        }
    }

    /// (R1020 §5.39) Mark this date-picker a keyboard focus stop (default
    /// `false`). The picker root + both nav buttons become Tab stops when
    /// opted in. See [`Self::focusable`].
    #[must_use]
    pub const fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }
}

impl Default for DatePickerStyle {
    fn default() -> Self {
        Self::m3()
    }
}

/// R704 §5.50 — the calendar month a [`view_datepicker`] call renders.
///
/// Groups the displayed `(year, month)` into one unit so the paint
/// signature stays under the readable argument budget; `month` is
/// `1..=12`. Paired with
/// [`DatePickerExternal`](pinion_core::widgets::datepicker::DatePickerExternal)'s
/// `year` / `month` introspect slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayedMonth {
    /// Displayed year (proleptic Gregorian).
    pub year: i32,
    /// Displayed month, `1..=12`.
    pub month: u8,
}

/// R704 §5.50 — day-cell fill for `state` + `selected` via the M3
/// state-layer overlay matrix.
///
/// A selected day fills with [`ColorRole::Accent`]; an unselected day is
/// transparent at rest. Hover (0.08) / pressed (0.12) overlays lerp
/// toward [`ColorRole::OnSurface`]; disabled fades toward
/// [`ColorRole::Surface`] (0.38). Identical weights to
/// [`crate::disclosure::disclosure_header_for`] so the catalog's
/// interactive surfaces share one state-layer language.
#[must_use]
pub fn day_cell_fill(theme: &Theme, state: RadioState, selected: bool) -> Color {
    let base = if selected {
        theme.resolve(ColorRole::Accent)
    } else {
        // Transparent so an unselected resting cell shows the block fill.
        Color::rgba(0, 0, 0, 0)
    };
    crate::state_layer::state_layer(base, state, theme)
}

/// One blank (non-interactive) grid cell — a fixed-size transparent box.
fn blank_cell(style: &DatePickerStyle) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_layout(LayoutStyle::new().with_size(Size::px(style.cell_size, style.cell_size))),
    )
}

/// One real day cell: a centred day-number label inside a tagged box.
///
/// The keyboard-focus ring is **not** drawn here: the shell's R694
/// `paint_focus_ring` substrate strokes the ring around whichever tag the
/// binding reports focused (the active-descendant day cell, via
/// `access_focus_target`), exactly as it does for every other widget in
/// the catalog — `RadioGroup` likewise draws no focus border of its own.
/// Drawing one here too would double the ring (a circular cell border
/// clashing with the shell's rounded-rect stroke).
fn day_cell(
    tag: &str,
    day: u8,
    state: RadioState,
    selected: bool,
    theme: &Theme,
    style: &DatePickerStyle,
) -> Scene {
    let fill = day_cell_fill(theme, state, selected);
    let fg = if selected {
        theme.resolve(ColorRole::OnAccent)
    } else if matches!(state, RadioState::Disabled) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let box_style = BoxStyle::filled(fill).with_corner_radius(style.cell_radius);
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            day.to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.label_size_px)
                .with_fg(fg),
        ))])
        .with_tag(tag.to_string())
        .with_style(box_style)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(style.cell_size, style.cell_size)),
        ),
    )
}

/// One nav button (prev / next) — a centred glyph inside a tagged box.
fn nav_button(tag: &str, glyph: &str, theme: &Theme, style: &DatePickerStyle) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(
            TextNode::styled(
                glyph,
                Rect::default(),
                TextStyle::new()
                    .with_size_px(style.header_size_px)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            )
            .with_role(TextRole::Presentational),
        )])
        .with_tag(tag.to_string())
        .with_style(
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
                .with_corner_radius(style.nav_size / 2),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(style.nav_size, style.nav_size))
                .with_focusable(style.focusable),
        ),
    )
}

/// R704 §5.50 — collapsed-month prev glyph (`U+25C0` BLACK LEFT-POINTING
/// TRIANGLE).
const GLYPH_PREV: &str = "\u{25C0}";
/// R704 §5.50 — next-month glyph (`U+25B6` BLACK RIGHT-POINTING
/// TRIANGLE).
const GLYPH_NEXT: &str = "\u{25B6}";

/// The header row: `[prev button | "Month YYYY" | next button]`.
fn header_row(tag: &str, month: DisplayedMonth, theme: &Theme, style: &DatePickerStyle) -> Scene {
    let month_idx = usize::from(month.month.clamp(1, 12)) - 1;
    let header_label = format!("{} {}", MONTH_NAMES[month_idx], month.year);
    Scene::Container(
        ContainerNode::new(vec![
            nav_button(&format!("{tag}#prev"), GLYPH_PREV, theme, style),
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    header_label,
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(style.header_size_px)
                        .with_fg(theme.resolve(ColorRole::OnSurface)),
                ))])
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::width_px(style.cell_size * 5)),
                ),
            ),
            nav_button(&format!("{tag}#next"), GLYPH_NEXT, theme, style),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_gap(style.cell_gap),
        ),
    )
}

/// The weekday-header row: 7 presentational `Su..Sa` cells tagged
/// `"<tag>_wh<col>"` for the binding's `columnheader` a11y attachment.
fn weekday_header_row(tag: &str, theme: &Theme, style: &DatePickerStyle) -> Scene {
    Scene::Container(
        ContainerNode::new(
            (0..7)
                .map(|col| {
                    Scene::Container(
                        ContainerNode::new(vec![Scene::Text(
                            TextNode::styled(
                                WEEKDAY_SHORT[col],
                                Rect::default(),
                                TextStyle::new()
                                    .with_size_px(style.label_size_px)
                                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
                            )
                            .with_role(TextRole::Presentational),
                        )])
                        .with_tag(format!("{tag}_wh{col}"))
                        .with_layout(
                            LayoutStyle::new()
                                .flex(FlexDirection::Row)
                                .with_justify(JustifyContent::Center)
                                .with_align_items(AlignItems::Center)
                                .with_size(Size::px(style.cell_size, style.cell_size)),
                        ),
                    )
                })
                .collect(),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_gap(style.cell_gap),
        ),
    )
}

/// The 6×7 day grid: leading blanks, day cells `"<tag>#<day>"`, trailing
/// blanks. `selected_day` is the in-view selected day (already filtered to
/// the displayed month). The keyboard-focus ring is the shell's job (R694
/// `paint_focus_ring` over the active-descendant tag), so no focus state
/// is threaded here.
fn day_grid(
    tag: &str,
    month: DisplayedMonth,
    selected_day: Option<u8>,
    day_states: &[RadioState],
    theme: &Theme,
    style: &DatePickerStyle,
) -> Scene {
    let days = days_in_month(month.year, month.month);
    let lead = usize::from(weekday_of_first(month.year, month.month));
    let mut grid_rows: Vec<Scene> = Vec::with_capacity(6);
    let mut cursor: usize = 0; // 0-based slot index across the 42-cell grid
    for _row in 0..6 {
        let mut cells: Vec<Scene> = Vec::with_capacity(7);
        for _col in 0..7 {
            if cursor < lead || cursor >= lead + usize::from(days) {
                cells.push(blank_cell(style));
            } else {
                let day = u8::try_from(cursor - lead + 1).unwrap_or(1);
                let cell_tag = format!("{tag}#{day}");
                let state = day_states
                    .get(usize::from(day) - 1)
                    .copied()
                    .unwrap_or(RadioState::Idle);
                let is_selected = selected_day == Some(day);
                cells.push(day_cell(&cell_tag, day, state, is_selected, theme, style));
            }
            cursor += 1;
        }
        grid_rows.push(Scene::Container(
            ContainerNode::new(cells).with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_gap(style.cell_gap),
            ),
        ));
    }
    Scene::Container(
        ContainerNode::new(grid_rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(style.cell_gap),
        ),
    )
}

/// R704 §5.50 — compose the M3 date-picker paint scene fragment.
///
/// # Arguments
///
/// - `tag` — composite paint-root tag; the header nav buttons are
///   `"<tag>#prev"` / `"<tag>#next"`, weekday headers `"<tag>_wh<col>"`,
///   and day cells `"<tag>#<day>"`.
/// - `month` — the [`DisplayedMonth`] rendered; paired with
///   [`DatePickerExternal`](pinion_core::widgets::datepicker::DatePickerExternal)'s
///   `year` / `month` introspect slots.
/// - `selected` — the selected [`CivilDate`], or `None`. A selected day
///   in the displayed month is filled with the accent role.
/// - `day_states` — per-day [`RadioState`] interaction projections,
///   indexed by `day − 1` (the binding reads them from the picker's
///   per-day `state.<d>` introspect slots). A day outside the slice
///   bounds defaults to [`RadioState::Idle`].
/// - `theme` — current [`Theme`] palette; the binding resolves it via
///   [`pinion_core::theme::use_theme`] and forwards.
/// - `style` — [`DatePickerStyle`] dimension carrier.
///
/// # Returns
///
/// A [`Scene::Container`] (Column) holding the header row, the weekday-
/// header row, and the 6×7 day grid, carrying `tag` on the outer block
/// so the composite root is paint-addressable.
#[must_use]
pub fn view_datepicker(
    tag: &str,
    month: DisplayedMonth,
    selected: Option<CivilDate>,
    day_states: &[RadioState],
    theme: &Theme,
    style: &DatePickerStyle,
) -> Scene {
    let header = header_row(tag, month, theme, style);
    let weekday_row = weekday_header_row(tag, theme, style);
    let selected_day = selected
        .filter(|d| d.year == month.year && d.month == month.month)
        .map(|d| d.day);
    let grid = day_grid(tag, month, selected_day, day_states, theme, style);

    Scene::Container(
        ContainerNode::new(vec![header, weekday_row, grid])
            .with_tag(tag.to_string())
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(style.corner_radius),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_gap(style.cell_gap)
                    .with_padding(Rect::new(
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                    ))
                    .with_focusable(style.focusable),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn light() -> Theme {
        Theme::light()
    }

    /// 31 idle day states — enough for any displayed month.
    fn all_idle() -> Vec<RadioState> {
        vec![RadioState::Idle; 31]
    }

    #[test]
    fn r704_style_m3_constants() {
        let s = DatePickerStyle::m3();
        assert_eq!(s.cell_size, 40);
        assert_eq!(s.cell_gap, 2);
        assert_eq!(s.cell_radius, 20);
        assert_eq!(s.nav_size, 32);
        assert_eq!(s.corner_radius, 12);
    }

    #[test]
    fn r704_scene_carries_root_nav_and_weekday_tags() {
        let scene = Owner::new().run(|| {
            view_datepicker(
                "datepicker",
                DisplayedMonth {
                    year: 2026,
                    month: 5,
                },
                None,
                &all_idle(),
                &light(),
                &DatePickerStyle::m3(),
            )
        });
        assert!(
            scene.contains_tag("datepicker"),
            "composite root tag present"
        );
        assert!(
            scene.contains_tag("datepicker#prev"),
            "prev nav tag present"
        );
        assert!(
            scene.contains_tag("datepicker#next"),
            "next nav tag present"
        );
        for col in 0..7 {
            assert!(
                scene.contains_tag(&format!("datepicker_wh{col}")),
                "weekday header col {col} present",
            );
        }
    }

    #[test]
    fn r704_scene_carries_every_day_cell_tag() {
        // 2026-05 has 31 days.
        let scene = Owner::new().run(|| {
            view_datepicker(
                "datepicker",
                DisplayedMonth {
                    year: 2026,
                    month: 5,
                },
                None,
                &all_idle(),
                &light(),
                &DatePickerStyle::m3(),
            )
        });
        for day in 1..=31u8 {
            assert!(
                scene.contains_tag(&format!("datepicker#{day}")),
                "day cell {day} present",
            );
        }
        // No day 32 (May has only 31).
        assert!(!scene.contains_tag("datepicker#32"));
    }

    #[test]
    fn r704_selected_cell_fill_differs_from_unselected() {
        let theme = light();
        let unselected = day_cell_fill(&theme, RadioState::Idle, false);
        let selected = day_cell_fill(&theme, RadioState::Idle, true);
        assert_ne!(selected, unselected, "selected cell fills differently");
        assert_eq!(selected, theme.resolve(ColorRole::Accent));
        // Unselected resting cell is transparent (shows the block fill).
        assert_eq!(unselected, Color::rgba(0, 0, 0, 0));
    }

    #[test]
    fn r704_hover_overlay_lerps_toward_on_surface() {
        let theme = light();
        let expected = theme
            .resolve(ColorRole::Accent)
            .lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(day_cell_fill(&theme, RadioState::Hover, true), expected);
    }
}
