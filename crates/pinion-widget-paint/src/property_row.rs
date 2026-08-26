//! R1849 §5.16 §5.38 §5.50 — **one row of a property grid**: an indented name
//! beside a value cell whose control is chosen by the value's own type.
//!
//! # What was missing, and what was not
//!
//! [[debt-the-property-grid-is-an-example-not-a-crate]] opened at R1646 on a
//! name grep (`property_grid|PropertyGrid` matched only two examples) and
//! R1848.1 measured what that grep could not see: the model half of a property
//! grid has been substrate for a long time. The flatten and the expansion
//! machinery are [`tree_nav`], the values are [`CellValue`], the undo journal is
//! [`UndoStack`], the keymap is [`edit_field_keymap`], and every control a value
//! cell can hold is already a painter in this crate —
//! [`checkbox`](crate::checkbox), [`slider`](crate::slider),
//! [`text_field`](crate::text_field).
//!
//! What no crate held was the **row that assembles them**: the two-column box
//! that insets a label by its depth, dispatches its value cell on the value's
//! type, and hands its trailing edge to whatever marks the screen wants there.
//! [`config_form`](crate::config_form) is not that row — it is a *flat*
//! settings form, deliberately, and a property grid's rows stand at a depth.
//!
//! # Why the control is derived rather than chosen
//!
//! [`ValueControl::resolve`] is the whole type-to-editor decision in one place,
//! and the painter has no second opinion: [`view_property_row`] matches on the
//! resolved control, never on the [`CellValue`] again. Two matches on the same
//! datum is how a value cell comes to paint a check box while its keyboard
//! opens a text field — [`CellKind::editor_form`] exists for that reason at the
//! kind level, and this is the same discipline one layer up, where the range
//! refinement lives (a bounded `Float` gauges, an unbounded one reads).
//!
//! # Why the geometry is published, and how far it is trusted
//!
//! [[debt-paint-and-gesture-read-two-facts]] is this repository's most-repeated
//! defect class: what a reader sees and what a press reaches derive from two
//! facts that drift. [`layout_property_row`] says where the row's parts land
//! and [`view_property_row`] paints from the same style, so a consumer's hit
//! test and a11y read that answer instead of re-spelling the arithmetic.
//!
//! ★ **And the prediction is checked rather than asserted.**
//! [`tests::r1849_the_published_geometry_is_the_layout_the_painter_produces`]
//! runs the painted row through the real layout pass and compares every
//! published rectangle with the one the solver put there. A geometry nobody
//! compares against the paint is precisely the second fact this class is about.
//!
//! ⚠ **The limit, stated rather than implied.** Only the parts whose boxes are
//! arithmetic are published: the row, the two cells, the control's own box, and
//! the gauge fill. A selector's chevron and a swatch's hex read-out are placed
//! by the flex solver from *text metrics*, so a rectangle for them here would
//! be a guess wearing a number's clothes. A consumer that needs those asks the
//! laid-out scene, which is where that fact actually lives.
//!
//! [`tree_nav`]: pinion_core::widgets::tree_nav
//! [`UndoStack`]: pinion_core::undo::UndoStack
//! [`edit_field_keymap`]: pinion_core::edit_field_keymap
//! [`CellKind::editor_form`]: pinion_core::cell_value::CellKind::editor_form

use pinion_core::cell_value::CellValue;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    SizeValue, TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::checkbox::CheckboxState;
use pinion_core::widgets::slider::SliderState;
use pinion_core::widgets::text_field::TextFieldState;

use crate::checkbox::{CheckboxStyle, view_checkbox_box};
use crate::slider::{slider_accent_for, slider_track_inactive};
use crate::state_layer::focus_fill;
use crate::text_field::{TextFieldStyle, view_field};

/// U+25BE BLACK DOWN-POINTING SMALL TRIANGLE — the closed selector's affordance.
///
/// A `const` rather than an inline literal for the reason
/// [[non-ascii-literal-named-const-escape]] records: a bare non-ASCII glyph in
/// an expression is unreadable in a diff and indistinguishable from its
/// neighbours in the same codepoint block.
const SELECTOR_CHEVRON: &str = "\u{25BE}";

/// The measurements a property row is laid out with.
///
/// Fields rather than constants where consumers genuinely differ: the two
/// column widths belong to the panel, and the row height is the density the
/// screen chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyRowStyle {
    /// Width of the name column, in logical pixels.
    pub name_col_w: u32,
    /// Width of the value column, in logical pixels.
    pub value_col_w: u32,
    /// Row height, in logical pixels.
    pub row_h: u32,
    /// How far one level of depth insets the name.
    pub indent_step: u32,
    /// Horizontal padding inside each cell.
    pub cell_pad: u32,
    /// Cell text size, in logical pixels.
    pub text_px: u32,
    /// Side of the boolean cell's check box.
    pub checkbox_size: u32,
    /// Height of a ranged scalar's gauge strip.
    pub gauge_track_h: u32,
    /// Gap between a colour chip and its hex read-out.
    pub swatch_gap: u32,
}

