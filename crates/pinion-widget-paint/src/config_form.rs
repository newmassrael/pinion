//! R1651 §5.21 §5.38 — the **node inspector**: a settings form laid out from a
//! [`ConfigForm`], where each row says what its key is, what shape its value
//! has, whether editing it reaches a running program, and what is wrong with it.
//!
//! ## Why this is here rather than in an example
//!
//! Measured at R1646: no crate held a property grid — it existed in two
//! examples, so an application that wanted one copied an example. That is what
//! kept the analysis-tool census row for *"a node inspector that is the
//! settings editor"* at `gap` in its must-have tier, and the per-field
//! applies badge unanswerable, because the widget had nowhere to be. R1650 gave
//! the **model** a home in `pinion_core::widgets::config_form` and left it with
//! no painter and no consumer; this is the other half.
//!
//! ## Geometry is computed once and read twice
//!
//! [`form_geometry`] returns every rectangle, and [`view_config_form`] paints
//! *from* that value. A consumer's hit test reads the same [`FormGeometry`], so
//! there is no second copy of the layout to drift from the first — the property
//! R1648 lost by hand-placing children and R1649's sweep exists to keep.
//!
//! ## The layout policies, and where they come from
//!
//! The floor is the mature toolkit's form layout at 6.11, **measured** rather
//! than read (R1651 built it from source and ran a three-row form through it):
//!
//! * [`RowWrap`] — its row-wrap policy. Three settings, and the third is
//!   derived per row: measured there, a 320 px box kept all three rows beside
//!   their labels while a 140 px box moved only the row whose label measured
//!   153 px.
//! * [`FieldGrowth`] — its field-growth policy. Measured beside the label, a
//!   control was 108 px at its size hint and 161 px when allowed to grow.
//! * A hidden row keeps its place and takes no space
//!   ([`ConfigField::hidden`]) — measured there, hiding the middle of three
//!   rows took the form from 159 px to 104 px with the row count unchanged.
//!
//! What that toolkit has no vocabulary for, and this paints: the applies badge,
//! the defect on the row it belongs to, and the launch verdict derived from
//! both. One measured finding cuts the other way and is worth carrying: its
//! label→control accessible relation is **not** automatic — passing a label the
//! application owns leaves the relation unset. Here the row's access node
//! carries the key, always, because [`row_access_nodes`] derives them from the
//! same geometry the paint came from.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, describedby_region};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::config_form::{
    Applies, ConfigDefect, ConfigField, ConfigForm, FieldType,
};
use pinion_core::{Scene, measured_text_extent};

/// R1654 §5.36 — the base style every run in this form carries.
///
/// ★ The overflow policy is the load-bearing part. Every run here is placed in
/// an exact rectangle derived from the row geometry, which fixes its WIDTH, and
/// every string in it is user data: a configuration path, an endpoint, a
/// permission list. Without a policy the ones that outgrow their box wrap to a
/// second line and land on the row below — the smear R1653's box-measuring
/// sweep could not see and R1654's `TextOverflow` arms exist to prevent.
///
/// `EllipsisMiddle`, not end-elision, because both ends of these strings carry
/// information: `transport.link.tx.batch_size` and `transport.link.tx.queue`
/// share a 24-character prefix, and `tcp/10.0.0.21:7449` differs from its
/// neighbour in the port. The doc on [`RowWrap::Beside`] has claimed since
/// R1651 that "a key wider than the column is elided rather than moving" — this
/// is the round that makes that sentence true.
fn form_run_style() -> TextStyle {
    TextStyle::new().with_overflow(TextOverflow::EllipsisMiddle)
}

/// Where a row's control sits relative to its key.
///
/// The mature toolkit's row-wrap policy, arm for arm. Named for what the row
/// does rather than for what it does not, so [`Self::WrapLong`] reads as the
/// derived case it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum RowWrap {
    /// Control beside the key, always. The key column is as wide as the widest
    /// key, and a key wider than the column is elided rather than moving.
    Beside,
    /// Beside the key unless the pair does not fit, and then below it —
    /// **derived per row** from the measured key width, not chosen per row.
    WrapLong,
    /// Control below the key, always. What the reference tool's inspector does,
    /// because a configuration path is long and a column wide enough for
    /// `transport.link.tx.batch_size` leaves no room for its value.
    WrapAll,
}

impl RowWrap {
    /// All three, so a consumer covers the vocabulary by enumerating.
    pub const ALL: [Self; 3] = [Self::Beside, Self::WrapLong, Self::WrapAll];
}

/// How much of the spare width a row's control takes.
///
/// The mature toolkit's field-growth policy, arm for arm. The distinction
/// between the last two is not decoration: a text box wants every pixel and a
/// row of option chips wants exactly its content, and a policy that could not
/// tell them apart would either strand the text box or stretch the chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum FieldGrowth {
    /// Every control keeps its natural width.
    AtSizeHint,
    /// Only the controls that want to expand do — a text box grows, a chip row
    /// hugs its options.
    ExpandingGrow,
    /// Every control fills the row.
    AllGrow,
}

impl FieldGrowth {
    /// All three.
    pub const ALL: [Self; 3] = [Self::AtSizeHint, Self::ExpandingGrow, Self::AllGrow];

    /// Whether a control of this appetite fills the space it is offered.
    const fn fills(self, hungry: bool) -> bool {
        match self {
            Self::AtSizeHint => false,
            Self::ExpandingGrow => hungry,
            Self::AllGrow => true,
        }
    }
}

/// The measurements a form is laid out with.
///
/// Tokens rather than parameters where three consumers would have to agree or
/// be wrong, and parameters where they genuinely differ — the width of the pane
/// the form is in is the caller's, and so is the policy pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormStyle {
    /// The width the form is laid out into.
    pub width: u32,
    /// Where each control sits relative to its key.
    pub wrap: RowWrap,
    /// How much spare width a control takes.
    pub growth: FieldGrowth,
    /// The key's text size.
    pub key_px: u32,
    /// The value's text size.
    pub value_px: u32,
    /// The height of a text control.
    pub control_h: u32,
    /// Vertical space between rows.
    pub row_gap: u32,
    /// Vertical space between a key and the control below it, when wrapped.
    pub wrap_gap: u32,
    /// Horizontal space between a key and the control beside it.
    pub beside_gap: u32,
    /// The natural width of a text control, before any growth policy.
    pub control_hint_w: u32,
}

impl Default for FormStyle {
    /// The reference inspector's own measurements: a 312 px pane with 14 px
    /// padding, keys above their controls, controls filling the row.
    fn default() -> Self {
        Self {
            width: 284,
            wrap: RowWrap::WrapAll,
            growth: FieldGrowth::AllGrow,
            key_px: 11,
            value_px: 12,
            control_h: 31,
            row_gap: 14,
            wrap_gap: 5,
            beside_gap: 8,
            control_hint_w: 108,
        }
    }
}

impl FormStyle {
    /// The same measurements laid out into a pane of `width`.
    #[must_use]
    pub const fn with_width(mut self, width: u32) -> Self {
        self.width = width;
        self
    }

    /// The same measurements under a different policy pair.
    #[must_use]
    pub const fn with_policy(mut self, wrap: RowWrap, growth: FieldGrowth) -> Self {
        self.wrap = wrap;
        self.growth = growth;
        self
    }
}

/// Where one row's parts landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowBox {
    /// The configuration path this row is about.
    pub key: String,
    /// The whole row, key and control together — what a click on the row hits.
    pub row: Rect,
    /// The key, its type badge and its applies badge.
    pub header: Rect,
    /// The control.
    pub control: Rect,
    /// Whether the control ended up below the key rather than beside it.
    ///
    /// Derived under [`RowWrap::WrapLong`], so a caller can see which rows the
    /// policy moved without re-measuring the text.
    pub wrapped: bool,
    /// Every **affordance inside the control**, as the tag suffix the painter
    /// gives it and the rectangle it landed in.
    ///
    /// The suffix is relative to the form's prefix — `option.<key>.<word>`,
    /// `step.<key>.up`, `toggle.<key>`, `item.<key>.<n>` — so a consumer's hit
    /// test iterates this list and never spells a shape's layout out again.
    ///
    /// ★ R1651.1 published the option rectangles because computing them twice
    /// is how R1651 shipped an option row whose second chip could not be
    /// pressed. R1652 generalises it to every shape for the same reason, before
    /// rather than after: a stepper and a checkbox are the same hazard.
    pub parts: Vec<(String, Rect)>,
}

impl RowBox {
    /// Where the part with that tag suffix landed.
    #[must_use]
    pub fn part(&self, suffix: &str) -> Option<Rect> {
        self.parts
            .iter()
            .find(|(name, _)| name == suffix)
            .map(|(_, rect)| *rect)
    }
}

/// Where every part of a form landed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FormGeometry {
    /// Where the form was laid out — what [`view_config_form`] places its
    /// container at, and what every rect below is measured from. Carried so the
    /// paint can subtract it exactly once: a container's absolutely-placed
    /// children are positioned relative to *it*, and R1648 drew a whole screen
    /// at twice its intended offset by forgetting that.
    pub origin: (u32, u32),
    /// The rows that are shown, in order. A [`ConfigField::hidden`] row is
    /// absent here and still present in the form — it takes no space and keeps
    /// its place and its defect.
    pub rows: Vec<RowBox>,
    /// The offered keys, as the chips that add them.
    pub chips: Vec<(String, Rect)>,
    /// How tall the whole form came out.
    pub height: u32,
}