impl Default for PropertyRowStyle {
    /// The density `hello-property-grid` was authored at — a 150/250
    /// two-column inspector at 38 px rows.
    fn default() -> Self {
        Self {
            name_col_w: 150,
            value_col_w: 250,
            row_h: 38,
            indent_step: 16,
            cell_pad: 10,
            text_px: 15,
            checkbox_size: 20,
            gauge_track_h: 6,
            swatch_gap: 8,
        }
    }
}

impl PropertyRowStyle {
    /// The whole row's width — both columns.
    #[must_use]
    pub fn row_w(self) -> u32 {
        self.name_col_w + self.value_col_w
    }

    /// The width a value cell's control actually gets, inside the cell padding.
    #[must_use]
    pub fn control_w(self) -> u32 {
        self.value_col_w.saturating_sub(2 * self.cell_pad)
    }
}

/// The control a property row's value cell gets.
///
/// ★ **Derived, never chosen** — [`Self::resolve`] is the only constructor a
/// consumer needs, so *what editor does this value open* is answered once and
/// read by the paint, the hit test and the introspection alike.
#[derive(Debug, Clone, PartialEq, pinion_derive::VariantCensus)]
pub enum ValueControl {
    /// The shared inline text editor, open on this row now.
    ///
    /// Carries the field's own state rather than a `bool`, because *being
    /// edited* and *the buffer being edited* are the same fact, and a painter
    /// handed them separately can be told one without the other.
    Field {
        /// The field's interaction posture.
        state: TextFieldState,
        /// The caret's byte offset.
        caret: u32,
    },
    /// A check box, for a boolean.
    Toggle(bool),
    /// A bounded read-out with a filled strip — a scalar that declares a range.
    Gauge {
        /// The value.
        value: f64,
        /// The low bound.
        lo: f64,
        /// The high bound.
        hi: f64,
    },
    /// A closed selector showing the current option — the choice cell's
    /// collapsed posture, which a press opens into a roster.
    Selector {
        /// The chosen option's index.
        selected: usize,
        /// Every option, in wire order.
        options: Vec<String>,
    },
    /// A colour chip beside its hex read-out.
    Swatch(Color),
    /// The value as text, for a scalar with nothing more specific to say.
    Label(String),
}

impl ValueControl {
    /// Every wire token, in declaration order.
    ///
    /// Its length is asserted against the definition by
    /// `#[derive(VariantCensus)]`'s [`ARMS`](Self::ARMS), so a seventh control
    /// added without a token fails to compile here rather than reaching the
    /// wire as a missing word.
    pub const NAMES: [&'static str; Self::ARMS] =
        ["field", "toggle", "gauge", "selector", "swatch", "label"];

    /// Which control this value opens.
    ///
    /// `range` is the bounds a *ranged* scalar declares — the refinement no
    /// [`CellKind`](pinion_core::cell_value::CellKind) can carry, because
    /// whether a float is bounded is a property of the slot and not of the
    /// type. `editing` is the shared inline editor's state when it is open on
    /// this row.
    ///
    /// ★ Editing wins over every shape **and is itself gated**: a row whose
    /// kind is not text-editable keeps its own control even when a caller hands
    /// in a field state. Ungated, a stale `editing` would open a text field
    /// over a check box — the divergence
    /// [`is_text_editable`](pinion_core::cell_value::CellKind::is_text_editable)
    /// was lifted at R1555 to prevent, kept here at the row.
    #[must_use]
    pub fn resolve(
        value: &CellValue,
        range: Option<(f64, f64)>,
        editing: Option<(TextFieldState, u32)>,
    ) -> Self {
        if let Some((state, caret)) = editing
            && value.kind().is_text_editable()
        {
            return Self::Field { state, caret };
        }
        match value {
            CellValue::Bool(b) => Self::Toggle(*b),
            CellValue::Choice { selected, options } => Self::Selector {
                selected: *selected,
                options: options.clone(),
            },
            CellValue::Color(c) => Self::Swatch(*c),
            CellValue::Float(f) => match range {
                Some((lo, hi)) => Self::Gauge { value: *f, lo, hi },
                None => Self::Label(value.display()),
            },
            CellValue::Int(_) | CellValue::Text(_) => Self::Label(value.display()),
        }
    }

    /// The wire token for this control — what `scene/query` calls it.
    ///
    /// A spelling of its own rather than the variant ident, so renaming the
    /// Rust arm cannot silently rename a word a driver already asks for.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Field { .. } => "field",
            Self::Toggle(_) => "toggle",
            Self::Gauge { .. } => "gauge",
            Self::Selector { .. } => "selector",
            Self::Swatch(_) => "swatch",
            Self::Label(_) => "label",
        }
    }

    /// Where in `[0, 1]` a gauge stands, or `None` for every other control.
    ///
    /// Published rather than left to the painter's caller, because a gauge's
    /// *fill width* and its *introspected fraction* are the same fact — R964
    /// tagged the fill precisely so a reader could ask for it, and two
    /// arithmetics for one number is the drift this module is against.
    ///
    /// A degenerate range (`hi <= lo`) reads `0.0` rather than refusing: the
    /// row still has to paint, and an empty strip is the truthful picture of a
    /// range with no room in it.
    #[must_use]
    pub fn fraction(&self) -> Option<f64> {
        match *self {
            Self::Gauge { value, lo, hi } if hi > lo => {
                Some(((value - lo) / (hi - lo)).clamp(0.0, 1.0))
            }
            Self::Gauge { .. } => Some(0.0),
            _ => None,
        }
    }
}

/// One row of a property grid, as the caller describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyRow<'a> {
    /// The row's node id — the tag suffix under the grid's prefix, and the name
    /// the flatten, the cursor and the a11y already use.
    pub id: &'a str,
    /// The in-context name: a struct field's short "X", a top-level property's
    /// full name.
    pub label: &'a str,
    /// How deep under its branch the row stands, in levels.
    pub depth: u32,
    /// The value cell's control.
    pub control: ValueControl,
    /// Whether the roving cursor is on this row.
    pub focused: bool,
    /// The tag suffix the control's own affordance carries, when the screen
    /// wants one — R964's `gauge<slot>`, for instance.
    ///
    /// The caller's rather than derived from [`Self::id`], because the suffix
    /// is a **wire name** a reader may already query, and a painter that
    /// renamed it on adoption would break that reader silently.
    pub part: Option<&'a str>,
}

/// The two External tags a property row paints under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyRowTags {
    /// The grid coordinator — the row is tagged `{grid}#{id}`.
    pub grid: &'static str,
    /// The one shared inline editor, for the row being edited.
    pub field: &'static str,
}

/// Where a property row's parts landed.
///
/// See the module header for which parts are here and which deliberately are
/// not.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PropertyRowGeometry {
    /// The whole row — what a press on the row hits.
    pub row: Rect,
    /// The name cell, padding included.
    pub name: Rect,
    /// The value cell, padding included.
    pub value: Rect,
    /// The control's own box, inside the value cell's padding.
    pub control: Rect,
    /// The affordances **inside the control** that have an arithmetic box, as
    /// the tag suffix the painter gives them.
    pub parts: Vec<(String, Rect)>,
}

impl PropertyRowGeometry {
    /// The same row seen from somewhere else: every rectangle moved by
    /// `(dx, dy)`.
    ///
    /// What a scrolling pane needs, and here rather than in each consumer for
    /// the reason
    /// [`FormGeometry::translated`](crate::config_form::FormGeometry::translated)
    /// gives: written twice, the two frames drift, the screen still looks right
    /// and the press lands on the row above.
    ///
    /// A rectangle the shift would move to a negative coordinate collapses to
    /// [`Rect::default`] rather than being clamped to the edge — clamping would
    /// report it at a position it is not at, and a zero-area box answers no hit
    /// test.
    #[must_use]
    pub fn translated(&self, dx: i32, dy: i32) -> Self {
        let moved = |r: Rect| -> Option<Rect> {
            let x = i64::from(r.x) + i64::from(dx);
            let y = i64::from(r.y) + i64::from(dy);
            if x < 0 || y < 0 {
                return None;
            }
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "both are non-negative on this branch and bounded by u32 + i32"
            )]
            Some(Rect::new(
                x.min(i64::from(u32::MAX)) as u32,
                y.min(i64::from(u32::MAX)) as u32,
                r.w,
                r.h,
            ))
        };
        Self {
            row: moved(self.row).unwrap_or_default(),
            name: moved(self.name).unwrap_or_default(),
            value: moved(self.value).unwrap_or_default(),
            control: moved(self.control).unwrap_or_default(),
            parts: self
                .parts
                .iter()
                .filter_map(|(s, r)| Some((s.clone(), moved(*r)?)))
                .collect(),
        }
    }

    /// The control affordance at that point, if any.
    #[must_use]
    pub fn part_at(&self, x: u32, y: u32) -> Option<&str> {
        self.parts
            .iter()
            .find(|(_, r)| within(*r, x, y))
            .map(|(s, _)| s.as_str())
    }

    /// Whether that point is on the row at all.
    #[must_use]
    pub fn hit(&self, x: u32, y: u32) -> bool {
        within(self.row, x, y)
    }
}