impl FormGeometry {
    /// The same form seen from somewhere else: every rectangle moved by
    /// `(dx, dy)`.
    ///
    /// ★ R1662 — what a scrolling pane needs, and the reason it is here rather
    /// than in each consumer. A pane that scrolls paints the form in its own
    /// content frame while a pointer, a screen reader and the wire all ask in
    /// window coordinates, so the two frames have to be related by exactly one
    /// piece of arithmetic. Written twice they drift, and the drift is
    /// invisible: the screen looks right and the press lands on the row above
    /// ([[debt-paint-and-gesture-read-two-facts]]).
    ///
    /// A row or chip the translation would move to a negative coordinate is
    /// **dropped, not clamped**. Clamping would report it at the edge, which is
    /// a position it is not at — a screen reader would announce it as visible
    /// and a hit test would answer it for a press on whatever really is there.
    /// Dropping says the truthful thing: the reader has scrolled it away.
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
            origin: moved(Rect::new(self.origin.0, self.origin.1, 0, 0))
                .map_or((0, 0), |r| (r.x, r.y)),
            rows: self
                .rows
                .iter()
                .filter_map(|row| {
                    Some(RowBox {
                        key: row.key.clone(),
                        row: moved(row.row)?,
                        header: moved(row.header)?,
                        control: moved(row.control)?,
                        wrapped: row.wrapped,
                        parts: row
                            .parts
                            .iter()
                            .filter_map(|(s, r)| Some((s.clone(), moved(*r)?)))
                            .collect(),
                    })
                })
                .collect(),
            chips: self
                .chips
                .iter()
                .filter_map(|(k, r)| Some((k.clone(), moved(*r)?)))
                .collect(),
            height: self.height,
        }
    }

    /// The row at that path, if it is shown.
    #[must_use]
    pub fn row(&self, key: &str) -> Option<&RowBox> {
        self.rows.iter().find(|r| r.key == key)
    }

    /// The chip that adds that key, if it is offered.
    #[must_use]
    pub fn chip(&self, key: &str) -> Option<Rect> {
        self.chips
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, rect)| *rect)
    }
}

/// The width the key column needs: the widest key, measured.
///
/// Falls back to a per-character estimate when no shell has seeded a text
/// measurer (headless, unit tests). The estimate is only ever read when the
/// real measurement is unavailable, and it is stated here rather than buried:
/// keys are shown in the monospace face, whose advance is close to `0.6 em`.
fn key_column_width(form: &ConfigForm, style: &FormStyle) -> u32 {
    let text_style = form_run_style().with_size_px(style.key_px);
    form.fields()
        .iter()
        .filter(|f| !f.hidden())
        .map(|f| measured_key_width(f.key(), &text_style, style.key_px))
        .max()
        .unwrap_or(0)
}

fn measured_key_width(key: &str, text_style: &TextStyle, key_px: u32) -> u32 {
    measured_text_extent(key, text_style, None).map_or_else(
        || u32::try_from(key.chars().count()).unwrap_or(0) * key_px * 3 / 5,
        pinion_core::TextExtent::width,
    )
}

/// How tall a field's control has to be to hold what it draws.
///
/// Every shape but one is a single line. A [`FieldType::List`] is one row per
/// element plus the row that adds one, so its height is a function of its
/// **value** — which is why this takes the field and not the shape.
fn control_height(field: &ConfigField, style: &FormStyle) -> u32 {
    match field.shape() {
        FieldType::List { .. } => {
            let elements = u32::try_from(FieldType::elements(field.value()).count()).unwrap_or(0);
            elements * (style.control_h + LIST_GAP) + ADD_CHIP_H
        }
        _ => style.control_h,
    }
}

/// Whether a field's control wants every pixel it is offered.
///
/// A text box does; a row of option chips does not, and stretching it would put
/// the chips' own borders in the wrong places. This is the distinction the
/// middle growth policy exists to make.
const fn control_is_hungry(shape: &FieldType) -> bool {
    !matches!(shape, FieldType::Choice { .. } | FieldType::Flags { .. })
}

/// The natural width of a field's control, before any growth policy.
fn control_hint(field: &ConfigField, style: &FormStyle) -> u32 {
    match field.shape() {
        FieldType::Choice { of } | FieldType::Flags { of } => {
            let text_style = form_run_style().with_size_px(style.key_px);
            let chips: u32 = of
                .iter()
                .map(|word| measured_key_width(word, &text_style, style.key_px) + CHIP_PAD * 2)
                .sum();
            chips + CHIP_GAP * u32::try_from(of.len().saturating_sub(1)).unwrap_or(0)
        }
        _ => style.control_hint_w,
    }
}

/// Horizontal padding inside an option chip.
const CHIP_PAD: u32 = 10;
/// Space between option chips.
const CHIP_GAP: u32 = 6;
/// Height of the "add this key" chip row entries.
const ADD_CHIP_H: u32 = 24;

/// A badge's horizontal padding, one side. Named because the header's width
/// budget has to add it back (R1656).
const BADGE_PAD: u32 = 6;

/// The gap between the header's items, which the width budget also spends.
const HEADER_GAP: u32 = 6;
/// A numeric stepper button's width.
const STEP_W: u32 = 26;
/// Vertical space between a list's element rows.
const LIST_GAP: u32 = 4;

/// Lay a form out, giving every part a rectangle.
///
/// `origin` is where the form's first row starts. The geometry is the single
/// source both [`view_config_form`] and a consumer's hit test read.
#[must_use]
pub fn form_geometry(form: &ConfigForm, origin: (u32, u32), style: &FormStyle) -> FormGeometry {
    let (x0, y0) = origin;
    let key_col = key_column_width(form, style);
    let mut y = y0;
    let mut rows = Vec::new();

    for field in form.fields().iter().filter(|f| !f.hidden()) {
        let row = lay_row(field, (x0, y), key_col, style);
        y += row.row.h + style.row_gap;
        rows.push(row);
    }

    let (chips, after) = lay_chips(form, (x0, y), style);
    y = after;

    FormGeometry {
        origin,
        rows,
        chips,
        height: y.saturating_sub(y0),
    }
}

/// One row's rectangles, under the style's policy pair.
fn lay_row(field: &ConfigField, at: (u32, u32), key_col: u32, style: &FormStyle) -> RowBox {
    let (x0, y) = at;
    // ★ R1656 — the shaper's LINE box for this face, not the face's size plus a
    // number somebody picked. `key_px + 7` was one short of it, so every key
    // label on every consumer of this widget painted a pixel below its own row
    // — invisible to a boolean overflow flag that was true of most runs anyway,
    // and caught the moment `pinion_core::containment` asked per edge.
    let key_line = pinion_core::containment::line_box(style.key_px);
    {
        let hungry = control_is_hungry(field.shape());
        let hint = control_hint(field, style);
        let wrapped = match style.wrap {
            RowWrap::WrapAll => true,
            RowWrap::Beside => false,
            // Derived, not chosen: the pair wraps exactly when it does not fit.
            RowWrap::WrapLong => {
                let key_w = measured_key_width(
                    field.key(),
                    &form_run_style().with_size_px(style.key_px),
                    style.key_px,
                );
                key_w + style.beside_gap + hint > style.width
            }
        };
        // ★ R1652.1 — the control's height is a function of the SHAPE, not a
        // token. A list draws one row per element and R1652 gave it
        // `control_h` regardless, so a list of six painted its rows straight
        // over the next field. Measured, not reasoned: an audit grew the list
        // and read the rectangles back off the scene.
        let control_h = control_height(field, style);
        let (header, control) = if wrapped {
            let offered = style.width;
            let w = if style.growth.fills(hungry) {
                offered
            } else {
                hint.min(offered)
            };
            (
                Rect::new(x0, y, style.width, key_line),
                Rect::new(x0, y + key_line + style.wrap_gap, w, control_h),
            )
        } else {
            let offered = style.width.saturating_sub(key_col + style.beside_gap);
            let w = if style.growth.fills(hungry) {
                offered
            } else {
                hint.min(offered)
            };
            (
                Rect::new(x0, y, key_col, control_h),
                Rect::new(x0 + key_col + style.beside_gap, y, w, control_h),
            )
        };
        let row_h = if wrapped {
            key_line + style.wrap_gap + control_h
        } else {
            control_h
        };
        RowBox {
            key: field.key().to_string(),
            row: Rect::new(x0, y, style.width, row_h),
            header,
            control,
            wrapped,
            parts: lay_parts(field, control, style),
        }
    }
}

/// Where a row's affordances land inside its control.
///
/// One function over every shape, because the alternative is one per shape and
/// a consumer that has to know which. What each shape gets:
///
/// | shape | parts |
/// |---|---|
/// | [`FieldType::Text`] | none — the control *is* the box |
/// | [`FieldType::Integer`] | `step.<key>.down`, `step.<key>.up` |
/// | [`FieldType::Boolean`] | `toggle.<key>` |
/// | [`FieldType::Choice`] / [`FieldType::Flags`] | `option.<key>.<word>` each |
/// | [`FieldType::List`] | `item.<key>.<n>` each, then `item.<key>.add` |
fn lay_parts(field: &ConfigField, control: Rect, style: &FormStyle) -> Vec<(String, Rect)> {
    let key = field.key();
    let text_style = form_run_style().with_size_px(style.key_px);
    // ★★ R1672 — inside the control's CONTENT box, not its box. A part laid at
    // the box covers the outline the box strokes inside itself, which is a gap
    // in that outline wherever the part sits; `pinion_core::containment` calls
    // it an escape from the moment it learned the distinction.
    let control = inset_by(control, control_frame(field.shape()));
    match field.shape() {
        FieldType::Text => Vec::new(),
        FieldType::Integer { .. } => {
            // A stepper pair at the trailing edge, so the value's text keeps
            // the left of the box and the two buttons never overlap it.
            let w = STEP_W;
            let right = control.x + control.w;
            vec![
                (
                    format!("step.{key}.down"),
                    Rect::new(right.saturating_sub(w * 2), control.y, w, control.h),
                ),
                (
                    format!("step.{key}.up"),
                    Rect::new(right.saturating_sub(w), control.y, w, control.h),
                ),
            ]
        }
        FieldType::Boolean => vec![(
            format!("toggle.{key}"),
            Rect::new(control.x, control.y, control.h, control.h),
        )],
        FieldType::Choice { of } | FieldType::Flags { of } => {
            let mut x = control.x;
            let mut placed = Vec::new();
            for word in of {
                let w = measured_key_width(word, &text_style, style.key_px) + CHIP_PAD * 2;
                placed.push((
                    format!("option.{key}.{word}"),
                    Rect::new(x, control.y, w, control.h),
                ));
                x += w + CHIP_GAP;
            }
            placed
        }
        FieldType::List { .. } => {
            // One row per element, then the row that adds one, all INSIDE the
            // control — whose height `control_height` sized for exactly this.
            let mut placed = Vec::new();
            let mut y = control.y;
            for (n, _) in FieldType::elements(field.value()).enumerate() {
                placed.push((
                    format!("item.{key}.{n}"),
                    Rect::new(control.x, y, control.w, style.control_h),
                ));
                y += style.control_h + LIST_GAP;
            }
            placed.push((
                format!("item.{key}.add"),
                Rect::new(control.x, y, control.w, ADD_CHIP_H),
            ));
            debug_assert!(
                y + ADD_CHIP_H <= control.y + control.h,
                "a list's rows must fit the control `control_height` sized"
            );
            placed
        }
    }
}