/// Whether a point is inside a rectangle, half-open at the trailing edges.
fn within(r: Rect, x: u32, y: u32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// Where a property row's parts will land when [`view_property_row`] paints it
/// at `origin`.
#[must_use]
pub fn layout_property_row(
    row: &PropertyRow<'_>,
    style: &PropertyRowStyle,
    origin: (u32, u32),
) -> PropertyRowGeometry {
    let (x, y) = origin;
    let name = Rect::new(x, y, style.name_col_w, style.row_h);
    let value = Rect::new(x + style.name_col_w, y, style.value_col_w, style.row_h);
    let control_w = style.control_w();
    let control_x = value.x + style.cell_pad;
    let control = match &row.control {
        // The inline editor takes the cell's content box, inset top and bottom
        // so its own frame does not sit on the row's edge.
        //
        // ★ R1849 — **the width is the room it has**, and that is a repair the
        // geometry check made rather than a transcription. The screen this was
        // lifted from asked for `value_col_w - cell_pad`, which is ten pixels
        // wider than the padded cell it is placed in; the flex solver clamped
        // it and nothing looked wrong, so the declaration had been a fiction
        // that agreed with the paint by accident. Predicting it is what made
        // the two disagree out loud.
        ValueControl::Field { .. } => {
            let h = style.row_h.saturating_sub(6);
            Rect::new(control_x, y + centre_offset(style.row_h, h), control_w, h)
        }
        // The check box is its own size, centred in the row and leading-aligned
        // in the cell — the one control that does not take the cell.
        ValueControl::Toggle(_) => Rect::new(
            control_x,
            y + centre_offset(style.row_h, style.checkbox_size),
            style.checkbox_size,
            style.checkbox_size,
        ),
        // Everything else fills the cell's content box.
        //
        // ★ The swatch belongs HERE and not with the check box, which the
        // geometry check is what established: a colour cell is a chip *and* its
        // hex read-out, so the control is the pair. The chip alone is an
        // affordance inside it, and this module does not claim a rectangle for
        // an affordance the caller has given no name to.
        ValueControl::Gauge { .. }
        | ValueControl::Selector { .. }
        | ValueControl::Swatch(_)
        | ValueControl::Label(_) => Rect::new(control_x, y, control_w, style.row_h),
    };
    let mut parts = Vec::new();
    if let (Some(part), Some(frac)) = (row.part, row.control.fraction()) {
        parts.push((
            part.to_owned(),
            Rect::new(
                control_x,
                y + style.row_h.saturating_sub(style.gauge_track_h),
                gauge_fill_w(frac, control_w),
                style.gauge_track_h,
            ),
        ));
    }
    PropertyRowGeometry {
        row: Rect::new(x, y, style.row_w(), style.row_h),
        name,
        value,
        control,
        parts,
    }
}

/// How wide a gauge's fill strip is at that fraction of that track.
///
/// One function rather than two call sites, because
/// [`layout_property_row`] publishes this number and [`gauge_control`] paints
/// it — the pair [[debt-paint-and-gesture-read-two-facts]] is about.
fn gauge_fill_w(frac: f64, track_w: u32) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "frac is clamped to [0,1] by ValueControl::fraction and the track is a small u32"
    )]
    let w = (frac * f64::from(track_w)) as u32;
    w
}

/// The colour chip's side — the text's cap height plus the ring around it.
fn swatch_side(style: &PropertyRowStyle) -> u32 {
    style.text_px + 6
}

/// The offset that centres a box of `inner` extent inside one of `outer`.
fn centre_offset(outer: u32, inner: u32) -> u32 {
    outer.saturating_sub(inner) / 2
}

/// Paint one property row: `[ indented name | value cell ]`, plus whatever
/// marks the screen puts at its trailing edge.
///
/// The row is tagged `{grid}#{id}` so a press routes to the coordinator, and
/// the value cell's control is whatever [`ValueControl::resolve`] decided — the
/// painter reaches no verdict of its own.
///
/// `trailing` is the caller's, and stays the caller's: a reset arrow, a remove
/// button and an add button are what a screen's *model* offers, not what a row
/// is. They come last so a press on one wins over the row underneath.
#[must_use]
pub fn view_property_row(
    tags: PropertyRowTags,
    row: &PropertyRow<'_>,
    style: &PropertyRowStyle,
    trailing: Vec<Scene>,
    theme: &Theme,
) -> Scene {
    let mut children = vec![
        name_cell(row, style, theme),
        value_cell(tags, row, style, theme),
    ];
    children.extend(trailing);
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{}#{}", tags.grid, row.id))
            .with_style(BoxStyle::filled(focus_fill(theme, row.focused)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(style.row_w(), style.row_h)),
            ),
    )
}

/// The name half: the depth inset, then the label.
fn name_cell(row: &PropertyRow<'_>, style: &PropertyRowStyle, theme: &Theme) -> Scene {
    let mut children: Vec<Scene> = Vec::new();
    let indent_px = row.depth * style.indent_step;
    if indent_px > 0 {
        children.push(Scene::Container(
            ContainerNode::new(Vec::new())
                .with_layout(LayoutStyle::new().with_size(Size::px(indent_px, style.row_h))),
        ));
    }
    children.push(run(
        row.label,
        style
            .name_col_w
            .saturating_sub(2 * style.cell_pad + indent_px),
        style,
        theme.resolve(ColorRole::OnSurfaceMuted),
    ));
    Scene::Container(
        ContainerNode::new(children).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(style.cell_pad, 0, style.cell_pad, 0))
                .with_size(Size::px(style.name_col_w, style.row_h)),
        ),
    )
}