/// The chips that add an offered key, wrapped into the pane's width.
fn lay_chips(form: &ConfigForm, at: (u32, u32), style: &FormStyle) -> (Vec<(String, Rect)>, u32) {
    let (x0, mut y) = at;
    let mut chips = Vec::new();
    let mut chip_x = x0;
    let text_style = form_run_style().with_size_px(style.key_px);
    for offered in form.addable() {
        let w = measured_key_width(offered.key(), &text_style, style.key_px) + CHIP_PAD * 2 + 12;
        if chip_x + w > x0 + style.width && chip_x > x0 {
            chip_x = x0;
            y += ADD_CHIP_H + CHIP_GAP;
        }
        chips.push((
            offered.key().to_string(),
            Rect::new(chip_x, y, w, ADD_CHIP_H),
        ));
        chip_x += w + CHIP_GAP;
    }
    if !chips.is_empty() {
        y += ADD_CHIP_H;
    }
    (chips, y)
}

/// How wide the laid-out form came out — the widest thing in it.
fn width_of(geometry: &FormGeometry) -> u32 {
    geometry
        .rows
        .iter()
        .map(|r| r.row.w)
        .chain(
            geometry
                .chips
                .iter()
                .map(|(_, c)| c.x + c.w - geometry.origin.0),
        )
        .max()
        .unwrap_or(0)
}

/// Place a node at `rect`, expressed in the same space [`form_geometry`]
/// reported — the paint subtracts the form's origin exactly once, here.
fn placed(layout: LayoutStyle, rect: Rect, origin: (u32, u32)) -> LayoutStyle {
    layout
        .with_absolute_position(
            rect.x.saturating_sub(origin.0),
            rect.y.saturating_sub(origin.1),
        )
        .with_size(Size::px(rect.w, rect.h))
        // ★ Pointer-TRANSPARENT, and this is load-bearing rather than a
        // default. The router resolves a press to the deepest TAGGED node
        // under the cursor and then looks for that tag's `External`; a tagged
        // node that is an address rather than a primitive therefore SWALLOWS
        // the press and forwards nothing, which is how R1649.1 found a whole
        // screen dead to a real mouse while 118 scripted assertions passed.
        // The form publishes its geometry instead, and its consumer's hit test
        // reads that — one fact, not two.
        .with_pointer_transparent(true)
}

/// The badge colours a row's applies-scope is shown in.
fn applies_ink(applies: Applies, theme: &Theme) -> Color {
    match applies {
        Applies::Hot => theme.resolve(ColorRole::Accent),
        Applies::Restart => theme.resolve(ColorRole::OnSurfaceMuted),
    }
}

fn badge(text: &str, ink: Color, theme: &Theme, tag: Option<String>) -> Scene {
    let label = Scene::Text(TextNode::styled(
        text.to_owned(),
        Rect::default(),
        form_run_style().with_size_px(9).with_fg(ink),
    ));
    let mut node = ContainerNode::new(vec![label])
        .with_style(
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
                .with_corner_radius(4)
                .with_border(Border::new(ink, 1)),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_padding(Rect::new(BADGE_PAD, 2, BADGE_PAD, 2))
                // ★ R1656 — a badge is a read-out and must not be shrunk to
                // make room; the key beside it is what gives way (R1536
                // measured a 10px mark painted at 6px when a decoration was
                // allowed to absorb a deficit).
                .with_flex_shrink(0.0)
                // ★ R1655 — a badge is a READ-OUT, and a tagged node that is
                // not transparent becomes the §5.35 router's hit target: the
                // router looks its tag up as an `External`, finds none (the tag
                // is an address, not a widget), and forwards NOTHING. Wherever
                // a badge is painted the form was dead to a real mouse, while
                // every wire-driven assertion about it passed — R1649.1's
                // class, in a crate, so every consumer of this painter had it.
                .with_pointer_transparent(true),
        );
    if let Some(tag) = tag {
        node = node.with_tag(tag);
    }
    Scene::Container(node)
}

/// Paint a form from the geometry it was laid out into.
///
/// `tag_prefix` addresses the parts: `<prefix>.row.<key>`,
/// `<prefix>.control.<key>`, `<prefix>.applies.<key>` and `<prefix>.add.<key>`.
/// One naming rule rather than a per-consumer convention, so an agent driving
/// the form by path does not have to learn each screen's spelling.
#[must_use]
pub fn view_config_form(
    tag_prefix: &str,
    form: &ConfigForm,
    geometry: &FormGeometry,
    theme: &Theme,
) -> Scene {
    let defects = form.defects();
    let mut children: Vec<Scene> = Vec::new();

    for row in &geometry.rows {
        let Some(field) = form.field(&row.key) else {
            continue;
        };
        let row_defects: Vec<&ConfigDefect> =
            defects.iter().filter(|d| d.key() == row.key).collect();
        // The blocking one when there is one, and otherwise the warning — a
        // row shows the news that matters most, and never nothing when there
        // is something.
        let worst = row_defects
            .iter()
            .find(|d| d.blocks())
            .or(row_defects.first())
            .copied();
        children.push(view_header(
            tag_prefix,
            field,
            row,
            worst,
            geometry.origin,
            theme,
        ));
        children.push(view_control(
            tag_prefix,
            field,
            row,
            worst,
            geometry.origin,
            theme,
        ));
    }

    for (key, rect) in &geometry.chips {
        children.push(view_add_chip(
            tag_prefix,
            key,
            *rect,
            geometry.origin,
            theme,
        ));
    }

    Scene::Container(
        ContainerNode::new(children).with_layout(
            LayoutStyle::new()
                .with_absolute_position(geometry.origin.0, geometry.origin.1)
                .with_size(Size::px(width_of(geometry), geometry.height))
                .with_pointer_transparent(true),
        ),
    )
}

/// A row's key, its declared type, its applies badge, and its defect.
fn view_header(
    tag_prefix: &str,
    field: &ConfigField,
    row: &RowBox,
    worst: Option<&ConfigDefect>,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    {
        // The header: key, declared type, applies badge, and the defect when
        // there is one. The defect sits ON the row rather than only in a list
        // at the bottom, which is why `ConfigDefect` carries its key.
        // ★ R1656 — the LAYOUT decides who gives way, not an estimate here.
        //
        // The key was `Rect::default()`, and a zero-width box means "no
        // maximum" to the shaper, so the eliding policy this style has declared
        // since R1654 could never fire: a long path simply pushed the badges
        // past the header's own right edge. Measured the first time
        // `pinion_core::containment` was pointed at a real screen — the defect
        // badge on `transport.link.tx.batch_size` was painted 7px outside its
        // row, and nothing else could see it because the row's box was right,
        // the badge's box was right, and only their SUM was wrong.
        //
        // The first repair computed a width budget by measuring the badge
        // strings, and it was still 7px short — a fallback advance estimate is
        // not a shaped advance, and a gate built on the difference is a gate
        // that is green on one host. So the deficit is handed to the flex pass:
        // the badges refuse to shrink (they are read-outs, and R1536 measured
        // what shrinking one costs), the key is allowed to shrink below its
        // content, and the shaper then elides it to the width it was actually
        // given. No number in this file has to be right.
        let mut header: Vec<Scene> = vec![Scene::Text(
            TextNode::styled(
                field.key().to_owned(),
                Rect::default(),
                form_run_style()
                    .with_size_px(11)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_min_size(Size::px(0, 0))
                    .with_flex_shrink(1.0),
            ),
        )];
        header.push(badge(
            field.ty(),
            theme.resolve(ColorRole::OnSurfaceMuted),
            theme,
            None,
        ));
        header.push(badge(
            field.applies().wire(),
            applies_ink(field.applies(), theme),
            theme,
            Some(format!("{tag_prefix}.applies.{}", row.key)),
        ));
        if let Some(defect) = worst {
            let ink = if defect.blocks() {
                theme.resolve(ColorRole::Error)
            } else {
                theme.resolve(ColorRole::Warning)
            };
            header.push(badge(
                defect.wire(),
                ink,
                theme,
                Some(format!("{tag_prefix}.defect.{}", row.key)),
            ));
        }
        Scene::Container(
            ContainerNode::new(header).with_layout(placed(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(HEADER_GAP)
                    .with_pointer_transparent(true),
                row.header,
                origin,
            )),
        )
    }
}

/// A row's control: an option set shows every option with the chosen ones
/// marked; everything else is a text box holding the value.
fn view_control(
    tag_prefix: &str,
    field: &ConfigField,
    row: &RowBox,
    worst: Option<&ConfigDefect>,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    {
        match field.shape() {
            FieldType::Choice { .. } | FieldType::Flags { .. } => {
                let chosen: Vec<&str> = FieldType::elements(field.value()).collect();
                option_chips(tag_prefix, row, &chosen, origin, theme)
            }
            FieldType::Boolean => boolean_control(tag_prefix, row, field, origin, theme),
            FieldType::Integer { .. } => {
                number_control(tag_prefix, row, field, worst, origin, theme)
            }
            FieldType::List { .. } => list_control(tag_prefix, row, field, origin, theme),
            FieldType::Text => text_control(tag_prefix, row, field, worst, origin, theme),
        }
    }
}

/// The skin every boxed control shares: the surface tone, the corner and the
/// border, which turns to [`ColorRole::Error`] when the row's defect blocks.
///
/// One chooser rather than four, because a divergence between the shapes here
/// would be a bug: "this value stops a launch" is a fact about the row, not
/// about which control the row happens to draw.
/// The width of the outline [`control_skin`] draws INSIDE a control's box.
///
/// ★ R1672 — reserved by [`framed`] so a control's content cannot sit on its own
/// outline. Measured the moment `containment` learned the border-box /
/// content-box distinction: five marks in one form were flush against the frame
/// they are drawn inside, which is a gap in the outline wherever they touch it.
const CONTROL_FRAME: u32 = 1;

/// The width of the outline the container that OWNS a shape's parts draws
/// inside its own box — so the inset those parts are laid within.
///
/// ★★ R1672 — the **layout** half of the same fact [`framed`] is the paint half
/// of, and the reason this is a function of the shape rather than a constant:
/// only two of the six shapes put their parts inside a box that strokes itself.
///
/// A part rectangle is published ([`RowBox::parts`]) and then both painted at
/// and pressed at, so it is not enough for the painter to add padding: an
/// absolutely-placed child ignores its parent's padding, which is exactly what
/// the measurement showed. R1672 added the padding, re-measured, and found the
/// two stepper buttons still standing on the control's outline — the padding
/// had moved the value text and nothing else.
///
/// The correspondence to what is actually painted is asserted over **every**
/// arm by `r1672_every_shapes_control_frame_is_the_one_it_paints`, so a shape
/// that gains or loses a skin cannot leave this behind.
const fn control_frame(shape: &FieldType) -> u32 {
    match shape {
        // The control container IS the box: it draws [`control_skin`], and
        // whatever it owns sits inside that stroke.
        FieldType::Text | FieldType::Integer { .. } => CONTROL_FRAME,
        // These paint into an unstyled container — a checkbox and its word, a
        // row of option pills, a column of self-skinned item boxes. There is no
        // outline of their own for a part to stand on.
        FieldType::Boolean
        | FieldType::Choice { .. }
        | FieldType::Flags { .. }
        | FieldType::List { .. } => 0,
    }
}

/// `rect` less the outline drawn inside it — its content box.
const fn inset_by(rect: Rect, frame: u32) -> Rect {
    Rect::new(
        rect.x + frame,
        rect.y + frame,
        rect.w.saturating_sub(frame * 2),
        rect.h.saturating_sub(frame * 2),
    )
}

/// A control's layout, with the frame's own pixels reserved.
///
/// One place rather than three: the three call sites below draw the same skin,
/// and a padding each of them remembers separately is a padding one of them
/// forgets — which is what the measurement found.
fn framed(base: LayoutStyle) -> LayoutStyle {
    let pad = base.padding;
    base.with_padding(Rect::new(
        pad.x + CONTROL_FRAME,
        pad.y + CONTROL_FRAME,
        pad.w + CONTROL_FRAME,
        pad.h + CONTROL_FRAME,
    ))
}

fn control_skin(worst: Option<&ConfigDefect>, theme: &Theme) -> BoxStyle {
    BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
        .with_corner_radius(8)
        .with_border(Border::new(
            if worst.is_some_and(ConfigDefect::blocks) {
                theme.resolve(ColorRole::Error)
            } else {
                theme.resolve(ColorRole::Outline)
            },
            // The constant, not a literal: [`control_frame`] answers with this
            // width and the two must be the same number by construction.
            CONTROL_FRAME,
        ))
}

fn part_seat(row: &RowBox, suffix: &str) -> Rect {
    row.part(suffix).unwrap_or(row.control)
}

/// A pill carrying one word, placed at a published part rectangle.
fn part_pill(
    tag: String,
    label: &str,
    ink: Color,
    seat: Rect,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            label.to_owned(),
            Rect::default(),
            form_run_style().with_size_px(10).with_fg(ink),
        ))])
        .with_tag(tag)
        .with_style(
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
                .with_corner_radius(6)
                .with_border(Border::new(ink, 1)),
        )
        .with_layout(placed(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center),
            seat,
            origin,
        )),
    )
}

/// Every option, with the chosen ones marked.
fn option_chips(
    tag_prefix: &str,
    row: &RowBox,
    chosen: &[&str],
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    let chips: Vec<Scene> = row
        .parts
        .iter()
        .filter_map(|(suffix, seat)| {
            let word = suffix.rsplit('.').next()?;
            let on = chosen.contains(&word);
            let ink = if on {
                theme.resolve(ColorRole::Accent)
            } else {
                theme.resolve(ColorRole::OnSurfaceMuted)
            };
            // Relative to the CONTROL, which is this chip's parent — an
            // absolutely-placed child is positioned against its own container.
            Some(part_pill(
                format!("{tag_prefix}.{suffix}"),
                word,
                ink,
                *seat,
                (row.control.x, row.control.y),
                theme,
            ))
        })
        .collect();
    Scene::Container(
        ContainerNode::new(chips)
            .with_tag(format!("{tag_prefix}.control.{}", row.key))
            .with_layout(placed(
                LayoutStyle::new().with_focusable(true),
                row.control,
                origin,
            )),
    )
}

/// A boolean row: a **checkbox**, not a box somebody types `true` into.
fn boolean_control(
    tag_prefix: &str,
    row: &RowBox,
    field: &ConfigField,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    let on = field.value().trim() == "true";
    let seat = part_seat(row, &format!("toggle.{}", row.key));
    let ink = if on {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    Scene::Container(
        ContainerNode::new(vec![
            part_pill(
                format!("{tag_prefix}.toggle.{}", row.key),
                if on { "\u{2713}" } else { " " },
                ink,
                seat,
                (row.control.x, row.control.y),
                theme,
            ),
            Scene::Text(TextNode::styled(
                if on { "true" } else { "false" }.to_owned(),
                Rect::new(seat.w + 10, 8, 80, 14),
                form_run_style()
                    .with_size_px(12)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            )),
        ])
        .with_tag(format!("{tag_prefix}.control.{}", row.key))
        .with_layout(placed(
            LayoutStyle::new().with_focusable(true),
            row.control,
            origin,
        )),
    )
}

/// A numeric row: the value, and a **stepper pair** that knows the bounds.
///
/// The bounds are on the field ([`FieldType::Integer`]), so the buttons clamp
/// rather than letting a person walk out of range and be told afterwards.
fn number_control(
    tag_prefix: &str,
    row: &RowBox,
    field: &ConfigField,
    worst: Option<&ConfigDefect>,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let mut children = vec![Scene::Text(TextNode::styled(
        field.value().to_owned(),
        Rect::new(10, 8, row.control.w.saturating_sub(STEP_W * 2 + 16), 14),
        form_run_style()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ))];
    for (suffix, glyph) in [("down", "-"), ("up", "+")] {
        let name = format!("step.{}.{suffix}", row.key);
        children.push(part_pill(
            format!("{tag_prefix}.{name}"),
            glyph,
            muted,
            part_seat(row, &name),
            (row.control.x, row.control.y),
            theme,
        ));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{tag_prefix}.control.{}", row.key))
            .with_style(control_skin(worst, theme))
            .with_layout(placed(
                framed(LayoutStyle::new().with_focusable(true)),
                row.control,
                origin,
            )),
    )
}

/// A list row: **one row per element**, then the row that adds one.
///
/// A list's text is comma-separated because that is the document's spelling;
/// a person editing it should not have to count commas.
fn list_control(
    tag_prefix: &str,
    row: &RowBox,
    field: &ConfigField,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let elements: Vec<&str> = FieldType::elements(field.value()).collect();
    let mut children = Vec::new();
    for (n, element) in elements.iter().enumerate() {
        let name = format!("item.{}.{n}", row.key);
        let seat = part_seat(row, &name);
        children.push(Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                (*element).to_owned(),
                Rect::new(10, 8, seat.w.saturating_sub(20), 14),
                form_run_style()
                    .with_size_px(12)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            ))])
            .with_tag(format!("{tag_prefix}.{name}"))
            .with_style(control_skin(None, theme))
            .with_layout(placed(
                framed(LayoutStyle::new().with_focusable(true)),
                seat,
                (row.control.x, row.control.y),
            )),
        ));
    }
    let add = format!("item.{}.add", row.key);
    children.push(part_pill(
        format!("{tag_prefix}.{add}"),
        "+ one more",
        muted,
        part_seat(row, &add),
        (row.control.x, row.control.y),
        theme,
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{tag_prefix}.control.{}", row.key))
            .with_layout(placed(
                LayoutStyle::new().with_focusable(true),
                row.control,
                origin,
            )),
    )
}

/// A free-text row: the box, and nothing else — the control IS the box.
fn text_control(
    tag_prefix: &str,
    row: &RowBox,
    field: &ConfigField,
    worst: Option<&ConfigDefect>,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            field.value().to_owned(),
            Rect::default(),
            form_run_style()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(format!("{tag_prefix}.control.{}", row.key))
        .with_style(control_skin(worst, theme))
        .with_layout(placed(
            framed(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(10, 0, 10, 0))
                    .with_focusable(true),
            ),
            row.control,
            origin,
        )),
    )
}