/// The value half: the padded cell around whichever control the value opened.
fn value_cell(
    tags: PropertyRowTags,
    row: &PropertyRow<'_>,
    style: &PropertyRowStyle,
    theme: &Theme,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![control_scene(tags, row, style, theme)]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(style.cell_pad, 0, style.cell_pad, 0))
                .with_size(Size::px(style.value_col_w, style.row_h)),
        ),
    )
}

/// One control, dispatched on the **resolved** control and never on the value.
fn control_scene(
    tags: PropertyRowTags,
    row: &PropertyRow<'_>,
    style: &PropertyRowStyle,
    theme: &Theme,
) -> Scene {
    match &row.control {
        ValueControl::Field { state, caret } => view_field(
            tags.field,
            *state,
            *caret,
            theme,
            &TextFieldStyle {
                // The room the padded cell leaves, not a wider number the
                // solver would silently clamp — see `layout_property_row`.
                field_w: style.control_w(),
                field_h: style.row_h.saturating_sub(6),
                ..TextFieldStyle::m3_filled()
            },
            "",
        ),
        ValueControl::Toggle(b) => view_checkbox_box(
            *b,
            CheckboxState::Idle,
            theme,
            &CheckboxStyle {
                box_size: style.checkbox_size,
                glyph_size_px: 16,
                ..CheckboxStyle::m3_filled()
            },
        ),
        ValueControl::Gauge { value, .. } => gauge_control(row, *value, style, theme),
        ValueControl::Selector { selected, options } => {
            selector_control(*selected, options, style, theme)
        }
        ValueControl::Swatch(c) => swatch_control(*c, style, theme),
        // In a box of its own, not as a bare run, so its rectangle is the one
        // `layout_property_row` publishes for every other control — a value
        // that is text is still a control and answers the same question.
        ValueControl::Label(text) => Scene::Container(
            ContainerNode::new(vec![run(
                text,
                style.control_w(),
                style,
                theme.resolve(ColorRole::OnSurface),
            )])
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(style.control_w(), style.row_h)),
            ),
        ),
    }
}

/// R964's bounded read-out: the number, with the track and its fill under it.
///
/// The strips are `pointer_transparent`, so the gauge is a *visible range*
/// affordance and a press falls through to the row's own scrub. The fill
/// carries [`PropertyRow::part`], so its fraction is queryable as data.
fn gauge_control(
    row: &PropertyRow<'_>,
    value: f64,
    style: &PropertyRowStyle,
    theme: &Theme,
) -> Scene {
    let track_w = style.control_w();
    let fill_w = gauge_fill_w(row.control.fraction().unwrap_or(0.0), track_w);
    let strip_y = style.row_h.saturating_sub(style.gauge_track_h);
    let bar = |w: u32, fill: Color, tag: Option<String>| {
        let mut node = ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(fill).with_corner_radius(style.gauge_track_h / 2))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(w, style.gauge_track_h))
                    .with_absolute_position(0, strip_y)
                    .with_pointer_transparent(true),
            );
        if let Some(t) = tag {
            node = node.with_tag(t);
        }
        Scene::Container(node)
    };
    let idle = SliderState::Idle;
    Scene::Container(
        ContainerNode::new(vec![
            run(
                &CellValue::Float(value).display(),
                track_w,
                style,
                theme.resolve(ColorRole::OnSurface),
            ),
            bar(track_w, slider_track_inactive(theme, idle), None),
            bar(
                fill_w,
                slider_accent_for(theme, idle),
                row.part.map(str::to_owned),
            ),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(track_w, style.row_h)),
        ),
    )
}

/// The closed selector: the chosen option, and the chevron that says it opens.
fn selector_control(
    selected: usize,
    options: &[String],
    style: &PropertyRowStyle,
    theme: &Theme,
) -> Scene {
    let label = options.get(selected).map_or("", String::as_str);
    // The chevron is a read-out and keeps its column; the option name is what
    // gives way — the same order of precedence `config_form`'s badges have over
    // their key, and for the same reason (a mark shrunk to make room is a mark
    // painted at a size nobody chose).
    let chevron_w = style.text_px;
    Scene::Container(
        ContainerNode::new(vec![
            run(
                label,
                style.control_w().saturating_sub(chevron_w + 4),
                style,
                theme.resolve(ColorRole::OnSurface),
            ),
            run(
                SELECTOR_CHEVRON,
                chevron_w,
                style,
                theme.resolve(ColorRole::OnSurfaceMuted),
            ),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::SpaceBetween)
                .with_size(Size::px(style.control_w(), style.row_h)),
        ),
    )
}

/// The colour cell: a filled chip and the `#RRGGBB` it stands for.
fn swatch_control(color: Color, style: &PropertyRowStyle, theme: &Theme) -> Scene {
    let side = swatch_side(style);
    let chip = Scene::Container(
        ContainerNode::new(vec![])
            .with_style(
                BoxStyle::filled(color)
                    .with_corner_radius(4)
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(side, side))),
    );
    Scene::Container(
        ContainerNode::new(vec![
            chip,
            run(
                &color.to_hex(),
                style.control_w().saturating_sub(side + style.swatch_gap),
                style,
                theme.resolve(ColorRole::OnSurface),
            ),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(style.swatch_gap)
                .with_size(Size::px(style.control_w(), style.row_h)),
        ),
    )
}