/// The chip that adds an offered key.
fn view_add_chip(
    tag_prefix: &str,
    key: &str,
    rect: Rect,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    {
        Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                format!("+ {key}"),
                Rect::default(),
                form_run_style()
                    .with_size_px(10)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            ))])
            .with_tag(format!("{tag_prefix}.add.{key}"))
            .with_style(
                BoxStyle::filled(Color::rgba(0, 0, 0, 0))
                    .with_corner_radius(6)
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
            )
            .with_layout(placed(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_focusable(true),
                rect,
                origin,
            )),
        )
    }
}

/// The access nodes for a form's rows: one control per shown row, each with the
/// **status region** that says what its badges say.
///
/// Derived from the same geometry the paint came from, so a control cannot be
/// on screen without a name. The name is the configuration path — the thing the
/// row is actually about.
///
/// The description goes through [`describedby_region`] rather than into the
/// name, and that is this crate's 4th consumer of that substrate (after the two
/// tooltips and the badge): "this needs a restart" and "this value is outside
/// 0..=65535" are *about* the control rather than what it is called, and a name
/// that grew a sentence would be read out on every focus move.
///
/// Measured on the mature toolkit at 6.11: its form layout does **not** set the
/// label→control relation when the application owns the label, so a form
/// assembled the way an application assembles one has controls with no
/// accessible name at all. Here the relation is derived, so it cannot be
/// forgotten per row.
#[must_use]
pub fn row_access_nodes(
    tag_prefix: &str,
    form: &ConfigForm,
    geometry: &FormGeometry,
) -> Vec<AccessNode> {
    let defects = form.defects();
    let mut nodes = Vec::new();
    for row in &geometry.rows {
        let Some(field) = form.field(&row.key) else {
            continue;
        };
        let mut said = vec![format!("{}, {}", field.ty(), field.applies().wire())];
        for defect in defects.iter().filter(|d| d.key() == row.key) {
            said.push(defect.sentence());
        }
        let control = AccessNode::new(
            format!("{tag_prefix}.control.{}", row.key),
            AriaRole::TextInput,
        )
        .with_name(field.key())
        .with_bounds(row.control)
        .with_value(AccessValue::Text(field.value().to_owned()));
        nodes.extend(describedby_region(
            control,
            format!("{tag_prefix}.said.{}", row.key),
            AriaRole::Status,
            Some(said.join("; ")),
            true,
        ));
    }
    nodes
}

/// What the status region for `key` says, for a caller checking a claim about
/// what a reader is told rather than about what is on screen.
#[must_use]
pub fn row_description(nodes: &[AccessNode], tag_prefix: &str, key: &str) -> Option<String> {
    let tag = format!("{tag_prefix}.said.{key}");
    nodes
        .iter()
        .find(|n| n.tag == tag)
        .and_then(|n| n.name.clone())
}

#[cfg(test)]
mod tests {

    /// ★ R1662 — one piece of arithmetic relates the pane frame to the window
    /// frame, so a press and a paint cannot read two facts.
    #[test]
    fn r1662_a_translated_geometry_moves_every_rectangle_by_the_same_shift() {
        let form = ConfigForm::new(
            vec![ConfigField::new("link.tx", "int", Applies::Hot, "8")],
            vec![ConfigField::new("link.rx", "int", Applies::Hot, "9")],
        );
        let local = form_geometry(&form, (14, 40), &FormStyle::default());
        let moved = local.translated(300, -10);
        assert_eq!(moved.origin, (314, 30));
        let a = local.row("link.tx").expect("shown");
        let b = moved.row("link.tx").expect("still shown");
        assert_eq!(b.control.x, a.control.x + 300);
        assert_eq!(b.control.y, a.control.y - 10);
        assert_eq!(b.control.w, a.control.w, "a shift is not a resize");
        assert_eq!(b.parts.len(), a.parts.len());
        assert_eq!(moved.chips.len(), local.chips.len());
        assert_eq!(moved.height, local.height, "height is not a coordinate");
    }

    /// ★ What a scroll carries off the top is DROPPED, not clamped to the edge.
    ///
    /// Clamping would answer a press on whatever is really at the edge with the
    /// key of a row that is not there, and would have a screen reader announce
    /// an off-screen row as visible.
    #[test]
    fn r1662_a_row_scrolled_past_the_origin_is_dropped_not_clamped() {
        let form = ConfigForm::new(
            vec![
                ConfigField::new("link.tx", "int", Applies::Hot, "8"),
                ConfigField::new("link.rx", "int", Applies::Hot, "9"),
            ],
            Vec::new(),
        );
        let local = form_geometry(&form, (0, 0), &FormStyle::default());
        let second = local.row("link.rx").expect("shown").row.y;
        assert!(second > 0, "the fixture needs two rows apart");
        let moved = local.translated(0, -i32::try_from(second).expect("small"));
        assert!(moved.row("link.tx").is_none(), "carried off the top");
        assert_eq!(moved.row("link.rx").map(|r| r.row.y), Some(0));
    }