/// The one text style every run in a row carries.
///
/// The overflow policy is the load-bearing part, and it is the argument
/// [`config_form`](crate::config_form) already makes: every string here is user
/// data in a box pinned to a constant, so without a policy the ones that
/// outgrow their column wrap onto the row below.
///
/// End-elision rather than the form's middle elision, because a property's name
/// and its value both read from the front — `Position X`, `12.5` — where a
/// configuration *path* carries information at both ends.
fn cell_text(style: &PropertyRowStyle, fg: Color) -> TextStyle {
    TextStyle::new()
        .with_size_px(style.text_px)
        .with_fg(fg)
        .with_overflow(TextOverflow::Ellipsis)
}

/// One run of cell text, in a box `w` wide.
///
/// ★★★★★ **The width is what makes the policy above do anything**, and this is
/// measured rather than assumed: the first draft of this module set
/// [`TextOverflow::Ellipsis`] and placed bare runs in flex cells, and the
/// containment gate reported a name overhanging its column by 269 px. A run
/// with no definite width is measured at its *content* size, so the shaper is
/// never asked to elide — the policy had nothing to clamp against.
///
/// Only the width is pinned. Pinning the height too would fix the line box
/// where the row's `AlignItems::Center` should be placing it, which is the
/// second copy of a fact this module exists to have one of.
fn run(text: &str, w: u32, style: &PropertyRowStyle, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(text, Rect::default(), cell_text(style, fg))
            .with_layout(LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Px(w)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;

    const TAGS: PropertyRowTags = PropertyRowTags {
        grid: "property_grid",
        field: "property_grid_edit",
    };

    /// The reactive scope the inline editor's own state hook needs.
    ///
    /// Required because `ValueControl::Field` paints a real
    /// [`text_field`](crate::text_field), which reads its buffer and its caret
    /// blink from the owner cache — a row painting an editor is not a picture
    /// of one.
    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    /// Every control the vocabulary declares, so a test that walks the roster
    /// cannot silently skip the arm nobody remembered.
    fn one_of_each() -> Vec<ValueControl> {
        vec![
            ValueControl::Field {
                state: TextFieldState::default(),
                caret: 0,
            },
            ValueControl::Toggle(true),
            ValueControl::Gauge {
                value: 0.25,
                lo: 0.0,
                hi: 1.0,
            },
            ValueControl::Selector {
                selected: 1,
                options: vec!["Static".to_owned(), "Dynamic".to_owned()],
            },
            ValueControl::Swatch(Color::rgb(0x33, 0x77, 0xdd)),
            ValueControl::Label("12.5".to_owned()),
        ]
    }

    /// ★ The type-to-editor dispatch, arm for arm — the whole reason this
    /// module exists rather than a `match` inside a screen.
    #[test]
    fn r1849_a_value_opens_the_control_its_type_declares() {
        /// A value, the bounds its slot declares, and the control it should
        /// open — named so the roster below reads as the table it is.
        type Case = (CellValue, Option<(f64, f64)>, &'static str);
        let cases: [Case; 6] = [
            (CellValue::Bool(true), None, "toggle"),
            (CellValue::Int(3), None, "label"),
            (CellValue::Float(12.5), None, "label"),
            (CellValue::Float(0.5), Some((0.0, 1.0)), "gauge"),
            (CellValue::Text("hero.fbx".to_owned()), None, "label"),
            (CellValue::Color(Color::rgb(1, 2, 3)), None, "swatch"),
        ];
        for (value, range, expected) in cases {
            let control = ValueControl::resolve(&value, range, None);
            assert_eq!(
                control.name(),
                expected,
                "{value:?} with range {range:?} opened {}",
                control.name()
            );
        }
        let choice = CellValue::Choice {
            selected: 0,
            options: vec!["A".to_owned()],
        };
        assert_eq!(
            ValueControl::resolve(&choice, None, None).name(),
            "selector"
        );
        // The roster is closed and the census is the definition's, so a seventh
        // control cannot be added without a token.
        assert_eq!(ValueControl::NAMES.len(), ValueControl::ARMS);
        let mut sorted = ValueControl::NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ValueControl::ARMS, "a token is spelled twice");
        for control in one_of_each() {
            assert!(
                ValueControl::NAMES.contains(&control.name()),
                "{} is not in the roster",
                control.name()
            );
        }
    }

    /// ★★★★★ The gate this module's second fact would fail.
    ///
    /// A stale `editing` handed in for a boolean would open a text field over a
    /// check box, which is exactly the divergence R1555 lifted
    /// `is_text_editable` to prevent. Asserted at the row, because the row is
    /// where the two facts meet.
    #[test]
    fn r1849_editing_a_row_that_is_not_text_editable_keeps_its_own_control() {
        let editing = Some((TextFieldState::default(), 0));
        for (value, expected) in [
            (CellValue::Bool(false), "toggle"),
            (CellValue::Color(Color::rgb(9, 9, 9)), "swatch"),
            (
                CellValue::Choice {
                    selected: 0,
                    options: vec!["A".to_owned()],
                },
                "selector",
            ),
        ] {
            assert_eq!(
                ValueControl::resolve(&value, None, editing).name(),
                expected,
                "{value:?} opened a field while being 'edited'",
            );
        }
        for value in [
            CellValue::Int(1),
            CellValue::Float(1.0),
            CellValue::Text("x".to_owned()),
        ] {
            assert_eq!(
                ValueControl::resolve(&value, None, editing).name(),
                "field",
                "{value:?} is text-editable and did not open the editor",
            );
        }
        // ★ A ranged float is the interesting one: it gauges when idle and
        // becomes a field when the editor is open on it, so the range does not
        // outrank the edit.
        assert_eq!(
            ValueControl::resolve(&CellValue::Float(0.5), Some((0.0, 1.0)), None).name(),
            "gauge",
        );
        assert_eq!(
            ValueControl::resolve(&CellValue::Float(0.5), Some((0.0, 1.0)), editing).name(),
            "field",
        );
    }

    /// The gauge's fraction, including the two ends and the degenerate range.
    #[test]
    fn r1849_a_gauge_reports_where_it_stands_and_only_a_gauge_does() {
        let at = |value, lo, hi| ValueControl::Gauge { value, lo, hi }.fraction();
        assert_eq!(at(0.0, 0.0, 1.0), Some(0.0));
        assert_eq!(at(1.0, 0.0, 1.0), Some(1.0));
        assert_eq!(at(0.25, 0.0, 1.0), Some(0.25));
        // Out of range clamps rather than reporting a strip longer than its
        // track — the paint has to stay inside the cell either way.
        assert_eq!(at(9.0, 0.0, 1.0), Some(1.0));
        assert_eq!(at(-9.0, 0.0, 1.0), Some(0.0));
        assert_eq!(at(5.0, 5.0, 5.0), Some(0.0), "a range with no room");
        for control in one_of_each() {
            if control.name() != "gauge" {
                assert_eq!(
                    control.fraction(),
                    None,
                    "{} reported a fraction",
                    control.name()
                );
            }
        }
    }

    /// ★★★★★ R1849 — **the published geometry is the layout the painter
    /// produces**, not a second arithmetic that agrees with it today.
    ///
    /// [[debt-paint-and-gesture-read-two-facts]] is this repository's
    /// most-repeated defect class, and a geometry helper is the shape that
    /// creates it: the screen looks right and the press lands somewhere else.
    /// So the prediction is run through the real layout pass and compared with
    /// what the solver actually put there, for every control the vocabulary
    /// declares and at more than one depth.
    #[test]
    fn r1849_the_published_geometry_is_the_layout_the_painter_produces() {
        use pinion_runtime::layout::compute_layout;
        let theme = Theme::default();
        let style = PropertyRowStyle::default();
        let mut checked = 0usize;
        for control in one_of_each() {
            for depth in [0u32, 2] {
                let spec = PropertyRow {
                    id: "opacity",
                    label: "Opacity",
                    depth,
                    control: control.clone(),
                    focused: depth == 0,
                    part: Some("gauge8"),
                };
                let geometry = layout_property_row(&spec, &style, (0, 0));
                let mut scene =
                    with_owner(|| view_property_row(TAGS, &spec, &style, Vec::new(), &theme));
                let mut cache = pinion_text::LayoutCache::new();
                compute_layout(&mut scene, &mut cache, style.row_w(), style.row_h);
                let Scene::Container(root) = &scene else {
                    panic!("a property row roots in a container");
                };
                assert_eq!(
                    root.rect,
                    geometry.row,
                    "{}: the row itself",
                    control.name()
                );
                let Scene::Container(name) = &root.children[0] else {
                    panic!("the name cell is a container");
                };
                assert_eq!(
                    name.rect,
                    geometry.name,
                    "{}: the name cell",
                    control.name()
                );
                let Scene::Container(value) = &root.children[1] else {
                    panic!("the value cell is a container");
                };
                assert_eq!(
                    value.rect,
                    geometry.value,
                    "{}: the value cell",
                    control.name()
                );
                // The control's own box — every arm, `Label` included, which is
                // why a value that is text still gets a container of its own.
                assert_eq!(
                    control_rect(&value.children[0]),
                    geometry.control,
                    "{}: the control's box",
                    control.name()
                );
                // The gauge fill, which is the one affordance with an
                // arithmetic box — and the number the caller queries.
                if control.name() == "gauge" {
                    let (suffix, rect) =
                        geometry.parts.first().expect("a gauge publishes its fill");
                    assert_eq!(suffix, "gauge8");
                    let painted = find_tagged(&scene, "gauge8").expect("the fill is painted");
                    assert_eq!(painted, *rect, "the gauge fill");
                } else {
                    assert!(
                        geometry.parts.is_empty(),
                        "{} published a part it does not paint",
                        control.name()
                    );
                }
                checked += 1;
            }
        }
        assert_eq!(
            checked,
            ValueControl::ARMS * 2,
            "every control, at two depths — a loop that stopped covering the \
             vocabulary would pass this vacuously",
        );
    }

    /// The rect a control occupies: its own box when it is a container, and the
    /// box it was placed in when it is a bare run.
    fn control_rect(scene: &Scene) -> Rect {
        match scene {
            Scene::Container(c) => c.rect,
            Scene::Text(t) => t.rect,
            other => panic!("a control paints a container or a run, not {other:?}"),
        }
    }

    /// The laid-out rect of the node carrying that tag.
    fn find_tagged(scene: &Scene, tag: &str) -> Option<Rect> {
        let mut found = None;
        scene.for_each_node(&mut |visit| {
            if let Scene::Container(c) = visit.node
                && c.tag.as_deref() == Some(tag)
            {
                found = Some(c.rect);
            }
        });
        found
    }

    /// A hit test reads the geometry the paint was made from — including the
    /// translation a scrolled pane applies.
    #[test]
    fn r1849_a_scrolled_row_answers_where_it_actually_is() {
        let style = PropertyRowStyle::default();
        let spec = PropertyRow {
            id: "opacity",
            label: "Opacity",
            depth: 1,
            control: ValueControl::Gauge {
                value: 0.5,
                lo: 0.0,
                hi: 1.0,
            },
            focused: false,
            part: Some("gauge8"),
        };
        let geometry = layout_property_row(&spec, &style, (0, 100));
        assert!(geometry.hit(10, 110));
        assert!(!geometry.hit(10, 90));
        let (_, fill) = &geometry.parts[0];
        assert_eq!(
            geometry.part_at(fill.x + 1, fill.y + 1),
            Some("gauge8"),
            "the fill is reachable where it was drawn",
        );
        // Scrolled up by 40: everything moves with it, and nothing is left
        // claiming its old seat.
        let moved = geometry.translated(0, -40);
        assert!(moved.hit(10, 70));
        assert!(!moved.hit(10, 110));
        assert_eq!(moved.row.y, geometry.row.y - 40);
        assert_eq!(moved.parts[0].1.y, geometry.parts[0].1.y - 40);
        // Scrolled past the top: a row that is no longer anywhere answers no
        // hit rather than answering the top edge.
        let gone = geometry.translated(0, -200);
        assert!(!gone.hit(10, 0));
        assert!(gone.parts.is_empty(), "a part off the top is dropped");
    }

    /// The crate's containment gate — a painter that strokes a border (the
    /// colour chip's ring) must show that its own contents stay inside its own
    /// frame, at more than one size.
    #[test]
    fn r1849_a_property_row_keeps_its_contents_inside_its_own_frame() {
        let theme = Theme::default();
        let style = PropertyRowStyle::default();
        with_owner(|| {
            for control in one_of_each() {
                let name = control.name();
                crate::frame_gate::assert_frame_contained(
                    &format!("property row ({name})"),
                    &mut |_w, _h| {
                        let spec = PropertyRow {
                            id: "tint",
                            label: "Tint",
                            depth: 1,
                            control: control.clone(),
                            focused: false,
                            part: Some("gauge8"),
                        };
                        view_property_row(TAGS, &spec, &style, Vec::new(), &theme)
                    },
                );
            }
        });
    }

    /// A label longer than its column elides instead of reaching the row below.
    ///
    /// The exposure the crate's own note names: a box pinned to a constant with
    /// a run that outgrows it moves only the ink, so this is the property that
    /// keeps the gate above honest on a host with wider faces.
    #[test]
    fn r1849_a_name_wider_than_its_column_gives_way() {
        let theme = Theme::default();
        let style = PropertyRowStyle::default();
        let spec = PropertyRow {
            id: "long",
            label: "A property name far longer than the column it has to live in",
            depth: 3,
            control: ValueControl::Label(
                "a value that is itself much longer than its own column".to_owned(),
            ),
            focused: false,
            part: None,
        };
        let mut scene = view_property_row(TAGS, &spec, &style, Vec::new(), &theme);
        let mut cache = pinion_text::LayoutCache::new();
        pinion_runtime::layout::compute_layout(&mut scene, &mut cache, style.row_w(), style.row_h);
        let escapes = pinion_core::containment::escapes(&scene, &mut |text| {
            let max_width = (text.rect.w > 0).then_some(text.rect.w);
            cache.ink_size(&text.content, &text.style, &text.runs, max_width)
        });
        assert!(
            escapes.is_empty(),
            "{} mark(s) left the box that owns them: {:?}",
            escapes.len(),
            escapes
                .iter()
                .map(|e| (e.content.clone(), e.owner.clone(), e.over))
                .collect::<Vec<_>>()
        );
    }
}