    /// ★ R1656 — a header's parts fit the header, however long the key is.
    ///
    /// The badges are read-outs and the key gives way, so a long configuration
    /// path elides instead of pushing them off the row. Asserted HERE and not
    /// only in a consumer's sweep for the reason R1655 established: a
    /// consumer's suite finds one screen at a time, and this painter is used by
    /// every screen that shows a settings form. Run through the real layout
    /// pass, because "who absorbs the deficit" is a question only the flex
    /// solver answers — the round's first repair computed a width budget by
    /// measuring the badge strings and was still 7px short.
    #[test]
    fn r1656_a_long_key_gives_way_instead_of_pushing_the_badges_out() {
        use pinion_runtime::layout::compute_layout;
        let theme = pinion_core::theme::Theme::default();
        let style = FormStyle::default();
        let field = ConfigField::new(
            "transport.link.tx.batch_size.and.then.some.more.segments",
            "int",
            Applies::Restart,
            "65535",
        );
        let form = ConfigForm::new(vec![field], Vec::new());
        let geometry = form_geometry(&form, (0, 0), &style);
        let mut scene = view_config_form("f", &form, &geometry, &theme);
        let mut cache = pinion_text::LayoutCache::new();
        compute_layout(&mut scene, &mut cache, 400, 400);
        let escapes = pinion_core::containment::escapes(&scene, &mut |t| {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a configuration path is a handful of characters"
            )]
            let chars = t.content.chars().count() as u32;
            let px = t.style.font_size_px.max(1);
            let w = if t.style.overflow.shortens() {
                t.rect.w.min(chars * px)
            } else {
                chars * px
            };
            (w, t.rect.h)
        });
        assert!(
            escapes.is_empty(),
            "{} mark(s) left the box that owns them with a long key: {:?}",
            escapes.len(),
            escapes
                .iter()
                .map(|e| (e.content.clone().or_else(|| e.tag.clone()), e.over))
                .collect::<Vec<_>>()
        );
    }

    /// ★ R1656 — a row's key line is at least the LINE box of the face it
    /// holds, at every size a caller can set.
    ///
    /// Written because a counterfactual that put it back to `key_px + 7` — one
    /// pixel short, which is what shipped — was caught by nothing in Rust. The
    /// escape it causes is real and `scene/containment` reports it at boot, but
    /// a property this crate owns should not depend on a consumer booting to be
    /// checked, and a gate that lives only in another language is a gate that
    /// does not run when this crate is edited.
    #[test]
    fn r1656_a_rows_key_line_holds_the_face_it_is_given() {
        for key_px in 6..=24u32 {
            let style = FormStyle {
                key_px,
                ..FormStyle::default()
            };
            let field = ConfigField::new("listen.endpoints", "text", Applies::Restart, "tcp/0:1");
            let form = ConfigForm::new(vec![field], Vec::new());
            let row = &form_geometry(&form, (0, 0), &style).rows[0];
            let want = pinion_core::containment::line_box(key_px);
            assert!(
                row.header.h >= want,
                "a {key_px}px face needs a {want}px line box and the header \
                 reserved {}px — a box authored at the FONT SIZE overflows by \
                 construction, which is the whole reason the ink read exists",
                row.header.h
            );
        }
    }
    use pinion_core::widgets::config_form::{Applies, ConfigField, ConfigForm, FieldType};

    use pinion_core::Scene;

    use super::{
        CONTROL_FRAME, FieldGrowth, FormStyle, RowWrap, control_frame, form_geometry, inset_by,
        row_access_nodes, row_description, view_config_form,
    };

    fn inspector() -> ConfigForm {
        ConfigForm::new(
            vec![
                ConfigField::new("id", "text", Applies::Restart, "a1"),
                ConfigField::new(
                    "control.permissions",
                    "perm",
                    Applies::Restart,
                    "read, write",
                )
                .with_shape(FieldType::Flags {
                    of: vec!["read".into(), "write".into()],
                }),
                ConfigField::new(
                    "transport.link.tx.batch_size",
                    "int",
                    Applies::Restart,
                    "65535",
                )
                .with_shape(FieldType::Integer { min: 0, max: 65535 }),
            ],
            vec![ConfigField::new(
                "routing",
                "mode",
                Applies::Restart,
                "peer",
            )],
        )
    }

    #[test]
    fn r1651_wrap_all_puts_every_control_below_its_key_and_beside_puts_none() {
        let form = inspector();
        let below = form_geometry(
            &form,
            (0, 0),
            &FormStyle::default().with_policy(RowWrap::WrapAll, FieldGrowth::AllGrow),
        );
        assert!(
            below
                .rows
                .iter()
                .all(|r| r.wrapped && r.control.y > r.header.y),
            "every control below its key"
        );

        let beside = form_geometry(
            &form,
            (0, 0),
            &FormStyle::default().with_policy(RowWrap::Beside, FieldGrowth::AllGrow),
        );
        assert!(
            beside
                .rows
                .iter()
                .all(|r| !r.wrapped && r.control.y == r.header.y && r.control.x > r.header.x),
            "every control beside its key"
        );
        assert!(
            beside.height < below.height,
            "and the beside form is shorter: {} vs {}",
            beside.height,
            below.height
        );
    }

    #[test]
    fn r1651_wrap_long_is_derived_per_row_from_the_key_it_holds() {
        // ★ The property that makes this a policy rather than a per-row flag,
        // and the one the reference toolkit's own measurement shows: in a wide
        // box every row stays beside its key, and in a narrow one only the row
        // whose key does not fit moves.
        let form = inspector();
        let wide = form_geometry(
            &form,
            (0, 0),
            &FormStyle::default()
                .with_width(600)
                .with_policy(RowWrap::WrapLong, FieldGrowth::AllGrow),
        );
        assert!(
            wide.rows.iter().all(|r| !r.wrapped),
            "a wide pane wraps nothing"
        );

        let narrow = form_geometry(
            &form,
            (0, 0),
            &FormStyle::default()
                .with_width(160)
                .with_policy(RowWrap::WrapLong, FieldGrowth::AllGrow),
        );
        let moved: Vec<&str> = narrow
            .rows
            .iter()
            .filter(|r| r.wrapped)
            .map(|r| r.key.as_str())
            .collect();
        assert_eq!(
            moved,
            vec!["control.permissions", "transport.link.tx.batch_size"],
            "only the rows whose key and control do not fit together"
        );
        assert!(
            !narrow.row("id").expect("shown").wrapped,
            "and the short key keeps its control beside it"
        );
    }

    #[test]
    fn r1651_growth_tells_a_hungry_control_from_one_that_hugs_its_options() {
        // The distinction the middle policy exists to make: stretching a row of
        // option chips puts their borders in the wrong places, and stranding a
        // text box wastes the pane.
        let form = inspector();
        let style = FormStyle::default().with_width(284);
        let hint = form_geometry(
            &form,
            (0, 0),
            &style.with_policy(RowWrap::WrapAll, FieldGrowth::AtSizeHint),
        );
        let expanding = form_geometry(
            &form,
            (0, 0),
            &style.with_policy(RowWrap::WrapAll, FieldGrowth::ExpandingGrow),
        );
        let all = form_geometry(
            &form,
            (0, 0),
            &style.with_policy(RowWrap::WrapAll, FieldGrowth::AllGrow),
        );

        let text_at = |g: &super::FormGeometry| g.row("id").expect("shown").control.w;
        let chips_at =
            |g: &super::FormGeometry| g.row("control.permissions").expect("shown").control.w;

        assert!(
            text_at(&hint) < text_at(&expanding),
            "a text box grows when the policy lets it"
        );
        assert_eq!(
            text_at(&expanding),
            text_at(&all),
            "and it is hungry under both growing policies"
        );
        assert_eq!(
            chips_at(&hint),
            chips_at(&expanding),
            "★ a chip row hugs its options under ExpandingGrow"
        );
        assert!(
            chips_at(&all) > chips_at(&expanding),
            "and only AllGrow stretches it: {} vs {}",
            chips_at(&all),
            chips_at(&expanding)
        );
    }

    #[test]
    fn r1651_a_hidden_row_takes_no_space_and_the_form_gets_shorter() {
        let shown = inspector();
        let mut fields: Vec<ConfigField> = shown.fields().to_vec();
        fields[1] = fields[1].clone().with_hidden(true);
        let hiding = ConfigForm::new(fields, shown.addable().to_vec());

        let a = form_geometry(&shown, (0, 0), &FormStyle::default());
        let b = form_geometry(&hiding, (0, 0), &FormStyle::default());
        assert_eq!(a.rows.len(), 3);
        assert_eq!(b.rows.len(), 2, "the hidden row is not laid out");
        assert_eq!(
            hiding.fields().len(),
            3,
            "and it is still a row of the form"
        );
        assert!(b.height < a.height, "{} vs {}", b.height, a.height);
    }

    #[test]
    fn r1651_the_paint_and_the_access_tree_read_one_geometry() {
        // The property R1648 lost by hand-placing children: a second copy of
        // the layout cannot notice a drift from the first.
        let form = inspector();
        let geometry = form_geometry(&form, (14, 40), &FormStyle::default());
        let nodes = row_access_nodes("insp", &form, &geometry);
        for row in &geometry.rows {
            let control = nodes
                .iter()
                .find(|n| n.tag == format!("insp.control.{}", row.key))
                .unwrap_or_else(|| panic!("{} has an access node", row.key));
            assert_eq!(control.bounds, Some(row.control), "{}", row.key);
            assert_eq!(
                control.name.as_deref(),
                Some(row.key.as_str()),
                "the name IS the configuration path"
            );
            assert!(
                row_description(&nodes, "insp", &row.key).is_some(),
                "and every row has a status region: {}",
                row.key
            );
        }
    }

    #[test]
    fn r1651_a_defective_row_says_so_on_the_row_and_the_reader_is_told_why() {
        let mut form = inspector();
        form.set("transport.link.tx.batch_size", "70000")
            .expect("held");
        let geometry = form_geometry(&form, (0, 0), &FormStyle::default());
        let painted = view_config_form("insp", &form, &geometry, &pinion_core::Theme::dark());
        let mut tags = Vec::new();
        painted.for_each_node(&mut |node| {
            if let Some(tag) = node.node.tag() {
                tags.push(tag.to_string());
            }
        });
        assert!(
            tags.contains(&"insp.defect.transport.link.tx.batch_size".to_string()),
            "the defect is painted on the row it is about: {tags:?}"
        );
        assert!(
            !tags.iter().any(|t| t.starts_with("insp.defect.id")),
            "and only on that row"
        );

        let nodes = row_access_nodes("insp", &form, &geometry);
        let said = row_description(&nodes, "insp", "transport.link.tx.batch_size")
            .expect("a status region");
        assert!(
            said.contains("0..=65535"),
            "★ a reader who cannot see the badge is told what is wrong: {said}"
        );
        assert!(
            said.contains("restart"),
            "and whether editing it reaches a running program: {said}"
        );
    }

    #[test]
    fn r1651_every_control_and_every_offered_key_is_addressable_by_one_rule() {
        let form = inspector();
        let geometry = form_geometry(&form, (0, 0), &FormStyle::default());
        let painted = view_config_form("insp", &form, &geometry, &pinion_core::Theme::dark());
        let mut tags = Vec::new();
        painted.for_each_node(&mut |node| {
            if let Some(tag) = node.node.tag() {
                tags.push(tag.to_string());
            }
        });
        for row in &geometry.rows {
            assert!(
                tags.contains(&format!("insp.control.{}", row.key)),
                "{} has a control: {tags:?}",
                row.key
            );
            assert!(
                tags.contains(&format!("insp.applies.{}", row.key)),
                "{} says whether it is live: {tags:?}",
                row.key
            );
        }
        for (key, _) in &geometry.chips {
            assert!(
                tags.contains(&format!("insp.add.{key}")),
                "{key} is offered"
            );
        }
    }

    #[test]
    fn r1652_every_declared_shape_gets_a_control_of_its_own() {
        // ★ R1651 declared six shapes and drew two: a boolean was a box
        // somebody typed `true` into and an integer knew its range with no way
        // to step inside it. The model's precision was invisible on screen.
        // This asserts the OTHER direction of that — one arm per shape, and
        // each shape's own affordance painted.
        use pinion_core::Theme;

        let form = ConfigForm::new(
            vec![
                ConfigField::new("free", "text", Applies::Hot, "anything"),
                ConfigField::new("count", "int", Applies::Hot, "3")
                    .with_shape(FieldType::Integer { min: 0, max: 9 }),
                ConfigField::new("on", "bool", Applies::Hot, "true").with_shape(FieldType::Boolean),
                ConfigField::new("mode", "mode", Applies::Hot, "b").with_shape(FieldType::Choice {
                    of: vec!["a".into(), "b".into()],
                }),
                ConfigField::new("perm", "perm", Applies::Hot, "read").with_shape(
                    FieldType::Flags {
                        of: vec!["read".into(), "write".into()],
                    },
                ),
                ConfigField::new("hosts", "name[]", Applies::Hot, "one, two").with_shape(
                    FieldType::List {
                        of: Box::new(FieldType::Text),
                    },
                ),
            ],
            vec![],
        );
        let geometry = form_geometry(&form, (0, 0), &FormStyle::default());
        let painted = view_config_form("f", &form, &geometry, &Theme::dark());
        let mut tags = Vec::new();
        painted.for_each_node(&mut |node| {
            if let Some(tag) = node.node.tag() {
                tags.push(tag.to_string());
            }
        });
        let has = |t: &str| tags.iter().any(|x| x == t);

        assert!(has("f.control.free"), "text: the control IS the box");
        assert_eq!(
            geometry.row("free").expect("shown").parts.len(),
            0,
            "and it has no parts inside it"
        );
        assert!(
            has("f.step.count.up") && has("f.step.count.down"),
            "{tags:?}"
        );
        assert!(has("f.toggle.on"), "a boolean is a checkbox: {tags:?}");
        assert!(has("f.option.mode.a") && has("f.option.mode.b"), "{tags:?}");
        assert!(
            has("f.option.perm.read") && has("f.option.perm.write"),
            "{tags:?}"
        );
        assert!(
            has("f.item.hosts.0") && has("f.item.hosts.1") && has("f.item.hosts.add"),
            "a list is one row per element, then the row that adds one: {tags:?}"
        );

        // And the census direction: every shape the vocabulary declares is
        // reached, so a seventh arm cannot arrive drawn as a text box.
        assert_eq!(FieldType::ARMS, 6);
        let with_parts = geometry.rows.iter().filter(|r| !r.parts.is_empty()).count();
        assert_eq!(with_parts, 5, "five of six shapes carry affordances");
    }

    #[test]
    fn r1652_1_a_grown_list_stays_inside_its_own_row() {
        // ★ R1652 sized every control with one token, so a list of six painted
        // its rows over the NEXT field. Found by growing the list on a running
        // screen and reading the rectangles back — not by reasoning, and not by
        // the sweep, which only ever probed the opening state where a list has
        // one element and cannot overflow.
        //
        // The assertion is over a RANGE of lengths, because a defect whose
        // trigger is "enough elements" is invisible to any single fixture.
        for count in 1..=8_usize {
            let value = (0..count)
                .map(|n| format!("t/0.0:{n}"))
                .collect::<Vec<_>>()
                .join(FieldType::SEPARATOR);
            let form = ConfigForm::new(
                vec![
                    ConfigField::new("hosts", "name[]", Applies::Hot, value).with_shape(
                        FieldType::List {
                            of: Box::new(FieldType::Text),
                        },
                    ),
                    ConfigField::new("after", "text", Applies::Hot, "next"),
                ],
                vec![],
            );
            let geometry = form_geometry(&form, (0, 0), &FormStyle::default());
            let list = geometry.row("hosts").expect("shown");
            let after = geometry.row("after").expect("shown");

            for (name, seat) in &list.parts {
                assert!(
                    seat.y + seat.h <= list.control.y + list.control.h,
                    "{count} element(s): {name} at {seat:?} leaves its control \
                     {:?}",
                    list.control
                );
                assert!(
                    seat.y + seat.h <= after.row.y,
                    "{count} element(s): {name} at {seat:?} is drawn over the \
                     next field at {:?}",
                    after.row
                );
            }
            assert!(
                list.row.y + list.row.h <= after.row.y,
                "{count} element(s): the rows do not overlap"
            );
        }

        // And the property that makes the fix a fix rather than a bigger
        // constant: the row's height TRACKS the element count. A control sized
        // by a token could pass every assertion above at some fixed length and
        // fail at the next one, which is exactly what R1652 did.
        let heights: Vec<u32> = (1..=4_usize)
            .map(|count| {
                let value = (0..count)
                    .map(|n| format!("t/0.0:{n}"))
                    .collect::<Vec<_>>()
                    .join(FieldType::SEPARATOR);
                let form = ConfigForm::new(
                    vec![
                        ConfigField::new("hosts", "name[]", Applies::Hot, value).with_shape(
                            FieldType::List {
                                of: Box::new(FieldType::Text),
                            },
                        ),
                    ],
                    vec![],
                );
                form_geometry(&form, (0, 0), &FormStyle::default())
                    .row("hosts")
                    .expect("shown")
                    .row
                    .h
            })
            .collect();
        assert!(
            heights.windows(2).all(|w| w[1] > w[0]),
            "each element adds height: {heights:?}"
        );
    }

    #[test]
    fn r1652_a_lists_rows_follow_the_elements_the_document_would_hold() {
        // Two spellings of "what the commas mean" is how a screen shows four
        // rows for a value the document reads as three.
        use pinion_core::Theme;
        let form = ConfigForm::new(
            vec![
                ConfigField::new("hosts", "name[]", Applies::Hot, "a, b,, c ").with_shape(
                    FieldType::List {
                        of: Box::new(FieldType::Text),
                    },
                ),
            ],
            vec![],
        );
        let geometry = form_geometry(&form, (0, 0), &FormStyle::default());
        let rows = geometry.row("hosts").expect("shown");
        let items = rows
            .parts
            .iter()
            .filter(|(n, _)| n.starts_with("item.") && n.rsplit('.').next() != Some("add"))
            .count();
        assert_eq!(
            items,
            3,
            "the empty run between two commas is not an element, and the \
             document agrees: {:?}",
            form.document().expect("sound")["hosts"]
        );
        assert_eq!(
            form.document().expect("sound")["hosts"]
                .as_array()
                .expect("a list")
                .len(),
            items,
            "★ one splitter, so the screen and the document cannot disagree"
        );
        let _ = view_config_form("f", &form, &geometry, &Theme::dark());
    }

    /// The pinned measurement of the floor — see `docs/reference-form-floor.json`.
    fn floor() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/reference-form-floor.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("the floor is pinned"))
            .expect("the pin parses")
    }

    #[test]
    fn r1652_this_form_reproduces_every_behaviour_the_pinned_floor_measured() {
        // ★ R1651 measured the floor and wrote the numbers into a doc comment,
        // and R1651.1's audit registered that as debt: the probe is outside the
        // repository by directive, so a fresh clone could not re-derive them and
        // nothing would notice them going stale. The measurement is now a file,
        // and this is what reads it.
        //
        // It gates RELATIONS, not pixels: our tokens are our own (a 31px control
        // where the floor's is 25) and copying its geometry would be cargo-cult.
        // What must hold is every behaviour it demonstrated.
        let floor = floor();
        let form = inspector();
        let style = FormStyle::default().with_width(284);

        // 1. Beside vs wrapped: wrapping makes the form taller.
        let beside = form_geometry(
            &form,
            (0, 0),
            &style.with_policy(RowWrap::Beside, FieldGrowth::AllGrow),
        );
        let wrapped = form_geometry(
            &form,
            (0, 0),
            &style.with_policy(RowWrap::WrapAll, FieldGrowth::AllGrow),
        );
        let taller = floor["wrapped"]["grown"]["height"]
            .as_u64()
            .expect("pinned")
            > floor["beside"]["grown"]["height"].as_u64().expect("pinned");
        assert!(taller, "the floor's wrapped form is the taller one");
        assert_eq!(wrapped.height > beside.height, taller, "and so is ours");

        // 2. Growth widens a control, beside AND wrapped, exactly where the
        //    floor's does.
        //
        //    ★ The fixture is 600 px wide and the first draft was 284. Beside a
        //    key column sized for `transport.link.tx.batch_size`, a 284 px pane
        //    has LESS room left than a control's natural width, so both growth
        //    policies hand back the same number and the fixture cannot tell
        //    them apart — it reported a failure against the floor and the code
        //    was right. A fixture that cannot discriminate is not a test
        //    (R1633's lesson, and the floor's own fixture is 320 px against a
        //    91 px key for the same reason).
        let roomy = style.with_width(600);
        for (policy, arm) in [(RowWrap::Beside, "beside"), (RowWrap::WrapAll, "wrapped")] {
            let hint = form_geometry(
                &form,
                (0, 0),
                &roomy.with_policy(policy, FieldGrowth::AtSizeHint),
            );
            let grown = form_geometry(
                &form,
                (0, 0),
                &roomy.with_policy(policy, FieldGrowth::AllGrow),
            );
            let floor_grows = floor[arm]["grown"]["field_w"].as_u64().expect("pinned")
                > floor[arm]["at_hint"]["field_w"].as_u64().expect("pinned");
            let we_grow = grown.row("id").expect("shown").control.w
                > hint.row("id").expect("shown").control.w;
            assert_eq!(we_grow, floor_grows, "{arm}: growth widens the control");
        }

        // And the narrow case is a stated no-op rather than an unexamined one:
        // with no room left beside the key, both policies hand back what is
        // left, which is the correct answer and not the absence of one.
        let cramped_hint = form_geometry(
            &form,
            (0, 0),
            &style.with_policy(RowWrap::Beside, FieldGrowth::AtSizeHint),
        );
        let cramped_grown = form_geometry(
            &form,
            (0, 0),
            &style.with_policy(RowWrap::Beside, FieldGrowth::AllGrow),
        );
        assert_eq!(
            cramped_hint.row("id").expect("shown").control.w,
            cramped_grown.row("id").expect("shown").control.w,
            "a control with no room to grow into does not grow"
        );

        floor_wrap_long_and_hidden_rows(&floor, &form, style);
    }

    /// The third and fourth behaviours the pin measured, so the parity test
    /// stays under the length the lint asks for without either half losing the
    /// pin it is checked against.
    fn floor_wrap_long_and_hidden_rows(
        floor: &serde_json::Value,
        form: &ConfigForm,
        style: FormStyle,
    ) {
        // 3. Beside-unless-it-does-not-fit wraps NOTHING in a wide box and
        //    SOMETHING in a narrow one — the property that makes it a derived
        //    policy rather than a per-row flag.
        let floor_wide = floor["wrap_when_it_does_not_fit"]["wide_box"]["wrapped_rows"]
            .as_array()
            .expect("pinned")
            .len();
        let floor_narrow = floor["wrap_when_it_does_not_fit"]["narrow_box"]["wrapped_rows"]
            .as_array()
            .expect("pinned")
            .len();
        assert_eq!(floor_wide, 0, "the floor wraps nothing when it fits");
        assert!(floor_narrow > 0, "and something when it does not");
        let wide = form_geometry(
            form,
            (0, 0),
            &style
                .with_width(600)
                .with_policy(RowWrap::WrapLong, FieldGrowth::AllGrow),
        );
        let narrow = form_geometry(
            form,
            (0, 0),
            &style
                .with_width(160)
                .with_policy(RowWrap::WrapLong, FieldGrowth::AllGrow),
        );
        assert_eq!(
            wide.rows.iter().filter(|r| r.wrapped).count(),
            floor_wide,
            "and so do we, in a box that fits"
        );
        assert!(
            narrow.rows.iter().filter(|r| r.wrapped).count() > 0,
            "and in one that does not"
        );
        assert!(
            floor["wrap_when_it_does_not_fit"]["narrow_box"]["key_widths"]
                .as_array()
                .expect("pinned")
                .windows(2)
                .all(|w| w[0].as_u64() <= w[1].as_u64()),
            "the floor's fixture has the long key last, which is why row 2 is \
             the one it moved — a fixture whose keys were all one width could \
             not tell a derived policy from a flag"
        );

        // 4. A hidden row shortens the form and does NOT change the row count.
        assert!(
            floor["hidden_row"]["height_middle_hidden"].as_u64()
                < floor["hidden_row"]["height_all_shown"].as_u64(),
        );
        assert_eq!(floor["hidden_row"]["row_count_unchanged"], true);
        let mut fields: Vec<ConfigField> = form.fields().to_vec();
        fields[1] = fields[1].clone().with_hidden(true);
        let hiding = ConfigForm::new(fields, vec![]);
        let shown = form_geometry(form, (0, 0), &FormStyle::default());
        let hidden = form_geometry(&hiding, (0, 0), &FormStyle::default());
        assert!(hidden.height < shown.height);
        assert_eq!(hiding.fields().len(), form.fields().len());
    }

    #[test]
    fn r1652_the_pin_records_the_two_things_the_floor_does_not_do() {
        // The half that is not parity: what the floor CANNOT express, recorded
        // as measurements rather than as a claim, so a re-measurement that
        // changed either would be a finding rather than a surprise.
        let floor = floor();
        assert_eq!(
            floor["per_field_verdict"]["empty_and_out_of_range_agree"], true,
            "★ its per-field verdict gives ONE answer for 'not finished' and \
             'out of range' — which is why `ConfigDefect` is three arms and not \
             a validator"
        );
        assert_eq!(floor["per_field_verdict"]["in_range_differs"], true);
        assert_eq!(
            floor["declared_properties_over_the_five_form_classes"], 105,
            "the population the applies-scope absence is claimed over — an \
             absence proved by searching for a name you invented is worth zero"
        );
        assert_eq!(
            floor["label_names_the_control_automatically"], false,
            "★ and its label→control relation is NOT automatic, so a form \
             assembled the way an application assembles one has controls with \
             no accessible name; `row_access_nodes` derives ours"
        );

        // Ours, by contrast, is derived per row and cannot be forgotten.
        let form = inspector();
        let geometry = form_geometry(&form, (0, 0), &FormStyle::default());
        let nodes = row_access_nodes("insp", &form, &geometry);
        for row in &geometry.rows {
            assert!(
                nodes
                    .iter()
                    .any(|n| n.tag == format!("insp.control.{}", row.key)
                        && n.name.as_deref() == Some(row.key.as_str())),
                "{} is named for an assistive technology",
                row.key
            );
        }
    }

    #[test]
    fn r1651_the_policy_vocabularies_are_closed_and_fully_enumerated() {
        assert_eq!(RowWrap::ALL.len(), RowWrap::ARMS);
        assert_eq!(FieldGrowth::ALL.len(), FieldGrowth::ARMS);
    }

    /// ★ R1655 — every node this painter tags is transparent to the §5.35
    /// router.
    ///
    /// A tag here is an ADDRESS, not a widget: the router resolves the deepest
    /// tagged node under the cursor and looks that tag up as an `External`,
    /// finds none, and forwards NOTHING. So an opaque tagged node makes the
    /// form dead to a real mouse wherever it is painted, while every
    /// wire-driven assertion about the form keeps passing — R1649.1's class.
    ///
    /// It belongs in this crate rather than in a consumer: the badges are
    /// painted here, so every screen that uses this form inherits whichever
    /// answer this file gives. A consumer's own test could only find it one
    /// screen at a time, and one screen did — the analysis-tool node lab, from
    /// a person clicking on it.
    #[test]
    fn r1655_every_tag_this_painter_writes_is_pointer_transparent() {
        let form = inspector();
        let geometry = form_geometry(&form, (0, 0), &FormStyle::default());
        let scene = view_config_form("insp", &form, &geometry, &pinion_core::Theme::dark());
        let mut tagged = 0;
        let mut opaque = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let Some(tag) = visit.node.tag() {
                tagged += 1;
                if !visit.node.is_pointer_transparent() {
                    opaque.push(tag.to_owned());
                }
            }
        });
        assert!(tagged > 10, "the form tags plenty to check: {tagged}");
        assert!(
            opaque.is_empty(),
            "{} tagged node(s) would swallow a real press: {opaque:?}",
            opaque.len()
        );
    }

    /// The border a tagged node strokes inside its own box, or `None` if the
    /// walk never reached that tag.
    fn painted_frame(scene: &Scene, tag: &str) -> Option<u32> {
        let mut found = None;
        scene.for_each_node(&mut |visit| {
            if visit.node.tag() == Some(tag) {
                let style = match visit.node {
                    Scene::Container(n) => Some(&n.style),
                    Scene::Box(n) => Some(&n.style),
                    _ => None,
                };
                found =
                    Some(style.map_or(0, |s| s.border.as_ref().map_or(0, |border| border.width)));
            }
        });
        found
    }

    /// One field of every shape, and the shape census that keeps it total.
    ///
    /// A [`FieldType`] arm added later lands in the `match` below as a
    /// non-exhaustive pattern, so the population cannot silently stop covering
    /// the type it is a census of.
    fn one_of_every_shape() -> Vec<ConfigField> {
        let words = || vec!["one".into(), "two".into()];
        let fields = vec![
            ConfigField::new("a.text", "text", Applies::Hot, "x"),
            ConfigField::new("a.int", "int", Applies::Hot, "8")
                .with_shape(FieldType::Integer { min: 0, max: 99 }),
            ConfigField::new("a.bool", "bool", Applies::Hot, "true").with_shape(FieldType::Boolean),
            ConfigField::new("a.choice", "choice", Applies::Hot, "one")
                .with_shape(FieldType::Choice { of: words() }),
            ConfigField::new("a.flags", "flags", Applies::Hot, "one")
                .with_shape(FieldType::Flags { of: words() }),
            ConfigField::new("a.list", "list", Applies::Hot, "one, two").with_shape(
                FieldType::List {
                    of: Box::new(FieldType::Text),
                },
            ),
        ];
        let mut seen = [false; 6];
        for field in &fields {
            seen[match field.shape() {
                FieldType::Text => 0,
                FieldType::Integer { .. } => 1,
                FieldType::Boolean => 2,
                FieldType::Choice { .. } => 3,
                FieldType::Flags { .. } => 4,
                FieldType::List { .. } => 5,
            }] = true;
        }
        assert!(
            seen.iter().all(|hit| *hit),
            "the census must hold one field of every shape: {seen:?}"
        );
        fields
    }

    /// ★★ R1672 — the frame every shape DECLARES is the frame it PAINTS.
    ///
    /// [`control_frame`] is the inset [`lay_parts`] lays a shape's affordances
    /// within, and it is a judgment about what the painter does: two of the six
    /// shapes put their parts inside a container that strokes [`control_skin`]
    /// and the other four do not. A judgment nothing checks is a comment, and
    /// this one goes wrong silently — a shape that gains a skin would start
    /// painting its own parts over its outline with every test still green.
    ///
    /// So the assertion is a correspondence over the whole census: for each
    /// shape, the border width the scene actually carries at
    /// `<prefix>.control.<key>` equals what the constant says.
    #[test]
    fn r1672_every_shapes_control_frame_is_the_one_it_paints() {
        let theme = pinion_core::theme::Theme::default();
        let fields = one_of_every_shape();
        let form = ConfigForm::new(fields.clone(), Vec::new());
        let geometry = form_geometry(&form, (0, 0), &FormStyle::default());
        let scene = view_config_form("f", &form, &geometry, &theme);
        for field in &fields {
            let tag = format!("f.control.{}", field.key());
            let painted = painted_frame(&scene, &tag)
                .unwrap_or_else(|| panic!("{tag} is painted by this form"));
            assert_eq!(
                painted,
                control_frame(field.shape()),
                "{tag}: the declared frame and the painted one have to be one \
                 number, or a part laid inside the first sits on the second",
            );
        }
    }

    /// ★★ R1672 — a stepper stands inside its control's outline, not on it.
    ///
    /// The published part rectangles are laid against the control's CONTENT
    /// box. Before this round they were laid against its box, so the two
    /// stepper buttons of every integer row on every consumer of this widget
    /// covered the control's own frame along three edges — reported by
    /// `pinion_core::containment` the moment it learned the distinction, and
    /// invisible to everything before that.
    ///
    /// The counter-assertion is the load-bearing half: an integer control keeps
    /// a frame to clear, so a repair that simply stopped drawing the outline
    /// would satisfy the first assertion and fail this one.
    #[test]
    fn r1672_a_steppers_seat_clears_its_controls_outline() {
        let field = ConfigField::new("link.tx.batch_size", "int", Applies::Restart, "8")
            .with_shape(FieldType::Integer { min: 0, max: 65535 });
        let form = ConfigForm::new(vec![field], Vec::new());
        let geometry = form_geometry(&form, (14, 40), &FormStyle::default());
        let row = geometry.row("link.tx.batch_size").expect("shown");
        let frame = control_frame(&FieldType::Integer { min: 0, max: 65535 });
        assert_eq!(frame, CONTROL_FRAME, "an integer control draws a frame");
        let content = inset_by(row.control, frame);
        assert!(
            content.w < row.control.w && content.h < row.control.h,
            "the content box is smaller than the box: {content:?} vs {:?}",
            row.control
        );
        let mut steppers = 0;
        for (suffix, seat) in &row.parts {
            steppers += 1;
            assert!(
                seat.x >= content.x
                    && seat.y >= content.y
                    && seat.x + seat.w <= content.x + content.w
                    && seat.y + seat.h <= content.y + content.h,
                "{suffix} at {seat:?} stands on the control's outline; the \
                 content box is {content:?}",
            );
        }
        assert_eq!(steppers, 2, "an integer row publishes a stepper pair");
    }
}
