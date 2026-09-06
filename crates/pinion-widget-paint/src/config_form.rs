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

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, describedby_region};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, StateTone, Theme};
use pinion_core::voice::Silence;
use pinion_core::widgets::config_form::{
    Applies, ConfigDefect, ConfigField, ConfigForm, FieldType, Source,
};
use pinion_core::widgets::picker::Picker;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::{Scene, measured_text_extent};

use crate::badge::{BadgeTone, view_badge};
use crate::indicator::Indicator;
use crate::switch::SwitchStyle;

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
pub(crate) fn form_run_style() -> TextStyle {
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
    ///
    /// ★ R1686 — *inside the control* is load-bearing and not a turn of phrase.
    /// The remove seat was drafted into this list and three readers were
    /// already relying on the narrower meaning: the option painter turns every
    /// entry into a chip, and the crate's containment gate asserts every entry
    /// stands inside the control's content box. It has its own field instead.
    pub parts: Vec<(String, Rect)>,
    /// ★★ R1686 — where the seat at the row's trailing edge landed, and
    /// **which act it offers**.
    ///
    /// Cut out of the header's trailing edge, which is where the reference tool
    /// puts it and the only edge that is free under both wrap policies: under
    /// [`RowWrap::Beside`] the row's own trailing edge is inside the control.
    ///
    /// ★★★ R1716 — R1686 wrote here that a rule making some rows unremovable
    /// "would turn this into an `Option`, and the type change is what would
    /// make every consumer handle it". The rule arrived — a row whose value the
    /// screen works out is nobody's to take away — and the answer is stronger
    /// than an `Option`: the seat does not disappear, it offers the **other**
    /// act. Both are about who owns the row's value, so one seat with two arms
    /// says that, where a rectangle-or-nothing would have left the reader to
    /// discover the take-over somewhere else.
    pub seat: Seat,
}

/// The one seat at a row's trailing edge, and which act a press there performs.
///
/// ★★★★★ R1716 — **which arm this is says who owns the row's value.** The
/// floor has neither: measured at 6.11, its form layout has no per-row
/// removability predicate at all, and taking a derived value over is done by
/// assigning to it, which drops the derivation with no news and no way back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// Somebody wrote this row, so the seat takes it out of the form.
    Remove(Rect),
    /// The screen worked this row out, so the seat takes the value **over**:
    /// the row becomes theirs, holding what it was derived to.
    TakeOver(Rect),
    /// ★★★ R1717 — somebody wrote **part** of this row and the screen works
    /// the rest out, so the seat gives their part back: the row stays, holding
    /// what the screen alone says.
    ///
    /// A third act and not the first one, because the first one's word is a
    /// promise this row cannot keep. Taking a shared row out puts it back one
    /// render later — the derivation is still true — and a reader who pressed
    /// "remove" and watched the row return has been told the tool is broken.
    GiveBack(Rect),
}

impl Seat {
    /// Where it landed, whichever act it offers.
    #[must_use]
    pub const fn rect(self) -> Rect {
        match self {
            Self::Remove(rect) | Self::TakeOver(rect) | Self::GiveBack(rect) => rect,
        }
    }

    /// The word this seat's accessible name is built from.
    #[must_use]
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Remove(_) => "remove",
            Self::TakeOver(_) => "take over",
            Self::GiveBack(_) => "give back",
        }
    }

    /// The word this seat's **tag** is built from — the name a driver presses.
    ///
    /// ★★ R1717 — one place, because there are two doors onto it: the painter
    /// builds the tag it draws under and the accessibility tree builds the tag
    /// it announces under, and a screen whose reader is told about a seat at a
    /// name no press reaches is worse than one with no seat. R1716 wrote the
    /// same three-armed match twice and this round added a third arm to one of
    /// them, which is how the pair came to be counted.
    #[must_use]
    pub const fn act(self) -> &'static str {
        match self {
            Self::Remove(_) => "remove",
            Self::TakeOver(_) => "author",
            Self::GiveBack(_) => "disown",
        }
    }

    /// The same act at another place — what [`FormGeometry::translated`] needs.
    #[must_use]
    pub const fn at(self, rect: Rect) -> Self {
        match self {
            Self::Remove(_) => Self::Remove(rect),
            Self::TakeOver(_) => Self::TakeOver(rect),
            Self::GiveBack(_) => Self::GiveBack(rect),
        }
    }
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

/// ★★★★★ R1732 — **the row a picker is open on, and the room it has.**
///
/// The three things a laid-out popup is a function of, and no more: which row
/// it belongs to, where the reader is in the roster, and the rectangle it has
/// to stay inside. The last one is the caller's because it is the only thing
/// that knows — a form is laid into a pane it cannot see the bottom of, so a
/// popup that decided its own direction against the form's own extent would
/// open downward off the end of a scrolled viewport.
#[derive(Debug, Clone, Copy)]
pub struct OpenPicker<'a> {
    /// The configuration path whose row is open.
    pub key: &'a str,
    /// Where the picking is. The roster and the highlight both come from here,
    /// so the paint and a driver read one fact.
    pub picker: &'a Picker,
    /// The room the popup may use, in the same coordinates the geometry is laid
    /// in. It opens downward from the control and flips upward when downward
    /// would leave this rectangle.
    pub room: Rect,
}

/// ★★★★★ R1732 — **where an open picker's roster landed**, as its own layer.
///
/// A separate field on [`FormGeometry`] rather than more entries in the open
/// row's [`RowBox::parts`], because a popup is *over* the rows below it and a
/// hit test that walked the rows in order would resolve a press on the roster
/// to whatever row it happens to cover. Publishing it apart makes the layering
/// a declared fact instead of a consequence of iteration order — and
/// [`FormGeometry::option_at`] then has one place to consult first.
///
/// ★ R1762 — the type itself moved to [`crate::chooser`], where the control it
/// belongs to now lives: a collapsed chooser is not a thing only a form has,
/// and a second consumer proved it. The suffix vocabulary is unchanged —
/// `option.<key>.<word>` — so a driver that could press an option before
/// presses the same name now.
pub use crate::chooser::RosterBox as PickerPopup;

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
    /// The open picker's roster, when one is open — the layer over the rows.
    pub popup: Option<PickerPopup>,
    /// How tall the whole form came out.
    ///
    /// The popup is **not** in it: a roster floats over the rows and reflowing
    /// the form every time one opened would move everything a reader was
    /// looking at.
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
                        seat: row.seat.at(moved(row.seat.rect())?),
                    })
                })
                .collect(),
            chips: self
                .chips
                .iter()
                .filter_map(|(k, r)| Some((k.clone(), moved(*r)?)))
                .collect(),
            // A roster whose box the shift would move off the top is dropped
            // whole, options and all — the same truthful answer the rows get,
            // and half a popup is worse than none because a press would still
            // land on the options that survived.
            popup: self.popup.as_ref().and_then(|popup| {
                Some(PickerPopup {
                    key: popup.key.clone(),
                    rect: moved(popup.rect)?,
                    options: popup
                        .options
                        .iter()
                        .filter_map(|(s, r)| Some((s.clone(), moved(*r)?)))
                        .collect(),
                    above: popup.above,
                })
            }),
            height: self.height,
        }
    }

    /// The row at that path, if it is shown.
    #[must_use]
    pub fn row(&self, key: &str) -> Option<&RowBox> {
        self.rows.iter().find(|r| r.key == key)
    }

    /// ★★★★★ R1732 — the option an open roster has at that point, if any.
    ///
    /// The layer a consumer's hit test must consult **before** the rows, and
    /// the reason the popup is published apart from them. Answers the tag
    /// suffix, which is the same name the press had when the roster was drawn
    /// in the row.
    #[must_use]
    pub fn option_at(&self, x: u32, y: u32) -> Option<&str> {
        let popup = self.popup.as_ref()?;
        popup
            .options
            .iter()
            .find(|(_, rect)| {
                x >= rect.x && x < rect.x + rect.w && y >= rect.y && y < rect.y + rect.h
            })
            .map(|(suffix, _)| suffix.as_str())
    }

    /// Whether that point is anywhere on an open roster.
    ///
    /// Distinct from [`Self::option_at`] on purpose: a press inside the box but
    /// between two options is still the picker's, and letting it fall through
    /// to the row underneath is how a reader dismisses a menu by accident.
    #[must_use]
    pub fn on_popup(&self, x: u32, y: u32) -> bool {
        self.popup.as_ref().is_some_and(|popup| {
            let r = popup.rect;
            x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
        })
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
    // ★★ R1716 — a derived row shows its value; it does not offer a way to
    // enter one. A list's height is the height of its editing affordances, and
    // a row with none of them is one line like any other read-out.
    if !field.source().writable() {
        return style.control_h;
    }
    match field.shape() {
        FieldType::List { .. } => {
            let shown = field.value();
            let elements = u32::try_from(FieldType::elements(&shown).count()).unwrap_or(0);
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
///
/// R1716 — a derived row is a read-out whatever its shape, so it takes the box
/// a read-out takes.
///
/// ★★★★★ R1732 — a [`FieldType::Choice`] moved to the hungry side, because it
/// stopped being a row of chips and became a **collapsed** control: it holds
/// one word and an affordance to open the rest, which is the same shape a text
/// box has, and the reference draws it at the full width of the pane exactly as
/// it draws the text box. Only a set stays unhungry.
fn control_is_hungry(field: &ConfigField) -> bool {
    !field.source().writable() || !matches!(field.shape(), FieldType::Flags { .. })
}

/// The natural width of a field's control, before any growth policy.
fn control_hint(field: &ConfigField, style: &FormStyle) -> u32 {
    if !field.source().writable() {
        return style.control_hint_w;
    }
    match field.shape() {
        FieldType::Flags { of } => {
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

/// The width of the chevron seat at a collapsed control's trailing edge.
///
/// ★ R1762 — the roster's own frame and gap left with it (see
/// [`crate::chooser`]); this one stays because the form measures a row's
/// control hint against it.
const CHEVRON_W: u32 = 22;

/// Horizontal padding inside an option chip.
const CHIP_PAD: u32 = 10;
/// Space between option chips.
const CHIP_GAP: u32 = 6;
/// Height of the "add this key" chip row entries.
const ADD_CHIP_H: u32 = 24;

// ★ R2020 — `BADGE_PAD` moved to `crate::badge`, with the paint it belongs to.
//
// ⚠ It is published there rather than private, and NOT because this form reads
// it: the header hands its width deficit to the flex pass (see `view_header`),
// so no number here has to be right. It is public because a caller laying a row
// out by hand has to know how much a badge takes beyond its word — which is the
// thing R1656 measured going wrong.

/// The gap between the header's items, which the width budget also spends.
const HEADER_GAP: u32 = 6;
/// A numeric stepper button's width.
const STEP_W: u32 = 26;
/// Vertical space between a list's element rows.
const LIST_GAP: u32 = 4;

/// The size a boolean's word is drawn at.
const BOOL_WORD_PX: u32 = 12;
/// The gap between a boolean's switch and its word. The behaviour canon's own
/// `gap`, measured off its boolean control.
const BOOL_WORD_GAP: u32 = 9;
/// The horizontal padding inside a boolean's box, each side. The canon's, from
/// the same rule.
const BOOL_WORD_PAD: u32 = 10;

/// ★★★★★ R2050 §5.2 §5.11 — **the addresses this painter gives a form's
/// parts**, declared where they are produced.
///
/// # What was missing
///
/// This painter composes a child's tag from the form's prefix and a row's key
/// — and it did so with a `format!` at every producing site, while every
/// consumer that wanted one composed the same string again in its own source:
/// the screen's hit test, its specification tables, its gates, and the walks
/// that check the frame from outside. Measured at this round's open, the
/// control address alone was spelled at **53** sites across a framework crate,
/// a screen and eight walks.
///
/// ⇒ one wrong letter compiles, paints, and makes every query looking for the
/// control answer nothing — quietly, because a mark that is not found reads as
/// a mark that was not painted.
///
/// # ★ The producer is the owner
///
/// R2049 lifted a screen's own address onto the type that owns it. This family
/// is not the screen's: the FRAMEWORK builds these tags, so the declaration
/// belongs here and a consumer asks. That is the same shape
/// [`crate::toolbar::composite_item_tag`] has had since R692 — a published
/// function that composes a child address from its parent's tag.
///
/// # ⚠ What a walk does instead
///
/// A walk is Python and cannot call this. Its answer is to read the address off
/// the wire, which is why a screen publishing a form should publish each row's
/// control address beside the row.
pub mod address {
    /// The address of the control a row's value is edited through.
    ///
    /// `prefix` is the tag the form was painted under and `key` is the row's.
    #[must_use]
    pub fn control(prefix: &str, key: &str) -> String {
        child(prefix, "control", key)
    }

    /// The address of the roster a collapsed chooser opens onto.
    #[must_use]
    pub fn roster(prefix: &str, key: &str) -> String {
        child(prefix, "roster", key)
    }

    /// The general form: one of this painter's parts, for one row.
    ///
    /// ★ Public because the painter names more parts than a consumer usually
    /// addresses, and a consumer that needs one of the others should reach for
    /// this rather than for a `format!` — which is the whole defect.
    #[must_use]
    pub fn child(prefix: &str, part: &str, key: &str) -> String {
        format!("{prefix}.{part}.{key}")
    }

    /// A part of the form that belongs to no row.
    ///
    /// ★ Takes anything that reads as a string, because the parts this composes
    /// arrive differently at different sites — some are literals and some are
    /// built from a row's own vocabulary — and a caller should not have to
    /// convert just to spell an address.
    #[must_use]
    pub fn part(prefix: &str, part: impl AsRef<str>) -> String {
        format!("{prefix}.{}", part.as_ref())
    }

    /// The row a control address names, given the prefix it was painted under.
    ///
    /// ★★ The inverse, here rather than at each consumer's router. R2049's
    /// lesson: a parse written against a separately-typed prefix is the second
    /// speller, and its mismatch is silent in the other direction — the press
    /// lands on nothing and the screen simply does not respond.
    #[must_use]
    pub fn control_key<'a>(prefix: &str, tag: &'a str) -> Option<&'a str> {
        key_of(prefix, "control", tag)
    }

    /// The row a part's address names, or `None` when the tag is not that part.
    #[must_use]
    pub fn key_of<'a>(prefix: &str, part: &str, tag: &'a str) -> Option<&'a str> {
        tag.strip_prefix(&format!("{prefix}.{part}."))
    }
}

/// Lay a form out, giving every part a rectangle.
///
/// `origin` is where the form's first row starts. The geometry is the single
/// source both [`view_config_form`] and a consumer's hit test read.
#[must_use]
pub fn form_geometry(form: &ConfigForm, origin: (u32, u32), style: &FormStyle) -> FormGeometry {
    form_geometry_showing(form, origin, style, None)
}

/// Lay a form out with one row's picker open.
///
/// ★★★★★ R1732 — the same layout, plus the roster's own layer. `open` names
/// the row and carries the room the popup has; `None` is [`form_geometry`].
///
/// A row whose key `open` names but whose shape offers no roster lays no popup
/// — the caller is describing a state the form cannot be in, and inventing a
/// roster for it would put options on a row that has none.
#[must_use]
pub fn form_geometry_showing(
    form: &ConfigForm,
    origin: (u32, u32),
    style: &FormStyle,
    open: Option<OpenPicker<'_>>,
) -> FormGeometry {
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

    let popup = open.and_then(|open| {
        let row = rows.iter().find(|r| r.key == open.key)?;
        let picks = form
            .field(open.key)
            .is_some_and(|field| matches!(field.shape(), FieldType::Choice { .. }));
        picks.then(|| lay_popup(row, open, style))
    });

    FormGeometry {
        origin,
        rows,
        chips,
        popup,
        height: y.saturating_sub(y0),
    }
}

/// One open roster's rectangles, over the row it belongs to.
///
/// The direction is **derived, not chosen**: downward from the control unless
/// the whole roster would leave [`OpenPicker::room`], and upward when it would.
/// A roster taller than the room stays downward and is reported at its full
/// height — clamping it would publish a box the paint does not draw, and the
/// consumer that has to scroll it needs the real number.
fn lay_popup(row: &RowBox, open: OpenPicker<'_>, style: &FormStyle) -> PickerPopup {
    // ★ R1762 — the arithmetic moved to `crate::chooser`, which is where the
    // control it belongs to lives. What stays here is the form's own half: the
    // row a roster hangs off, and the row height it lays its options at.
    crate::chooser::lay_roster(
        &row.key,
        row.control,
        open.picker,
        open.room,
        style.control_h,
    )
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
        let hungry = control_is_hungry(field);
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
        // ★★ R1686 — the seat that takes this row away, cut out of the header's
        // trailing edge before the header is handed to the flex pass. Cut
        // rather than overlaid: a badge laid out into the full width and a
        // glyph painted on top of its last pixels is the R1656 class exactly,
        // where every box is right and only their sum is wrong.
        let (header, seat) = split_off_remove(header, key_line);
        RowBox {
            key: field.key().to_string(),
            row: Rect::new(x0, y, style.width, row_h),
            header,
            control,
            wrapped,
            // ★★ R1716 — a row nobody wrote has no affordances INSIDE its
            // control either: the chips, the toggle and the stepper are all
            // ways to write a value, and painting them over one the screen
            // works out would be an invitation the form refuses.
            parts: if field.source().writable() {
                lay_parts(field, control, style)
            } else {
                Vec::new()
            },
            // ★★★ R1717 — three acts, from the three answers to where the
            // value came from. The seat is not a widget a screen chooses: the
            // row's own provenance decides which act it can honestly offer.
            seat: match field.source() {
                Source::Authored => Seat::Remove(seat),
                Source::Derived(_) => Seat::TakeOver(seat),
                Source::Shared(_) => Seat::GiveBack(seat),
            },
        }
    }
}

/// Take the remove seat out of a header, and answer both halves.
///
/// The seat is a **square of the key's line box**, so it is exactly as tall as
/// the text it sits beside and cannot be a number that goes stale when the
/// key's size changes. It is vertically centred, because a header under
/// [`RowWrap::Beside`] is as tall as the control rather than as tall as one
/// line, and the flex pass centres the badges it holds for the same reason.
fn split_off_remove(header: Rect, key_line: u32) -> (Rect, Rect) {
    let seat = key_line.min(header.w);
    let x = header.x + header.w - seat;
    let y = header.y + (header.h.saturating_sub(key_line)) / 2;
    (
        Rect::new(
            header.x,
            header.y,
            header.w.saturating_sub(seat + HEADER_GAP),
            header.h,
        ),
        Rect::new(x, y, seat, key_line.min(header.h)),
    )
}

/// The word a boolean row shows.
///
/// One derivation, because the box that has to hold it and the paint that draws
/// it must not disagree about which word it is.
fn boolean_word(field: &ConfigField) -> &'static str {
    if field.value().trim() == "true" {
        "true"
    } else {
        "false"
    }
}

/// The switch a boolean row wears, at the analysis tool's own metrics.
///
/// ★★★★★ R1837 — measured off the behaviour canon rather than chosen: its
/// boolean control is a 30x17 pill track with a 13x13 knob inset 2 px, the
/// track taking the accent when on. [`SwitchStyle::m3`]'s 64x32 is the size a
/// settings row gives a switch and is half again as tall as this form's control
/// line, which is why the metrics are stated here instead of defaulted.
///
/// Not focusable: the control container around it is the row's Tab stop, and a
/// second stop inside it would make a reader press Tab twice to leave one
/// control.
const fn boolean_switch_style() -> SwitchStyle {
    SwitchStyle {
        track_w: 30,
        track_h: 17,
        track_radius: 8,
        track_pad: 2,
        knob_size: 13,
        knob_radius: 6,
        focusable: false,
        // ★★★★★ R1837 — and pointer-TRANSPARENT, which is load-bearing rather
        // than tidy. This form publishes its geometry and its consumer's hit
        // test reads it; a tagged node that is an address rather than a
        // primitive SWALLOWS a press and forwards nothing (R1649.1, a whole
        // screen dead to a real mouse while 118 scripted assertions passed).
        // The tag has to stay, because the conformance census classifies a row
        // by the affordance its control draws and an untagged track is
        // invisible to it — so the press must pass through instead.
        pointer_transparent: true,
    }
}

/// ★★★★★ R1837 — the knob's TRAVEL is the canon's, and it is checked rather
/// than asserted in prose.
///
/// The canon puts its knob at `left: 2px` when off and `left: 15px` when on, so
/// it moves **13 px**. [`SwitchStyle`] documents travel as
/// `track_w - 2 * track_pad - knob_size`, and this form's metrics give
/// `30 - 4 - 13`, which is the same 13 — derived from the track and knob rather
/// than copied from the canon's two offsets.
///
/// A compile-time assertion because it is arithmetic over constants: a test
/// would run it, and this cannot even build wrong. Restyling the track without
/// restyling the knob is what it catches, which is the one edit that would move
/// the knob's throw while every rendering still looked plausible.
const _: () = {
    let s = boolean_switch_style();
    assert!(
        s.track_w - 2 * s.track_pad - s.knob_size == 13,
        "the switch's knob no longer travels the distance the canon's does"
    );
};

/// Where a row's affordances land inside its control.
///
/// One function over every shape, because the alternative is one per shape and
/// a consumer that has to know which. What each shape gets:
///
/// | shape | parts |
/// |---|---|
/// | [`FieldType::Text`] / [`FieldType::Formatted`] | none — the control *is* the box |
/// | [`FieldType::Integer`] | `step.<key>.down`, `step.<key>.up` |
/// | [`FieldType::Boolean`] | none — the control *is* the switch (R1837) |
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
        // A formatted string is a text box too: what differs is what it will
        // accept, and that is not a part anybody can press.
        //
        // ★★★★★ R1837 — and a `Boolean` joined them, which is why the arm reads
        // as one. It published `toggle.<key>` over a `h x h` square holding the
        // mark alone, so the word `true`/`false` beside it sat outside every
        // rectangle the form published — outside the press target, outside the
        // announcement's bounds — while the whole box took the press. A person
        // reported the row as unreadable ("is that a text edit or a button"),
        // and a control whose published affordance is a square next to an
        // unexplained word is what that reads as; the square was also announced
        // as a SECOND checkbox carrying the control's own name and bit. The
        // control IS the switch now, the same way a text row's control is its
        // box.
        FieldType::Text | FieldType::Formatted { .. } | FieldType::Boolean => Vec::new(),
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
        // ★★★★★ R1732 — one part, the chevron that opens the roster, at the
        // trailing edge where the reference draws it. The roster itself is not
        // in this list: it is a layer over the rows below, so it is published
        // as [`FormGeometry::popup`] and pressed through
        // [`FormGeometry::option_at`].
        FieldType::Choice { .. } => {
            let w = CHEVRON_W.min(control.w);
            vec![(
                format!("pick.{key}"),
                Rect::new(
                    control.x + control.w.saturating_sub(w),
                    control.y,
                    w,
                    control.h,
                ),
            )]
        }
        FieldType::Flags { of } => {
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
            let shown = field.value();
            for (n, _) in FieldType::elements(&shown).enumerate() {
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
pub(crate) fn placed(layout: LayoutStyle, rect: Rect, origin: (u32, u32)) -> LayoutStyle {
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

/// ★★★★★ R2020 — **the state a row's applies-scope reports, so the badge is
/// filled with that state's own ground.**
///
/// It was a bare ink — `Accent` for hot, `OnSurfaceMuted` for restart — on the
/// shared raised tier, and it said neither of the two things the canon says
/// here. Measured on the behaviour canon's own inspector, the policy chip is
/// drawn `HOT` on the RIGHT-state ground and `RESTART` on the CAUTION one:
/// *this edit reaches the running node* is a state that is well, and *this edit
/// waits for a restart* is one that wants care. `Accent` said something else
/// again — it is this vocabulary's interactive tone, so a hot row's badge read
/// as a thing to press.
/// It is `pub` because a gate that judged the painted colour against a table of
/// its own would be checking two spellings agree rather than checking the
/// screen. A consumer asks the rule and compares that to the pixels.
#[must_use]
pub const fn applies_state(applies: Applies) -> StateTone {
    match applies {
        Applies::Hot => StateTone::Success,
        Applies::Restart => StateTone::Warning,
    }
}

/// ★★★★★ R2020 — **the state a row's worst defect reports.**
///
/// The two arms are the distinction [`ConfigDefect::blocks`] already draws —
/// this one stops the node coming up, that one does not — and R1651 argued the
/// caution tier into existence for exactly it. Until this round both were an
/// ink on the shared raised tier, so the row that stops a launch and the row
/// that does not differed by the colour of eight small letters.
///
/// `pub` for [`applies_state`]'s reason.
#[must_use]
pub const fn defect_state(defect: &ConfigDefect) -> StateTone {
    if defect.blocks() {
        StateTone::Error
    } else {
        StateTone::Warning
    }
}

/// A read-out chip beside a row's key, in the neutral form.
///
/// The paint lives in [`crate::badge`] now — a badge is not this widget's, and
/// leaving it private here is what would have made the next screen that wanted
/// one copy it. What stays here is which of the two forms each of this form's
/// badges takes, which IS this widget's decision.
fn badge(text: &str, ink: ColorRole, theme: &Theme, tag: Option<(String, Silence)>) -> Scene {
    view_badge(text, BadgeTone::Neutral { ink }, theme, tag)
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
    view_config_form_showing(tag_prefix, form, geometry, theme, None)
}

/// Paint a form with one row's roster open over it.
///
/// ★★★★★ R1732 — `picker` must be the one [`form_geometry_showing`] laid the
/// popup from. The two halves are separate arguments because they answer
/// different questions — where the options landed, and where the reader is —
/// and a painter that re-derived the second from the first could not draw a
/// reader moving away from the value the document holds.
///
/// A popup in the geometry with no picker here paints nothing but the rows: the
/// caller has described half a state, and drawing a roster with no highlight
/// would invent the missing half.
#[must_use]
pub fn view_config_form_showing(
    tag_prefix: &str,
    form: &ConfigForm,
    geometry: &FormGeometry,
    theme: &Theme,
    picker: Option<&Picker>,
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
        children.push(view_remove_seat(tag_prefix, row, geometry.origin, theme));
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

    // Last, so it is over everything the form drew — the layering the popup's
    // own field exists to make explicit.
    if let (Some(popup), Some(picker)) = (geometry.popup.as_ref(), picker) {
        let chosen = form
            .field(&popup.key)
            .map(|field| field.value().into_owned())
            .unwrap_or_default();
        children.push(view_picker_popup(
            tag_prefix,
            popup,
            picker,
            &chosen,
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
        let said = address::child(tag_prefix, "said", &row.key);
        // ★★★★ R1732 — the key run is ADDRESSED now, and declares its silence.
        // It was an untagged run for its whole life, which meant the one thing
        // on the row that says what the row is about could not be found by a
        // driver, read back by a conformance check, or seen by the census at
        // all. Its words are the control's accessible name, so the honest
        // classification is `NameOf` rather than a second node saying the same
        // path over again.
        let mut header: Vec<Scene> = vec![Scene::Text(
            TextNode::styled(
                field.key().to_owned(),
                Rect::default(),
                form_run_style()
                    .with_size_px(11)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )
            .with_tag(address::child(tag_prefix, "key", &row.key))
            .with_layout(
                LayoutStyle::new()
                    .with_min_size(Size::px(0, 0))
                    .with_flex_shrink(1.0)
                    // Caught by R1655's gate the moment the tag was added: a
                    // tagged node that is an ADDRESS rather than a primitive
                    // swallows the press and forwards nothing.
                    .with_pointer_transparent(true)
                    .with_silence(Silence::name_of(address::control(tag_prefix, &row.key))),
            ),
        )];
        header.push(badge(
            &type_word(field),
            ColorRole::OnSurfaceMuted,
            theme,
            Some((
                address::child(tag_prefix, "type", &row.key),
                Silence::name_of(said.clone()),
            )),
        ));
        // What an edit costs, and where the value came from — see
        // [`provenance_badges`], which is where the rule for the pair lives.
        header.extend(provenance_badges(tag_prefix, field, &row.key, &said, theme));
        if let Some(instead) = field.goes().instead() {
            // ★★ R1716 — and a row that is not configuration says so beside
            // the source, because "where does this value come from" and "does
            // this value ship" are two questions and a reader has both.
            header.push(badge(
                instead,
                ColorRole::OnSurfaceMuted,
                theme,
                Some((
                    address::child(tag_prefix, "aside", &row.key),
                    Silence::name_of(said.clone()),
                )),
            ));
        }
        if let Some(defect) = worst {
            // ★★★★★ R2020 — a defect is a STATE, so it is filled with that
            // state's ground. See `defect_state` for which is which.
            let tone = defect_state(defect);
            // ★★★★★ R2002 — the badge paints the PHRASE, not the wire
            // spelling. See `ConfigDefect::phrase`: `out_of_range` is a token an
            // agent matches, and the description this badge lends its ink to is
            // built by putting the same phrase in front of the sentence, so
            // label-in-name holds by construction.
            header.push(view_badge(
                defect.phrase(),
                BadgeTone::State(tone),
                theme,
                Some((
                    address::child(tag_prefix, "defect", &row.key),
                    Silence::name_of(said),
                )),
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

/// ★★★★★ R1732 — what the type badge says: the word the configuration calls
/// this kind of value, and **how many words are on offer** when the value is
/// one of a set.
///
/// The count is the reference's own, and the two halves of this round need each
/// other: while every option was on screen a reader could see how many there
/// were, and a collapsed control hides exactly that. Derived here rather than
/// authored on the field, because a roster that grew and a badge that did not
/// would be a number going stale in the one place a reader trusts it.
fn type_word(field: &ConfigField) -> String {
    match field.shape() {
        FieldType::Choice { of } | FieldType::Flags { of } => {
            format!("{} \u{b7} {}", field.ty(), of.len())
        }
        _ => field.ty().to_owned(),
    }
}

/// The badges that say **what an edit costs** and **where the value came
/// from**, in the order a reader meets them.
///
/// ★★★ R1716 — a row nobody wrote shows where its value came from where a row
/// somebody wrote shows what an edit would cost. The restart badge answers "if
/// you change this, when does it land"; on a row that refuses every change that
/// question has no reader, and the one they do have — "why can I not type
/// here" — had no answer at all. The behaviour canon suppresses exactly this
/// badge on exactly these rows and keeps the live one, which is the same rule:
/// a hot row's value still reaches the running node when its SOURCE moves.
///
/// ★★★★★ R1717 — a row with TWO contributors keeps both, because a reader of
/// one has both questions: they may still type here, so what an edit costs is
/// news, and part of what they are reading is not theirs, so where the rest
/// came from is news as well. The source badge then carries the **count**,
/// which is the fact the floor has no shape for at all — measured, a cell
/// holding a composed value answers 2 of 256 standard roles and none of them is
/// how much of it is not yours.
fn provenance_badges(
    tag_prefix: &str,
    field: &ConfigField,
    key: &str,
    said: &str,
    theme: &Theme,
) -> Vec<Scene> {
    let applies = || {
        view_badge(
            field.applies().wire(),
            BadgeTone::State(applies_state(field.applies())),
            theme,
            Some((
                address::child(tag_prefix, "applies", key),
                Silence::name_of(said.to_owned()),
            )),
        )
    };
    let source = |word: &str| {
        badge(
            word,
            ColorRole::OnSurfaceMuted,
            theme,
            Some((
                address::child(tag_prefix, "source", key),
                Silence::name_of(said.to_owned()),
            )),
        )
    };
    match field.source() {
        Source::Authored => vec![applies()],
        Source::Derived(from) => {
            let mut out = Vec::new();
            if field.applies() == Applies::Hot {
                out.push(applies());
            }
            out.push(source(&provenance_phrase(&from, None)));
            out
        }
        Source::Shared(from) => vec![
            applies(),
            source(&provenance_phrase(&from, Some(field.derived_elements()))),
        ],
    }
}

/// ★★★★★ R1954 — **the words a source badge PAINTS, and the only place they
/// are spelled.**
///
/// A source badge declares [`Silence::name_of`] against the row's description
/// node: *my text is that node's name*. WAI-ARIA calls the resulting obligation
/// label-in-name, and it is the one place on these screens where some ink and
/// some announcement are declared to be the same words — so it is the one place
/// a comparison is a comparison rather than a guess.
///
/// It was two spellings until this round. R1952 replaced the badge's hooked
/// arrow with a word and wrote `"from {from}"`, while the description said
/// `"worked out from the {from}"` — one article apart, which is enough: a
/// person reading the badge aloud says three words the screen reader never
/// says, and `r1692` failed exactly there. The sighted reader and the listening
/// reader were handed different phrases for one fact.
///
/// ⇒ [`provenance_sentence`] is built by PREPENDING to this, so the badge's
/// words are a contiguous run of the description's words **by construction**.
/// A gate can still check it — and does — but the check can no longer be the
/// only thing holding them together.
fn provenance_phrase(from: &str, shared_elements: Option<usize>) -> String {
    match shared_elements {
        Some(n) => format!("from the {from} {n}"),
        None => format!("from the {from}"),
    }
}

/// ★★★★★ R1954 — **what a reader who cannot see the badge is told instead.**
///
/// [`provenance_phrase`] with the verb in front of it. The composition is the
/// guarantee: there is no way to write this sentence that does not contain the
/// badge's own words, in the badge's own order.
///
/// R1716 recorded why the sentence has to carry everything the badge does — *a
/// reader who cannot see the badge is the reader who most needs what it says* —
/// and that is why the shared-row COUNT travels here too. It was only on the
/// badge until this round, so the one fact the floor has no shape for at all
/// was the one fact a listening reader did not get.
fn provenance_sentence(from: &str, shared_elements: Option<usize>) -> String {
    format!("worked out {}", provenance_phrase(from, shared_elements))
}

// ★★★★★ R1952 — the badge is **prose**, and a badge is the one place in this
// module where a drawn mark is the wrong answer: its run is 9px and the
// narrowest slot `Indicator::MIN` draws into is thirteen, so a mark would set
// the pill's height rather than sit in it. Prose has to be spellable in the
// face this tree ships, and `U+21AA` is not — so the badge says the word.
//
// ⚠ R1954 — and saying "the word" is what broke it. The badge's prose is
// [`provenance_phrase`] now, composed into the description by
// [`provenance_sentence`], because a badge that declares itself another node's
// NAME cannot be allowed to spell that name a second way.

// ★★★★★ R1952 — **the three seats stopped being characters, and one of them
// carried a sentence that had gone false.**
//
// They were `U+00D7`, `U+21AA` and `U+21A9`, and the middle one's doc said in
// as many words: *the face this project ships covers it.* Measured at R1952
// with `Font::glyph_id_for` against `NotoSans-Regular` — the face
// `pinion_text::test_font` calls *one face across the tree* — it does not, and
// neither does the other arrow. The analysis shell's node lab was painting
// `.notdef` boxes on eight rows of its author and source forms.
//
// ⚠ `U+00D7` IS in that face, and it moved anyway. The rule cannot be *typeset
// when the face happens to have the character*: that is exactly the reasoning
// which produced this round's four defects — every one of them was written by
// somebody who had checked, or believed they had. A mark a widget draws is
// drawn. What remains typeset here is prose.
//
// The marks are `crate::indicator::Indicator::{Discard, TakeOver, GiveBack}`,
// and the take-over / give-back pair is still one drawing mirrored, which is
// what R1717 asked for and what a character pair could never guarantee.

/// The seat at a row's trailing edge: it takes an authored row **out**, takes a
/// derived row **over**, and gives a shared row's written half **back**.
///
/// Painted at the rectangle [`RowBox::seat`] published, not at one computed
/// here — the property this module has kept since R1651.1, and the reason it
/// keeps it is that the two copies drift silently and the press lands nowhere.
fn view_remove_seat(tag_prefix: &str, row: &RowBox, origin: (u32, u32), theme: &Theme) -> Scene {
    let mark = match row.seat {
        Seat::Remove(_) => Indicator::Discard,
        Seat::TakeOver(_) => Indicator::TakeOver,
        Seat::GiveBack(_) => Indicator::GiveBack,
    };
    let seat = row.seat.rect();
    let tag = address::child(tag_prefix, row.seat.act(), &row.key);
    Scene::Container(
        ContainerNode::new(vec![crate::indicator::inline(
            mark,
            seat.h.min(seat.w),
            theme.resolve(ColorRole::OnSurfaceMuted),
            "the seat's mark; the seat itself says the act in words",
        )])
        .with_tag(tag)
        .with_layout(placed(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center),
            row.seat.rect(),
            origin,
        )),
    )
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
        // ★★★★★ R1716 — **the shape decides the control only for a value
        // somebody owns.** A chip row, a toggle and a stepper are all ways of
        // writing, and drawing one over a value the screen works out is the
        // exact lie the badge is there to stop: the form would refuse the press
        // it just invited. So a derived row is a read-out of its own value, in
        // the muted ink the canon draws it in, and the seat beside it offers
        // the act that IS available — taking the row over.
        if !field.source().writable() {
            return derived_control(tag_prefix, row, field, origin, theme);
        }
        match field.shape() {
            // ★★★★★ R1732 — one word and a chevron, where a row of every
            // option used to be. The roster is drawn by
            // [`view_picker_popup`], over the rows, only while it is open.
            FieldType::Choice { .. } => {
                picker_control(tag_prefix, row, field, worst, origin, theme)
            }
            FieldType::Flags { .. } => {
                let shown = field.value();
                let chosen: Vec<&str> = FieldType::elements(&shown).collect();
                option_chips(tag_prefix, row, &chosen, origin, theme)
            }
            FieldType::Boolean => boolean_control(tag_prefix, row, field, worst, origin, theme),
            FieldType::Integer { .. } => {
                number_control(tag_prefix, row, field, worst, origin, theme)
            }
            FieldType::List { .. } => list_control(tag_prefix, row, field, origin, theme),
            // ★ R1690 — the same box. A shape is what the value has to BE, and
            // a control is how it is entered; those two coincide for every
            // string, whether or not anything downstream parses it. Giving a
            // formatted string its own control would put the difference in the
            // wrong place, where a person has to learn it twice.
            FieldType::Text | FieldType::Formatted { .. } => {
                text_control(tag_prefix, row, field, worst, origin, theme)
            }
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
        //
        // ★ R1732 — a `Choice` joined them when it collapsed. Its one part is
        // the chevron seat at the trailing edge, and before the inset was
        // applied that seat stood on the control's own outline, which is
        // exactly the escape R1672 measured for the stepper pair.
        //
        // ★★★★★ R1837 — and a `Boolean` joined them when it stopped painting
        // into an unstyled container. It wears the row box now, for the reason
        // the behaviour canon does: the box is the same for a value you type
        // and a value you flip, and what tells the two apart is the SWITCH
        // inside it, not the presence of a frame.
        FieldType::Text
        | FieldType::Formatted { .. }
        | FieldType::Integer { .. }
        | FieldType::Boolean
        | FieldType::Choice { .. } => CONTROL_FRAME,
        // These paint into an unstyled container — a row of option pills, a
        // column of self-skinned item boxes. There is no outline of their own
        // for a part to stand on.
        FieldType::Flags { .. } | FieldType::List { .. } => 0,
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
pub(crate) fn framed(base: LayoutStyle) -> LayoutStyle {
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

/// The skin a derived row's read-out wears: **no fill at all**, and the muted
/// outline.
///
/// R1716 — the fill is what a person's eye reads as "you may type here", so a
/// read-out does not wear one. The border stays, because the value is still one
/// field's worth of text and losing its box would make a list of them
/// unreadable.
///
/// 🟥★★★★★ **The transparency is the decision, and it was reached by
/// LOOKING.** The first draft asked the theme for `Surface` — the panel's own
/// tone, which reads as "no fill" in the palette this widget was written
/// against. Photographed on the analysis tool's node lab and sampled: the panel
/// is `(22, 24, 29)`, an editable control is `(236, 230, 240)`, and the
/// read-out came out `(255, 255, 255)` — **brighter than the rows a person may
/// type into**, so the one box on the panel that refuses every keystroke was
/// the one that invited them hardest. Every gate was green: nothing here
/// asserted a skin, and a role name is not a colour. Hence the test below,
/// which pins the *decision* rather than the pixel: a read-out has no fill,
/// whatever a theme resolves its roles to.
fn derived_skin(theme: &Theme) -> BoxStyle {
    BoxStyle::filled(Color::TRANSPARENT)
        .with_corner_radius(8)
        .with_border(Border::new(
            theme.resolve(ColorRole::Outline),
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

/// ★★★★★ R1732 — a choice row **collapsed**: the word it holds, and the
/// chevron that opens the rest.
///
/// The same box a text row wears, which is the reference's own decision — its
/// inspector gives the enumeration control the text field's style verbatim, so
/// the two read as the same kind of thing and only the chevron says one of them
/// has a roster.
fn picker_control(
    tag_prefix: &str,
    row: &RowBox,
    field: &ConfigField,
    worst: Option<&ConfigDefect>,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    // ★ R1762 — the control moved to `crate::chooser`. What stays here is what
    // is about a FORM: which tags this form addresses its parts by, and the
    // skin a defect on the value behind it paints.
    crate::chooser::view_collapsed(
        &crate::chooser::ChooserTags {
            control: address::control(tag_prefix, &row.key),
            shown: address::child(tag_prefix, "shown", &row.key),
            arrow: address::child(tag_prefix, "pick", &row.key),
        },
        &field.value(),
        row.control,
        origin,
        control_skin(worst, theme),
        theme,
    )
}

/// ★★★★★ R1732 — **an open roster, over the rows it covers.**
///
/// Painted after the form so it is on top, and from
/// [`FormGeometry::popup`] so the rectangles a press is resolved against and
/// the rectangles a reader sees are the same value. The highlight comes from
/// the [`Picker`], the mark from the field's own word: those are two different
/// facts — where the reader is, and what the document holds — and a roster that
/// drew one of them twice would be unable to show a reader moving away from the
/// value.
#[must_use]
pub fn view_picker_popup(
    tag_prefix: &str,
    popup: &PickerPopup,
    picker: &Picker,
    chosen: &str,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    // ★ R1762 — the roster's paint moved to `crate::chooser` with the control
    // it belongs to. This name stays because it is what a form's caller reaches
    // for, and it forwards rather than restating: a second copy of a roster's
    // paint is how two surfaces come to disagree about one control.
    crate::chooser::view_roster(tag_prefix, popup, picker, chosen, origin, theme)
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
                address::part(tag_prefix, suffix),
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
            .with_tag(address::control(tag_prefix, &row.key))
            .with_layout(placed(
                LayoutStyle::new().with_focusable(true),
                row.control,
                origin,
            )),
    )
}

/// A boolean row: **a switch and its word**, in the box every other row wears.
///
/// ★★★★★ R1837 — this drew a bordered pill carrying `U+2713` or a SPACE, in an
/// unstyled container, and a person looking at the running window could not
/// tell the row from a text field: *"is that a text edit or a button"*.
///
/// Two things were wrong and only one of them was the mark.
///
/// **The catalog already had the control, and this file drew a different one.**
/// [`crate::switch`] was lifted at R1574 out of twelve bindings that had each
/// hand-rolled a track and a knob. This file did not hand-roll a thirteenth —
/// it drew something else entirely, a check pill, on a row that the behaviour
/// canon and `docs/analyzer-inspector-spec.json` both call **a switch, and the
/// word it is set to**. That is the worse of the two failures: a duplicated
/// painter is a maintenance cost, and the wrong control kind is a screen that
/// does not reproduce what it is specified against, with the right painter
/// sitting one module away in the same crate.
///
/// **And the canon does not distinguish the two rows by their box.** Measured
/// off the behaviour canon rather than reasoned about: its boolean control
/// carries the *same* filled, bordered, 8 px-radius box as the rows a person
/// types into, and what tells them apart is a 30x17 pill track with a 13x13
/// knob that MOVES — off at 2 px, on at 15 px. A mark that only changes colour
/// says nothing at a glance; a knob at the other end of its track is a
/// different picture.
///
/// ⚠ **Not what the reference toolkit at 6.11 does, and the difference was
/// measured rather than assumed.** Probed offscreen — four labels, two allotted
/// widths, a real press at three points each — its check control HUGS its
/// content: the pressable extent is the indicator plus the label's own advance
/// plus about four pixels (`false` -> 49 px, a 32-character label -> 206 px),
/// it does not grow with the room the layout hands the widget (284 px and 60 px
/// of allotment give the same 49 px), a press one pixel past it does nothing,
/// and the accessible bounds equal that extent exactly. That is a coherent
/// design and it is **not this screen's**: the canon governs here, its box is
/// the row's full cell with `cursor: pointer` across all of it, and this form's
/// consumer already presses it that way. Recorded because "we did not copy the
/// reference" should be a measurement, not an omission.
fn boolean_control(
    tag_prefix: &str,
    row: &RowBox,
    field: &ConfigField,
    worst: Option<&ConfigDefect>,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    let on = field.value().trim() == "true";
    let style = boolean_switch_style();
    let inner = inset_by(row.control, CONTROL_FRAME);
    // Children are positioned against this container, so every rectangle below
    // is RELATIVE to `row.control` — the frame included, because the content
    // box starts inside the outline the skin strokes.
    let left = CONTROL_FRAME + BOOL_WORD_PAD;
    let track_y = CONTROL_FRAME + inner.h.saturating_sub(style.track_h) / 2;
    let word_x = left + style.track_w + BOOL_WORD_GAP;
    let word_w = inner
        .w
        .saturating_sub(BOOL_WORD_PAD * 2 + style.track_w + BOOL_WORD_GAP);
    Scene::Container(
        ContainerNode::new(vec![
            // The switch is PLACED rather than flowed, because this container
            // holds absolutely-positioned children like every other control
            // here; the painter itself sizes the track, so the wrapper only
            // says where it sits.
            Scene::Container(
                ContainerNode::new(vec![crate::switch::view_switch(
                    address::child(tag_prefix, "switch", &row.key),
                    ToggleState::Idle,
                    on,
                    theme,
                    &style,
                    // Empty: the caption is the control's own announcement,
                    // which already carries this row's key and its checked bit.
                    // A name here would be the same control announced twice.
                    "",
                    // ★★★★★ R1837 — and it declares WHERE a reader receives it,
                    // rather than simply going quiet. A tagged, painted region
                    // that says nothing is `unvoiced` to the voice census, which
                    // is the apparatus R1691 built after 35 accessibility nodes
                    // turned out to leave five regions unaccounted for. The tag
                    // has to stay — the conformance census classifies this row
                    // by it — so the silence is what makes it legitimate.
                    Some(Silence::part_of(address::control(tag_prefix, &row.key))),
                )])
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(left, track_y)
                        .with_size(Size::px(style.track_w, style.track_h)),
                ),
            ),
            // ★★★★★ R1842 — the word is PLACED, the way the switch beside it
            // is, and it was not before. A `TextNode`'s own rectangle is a
            // declaration a laid-out parent does not read, so the rectangle
            // computed below was dead data and the word landed at the
            // control's ORIGIN: on top of the switch, and one pixel over the
            // outline the control strokes inside itself.
            //
            // ⚠ THE FIRST DRAFT OF THIS COMMENT SAID EVERY OTHER CONTROL HERE
            // PASSES `Rect::default()`, AND THIS ROUND'S OWN CLOSING AUDIT
            // FOUND THAT FALSE. Count them rather than trusting a number:
            // `grep -A2 'TextNode::styled' <this file> | grep -c 'Rect::new'`.
            // The ones that do not are the SAME shape this repair removes — a
            // computed rect under a `placed(...)` parent, carrying the same
            // 14 px line box beside the same 12 px face. Whether their runs
            // land wrong too is NOT measured here ⇒
            // `debt-a-text-nodes-rectangle-is-dead-under-a-laid-out-parent`.
            //
            // Nothing saw it for two reasons worth writing down. The painter's
            // own tests lay a form out and read the geometry, which is right
            // about the rectangle and says nothing about where the run went;
            // and the ink gate that WOULD have caught it runs over a screen,
            // and no screen in this tree had a boolean row in the form it
            // opens with until R1842 gave one two. The check existed, the
            // population did not.
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    boolean_word(field).to_owned(),
                    Rect::default(),
                    form_run_style()
                        .with_size_px(BOOL_WORD_PX)
                        .with_fg(theme.resolve(ColorRole::OnSurface)),
                ))])
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_absolute_position(word_x, CONTROL_FRAME)
                        // The room that is left, not a constant. An 80 px box
                        // was here, and a box that does not follow the control
                        // it sits in is a second answer to how much room the
                        // word has.
                        //
                        // ★ R1842 — the HEIGHT is the control's content height
                        // and the run is centred in it, where a 14 px line box
                        // was written down beside a 12 px face. That number was
                        // a second answer to how tall a line is, and it was too
                        // small: the run laid out four pixels below the box
                        // that was supposed to hold it. A line box is the face
                        // size plus what the face needs under it, so the only
                        // honest way to reserve it is to let the run say.
                        .with_size(Size::px(word_w, inner.h)),
                ),
            ),
        ])
        .with_tag(address::control(tag_prefix, &row.key))
        .with_style(control_skin(worst, theme))
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
        field.value().into_owned(),
        Rect::new(10, 8, row.control.w.saturating_sub(STEP_W * 2 + 16), 14),
        form_run_style()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ))];
    for (suffix, glyph) in [("down", "-"), ("up", "+")] {
        let name = format!("step.{}.{suffix}", row.key);
        children.push(part_pill(
            address::part(tag_prefix, &name),
            glyph,
            muted,
            part_seat(row, &name),
            (row.control.x, row.control.y),
            theme,
        ));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(address::control(tag_prefix, &row.key))
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
    let shown = field.value();
    let elements: Vec<&str> = FieldType::elements(&shown).collect();
    let mut children = Vec::new();
    for (n, element) in elements.iter().enumerate() {
        let name = format!("item.{}.{n}", row.key);
        let seat = part_seat(row, &name);
        // ★★★★★ R1717 — an element the derivation contributed wears the
        // read-out's skin, for the reason a whole derived row does: the fill is
        // what an eye reads as "you may type here", and this line cannot be
        // typed away — the link that put it there is still drawn.
        let mine = field.element_source(n).writable();
        children.push(Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                (*element).to_owned(),
                Rect::new(10, 8, seat.w.saturating_sub(20), 14),
                form_run_style().with_size_px(12).with_fg(if mine {
                    theme.resolve(ColorRole::OnSurface)
                } else {
                    theme.resolve(ColorRole::OnSurfaceMuted)
                }),
            ))])
            .with_tag(address::part(tag_prefix, name))
            .with_style(if mine {
                control_skin(None, theme)
            } else {
                derived_skin(theme)
            })
            .with_layout(placed(
                framed(LayoutStyle::new().with_focusable(true)),
                seat,
                (row.control.x, row.control.y),
            )),
        ));
    }
    let add = format!("item.{}.add", row.key);
    children.push(part_pill(
        address::part(tag_prefix, &add),
        "+ one more",
        muted,
        part_seat(row, &add),
        (row.control.x, row.control.y),
        theme,
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(address::control(tag_prefix, &row.key))
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
            field.value().into_owned(),
            Rect::default(),
            form_run_style()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(address::control(tag_prefix, &row.key))
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

/// A derived row's read-out: the value, in muted ink, with no way to enter one.
///
/// ★★ R1716 — it keeps the control's tag and its focus stop. A reader must
/// still be able to reach the value with a keyboard and hear it; what they must
/// not be able to do is type into it, and that is the form's refusal rather
/// than a missing tab stop. The canon draws the same box with a dashed edge —
/// this framework has no dashed stroke, so the difference it carries here is
/// the muted ink and the absence of the surface fill, and the badge beside it
/// is what actually names the state ([[debt-a-derived-row-is-drawn-without-a-dashed-edge]]).
fn derived_control(
    tag_prefix: &str,
    row: &RowBox,
    field: &ConfigField,
    origin: (u32, u32),
    theme: &Theme,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            field.value().into_owned(),
            Rect::default(),
            form_run_style()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(address::control(tag_prefix, &row.key))
        .with_style(derived_skin(theme))
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
            .with_tag(address::child(tag_prefix, "add", key))
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
        // ★★★★★ R1732 — [`type_word`], not `field.ty()`, and a gate is what
        // said so. The badge declares its silence as "my text is that region's
        // name"; the moment the badge started carrying the option count and
        // this sentence did not, the redirect pointed at a node that speaks and
        // says something else — a reader following the label aloud arrives
        // nowhere. One derivation, two readers.
        let mut said = vec![format!("{}, {}", type_word(field), field.applies().wire())];
        // ★★★ R1716 — a reader who cannot see the badge is the reader who most
        // needs what it says: without this they meet a control that answers
        // nothing they type and are told only its type. Measured at 6.11, a
        // locked cell answers 3 of 256 standard roles and none of them is a
        // reason, so this sentence has no counterpart there at all.
        if let Some(from) = field.source().derived_from() {
            // ★★★★★ R1954 — the SAME phrase the badge paints, with a verb in
            // front of it, so label-in-name holds by construction rather than
            // by two authors agreeing. The shared-row count travels here for
            // R1716's reason: the reader who cannot see the badge is the one
            // who most needs what it says, and until this round the count was
            // the one thing that stayed on the badge alone.
            let shared =
                matches!(field.source(), Source::Shared(_)).then(|| field.derived_elements());
            said.push(provenance_sentence(from, shared));
        }
        if let Some(instead) = field.goes().instead() {
            said.push(format!("{instead}, not configuration"));
        }
        // ★★★★★ R2002 — the SAME phrase the defect badge paints, with the
        // sentence after it. R1954 did this for the source badge and stopped
        // there; this badge went on painting `out_of_range` beside a
        // description that never said those words, and a person reading the
        // badge aloud reached nothing. One derivation, two readers.
        for defect in defects.iter().filter(|d| d.key() == row.key) {
            said.push(format!("{}, {}", defect.phrase(), defect.sentence()));
        }
        // ★★★ R1691 — the role is the SHAPE's, not `TextInput` for everything.
        // A boolean row was announced as a text box for its whole life: a
        // reader was told to type into a control that only toggles, which is
        // the accessibility half of a defect a person reported about its ink.
        let control_tag = address::control(tag_prefix, &row.key);
        let mut control = AccessNode::new(control_tag.clone(), control_role(field))
            .with_name(field.key())
            .with_bounds(row.control)
            .with_state(control_state(field))
            .with_value(control_value(field));
        // ★★★★★ R1732 — a collapsed roster says whether it is open, and names
        // the roster it opens. Both are the combo box's own contract, and
        // `expanded` is derived from the geometry rather than passed in: the
        // popup being laid IS the roster being open, so the announcement and
        // the paint cannot disagree about it.
        if matches!(control_role(field), AriaRole::ComboBox) {
            let open = geometry
                .popup
                .as_ref()
                .is_some_and(|popup| popup.key == row.key);
            control = control.with_expanded(open);
            if open {
                control = control.with_controls(address::roster(tag_prefix, &row.key));
            }
        }
        nodes.extend(describedby_region(
            control,
            address::child(tag_prefix, "said", &row.key),
            AriaRole::Status,
            Some(said.join("; ")),
            true,
        ));
        // ★★ R1686 — the seat is a BUTTON to a screen reader, and its name says
        // which row it takes away. A glyph with no accessible name announces as
        // its own character, which for U+00D7 is "multiplication sign" — a
        // reader would be told the row's arithmetic rather than its affordance.
        //
        // ★★ R1716 — and WHICH act, from the seat itself. A seat that takes a
        // row over announced as "remove" would be the same failure one step
        // later: the name is what a reader decides by.
        let seat_tag = address::child(tag_prefix, row.seat.act(), &row.key);
        nodes.push(
            AccessNode::new(seat_tag, AriaRole::Button)
                .with_name(format!("{} {}", row.seat.verb(), field.key()))
                .with_bounds(row.seat.rect()),
        );
        // ★★★ R1691 — every affordance INSIDE the control, from the list the
        // painter and the hit test already share. A stepper, a checkbox, an
        // option chip and a list element are all pressable and all of them were
        // silent: measured on the reference tool's first screen, 15 of the 136
        // regions it painted without announcing were these.
        //
        // Derived from `row.parts` rather than re-enumerated, so a shape that
        // grows an affordance gets a voice in the same act it gets a rectangle.
        //
        // ★★★★★ R1732 — except the chevron. It is not a second affordance: it
        // is the collapsed control's own arrow, and the control above already
        // announces as a combo box carrying the state it draws. A node here
        // would read the same act out twice on every focus move, which is the
        // duplicate `Silence` exists to stop — so the painter declares it
        // folded into the control and this list leaves it out. The two have to
        // agree, and a test below asserts they do.
        //
        // ★★★★★ R1837 — a boolean has no part in this list any more, for the
        // same reason: its mark was not an affordance INSIDE the control, it
        // WAS the control, and the node above already announced a checkbox with
        // the same name and the same bit. Two of them was one thing announced
        // twice, at two rectangles, the second a square inside the first.
        for (suffix, seat) in row.parts.iter().filter(|(s, _)| !s.starts_with("pick.")) {
            nodes.push(part_access_node(tag_prefix, field, suffix, *seat));
        }
    }
    // ★★★★★ R1732 — an open roster, as the listbox the combo box controls.
    // Built from the geometry the paint came from, so a reader is offered
    // exactly the options that are on screen.
    if let Some(popup) = &geometry.popup {
        if let Some(field) = form.field(&popup.key) {
            let shown = field.value();
            nodes.push(
                AccessNode::new(address::roster(tag_prefix, &popup.key), AriaRole::Listbox)
                    .with_name(format!("{} options", popup.key))
                    .with_bounds(popup.rect),
            );
            for (suffix, seat) in &popup.options {
                let word = suffix.rsplit('.').next().unwrap_or_default();
                nodes.push(
                    AccessNode::new(address::part(tag_prefix, suffix), AriaRole::ListBoxOption)
                        .with_name(format!("{word}, {}", popup.key))
                        .with_bounds(*seat)
                        .with_selected(shown.trim() == word),
                );
            }
        }
    }
    // The chips that offer a key the form does not hold yet.
    for (key, seat) in &geometry.chips {
        nodes.push(
            AccessNode::new(address::child(tag_prefix, "add", key), AriaRole::Button)
                .with_name(format!("add {key}"))
                .with_bounds(*seat),
        );
    }
    nodes
}

/// The role a row's control takes, from the shape of what it holds.
///
/// A control announced as the wrong kind is worse than one announced with a
/// poor name: a reader is told what they can *do*, and "text box" on a control
/// that only toggles is an instruction that fails.
fn control_role(field: &ConfigField) -> AriaRole {
    // ★★★★★ R1716 — a derived row announces what it IS, which is a read-out.
    // The rule this function was written under says a control announced as the
    // wrong kind is worse than one with a poor name, because a reader is told
    // what they can *do* — and "radio group" over a row that paints no options
    // and refuses every write is exactly that failure, one axis over. The
    // read-only state on the same node is what says it cannot be typed into;
    // the role is what says what is there.
    if !field.source().writable() {
        return AriaRole::TextInput;
    }
    match field.shape() {
        FieldType::Boolean => AriaRole::CheckBox,
        FieldType::Integer { .. } => AriaRole::SpinButton,
        // ★★★★★ R1732 — exactly one of a fixed set, from a control that is
        // **collapsed**. A radio group was right while every option was on
        // screen: a reader met the whole roster and picking one un-picked
        // another. It is wrong now — there is nothing to move between until the
        // roster is opened, and a reader told "radio group" would go looking
        // for members that are not there. A combo box is the role whose
        // contract is exactly this one: a value, and a roster that appears.
        FieldType::Choice { .. } => AriaRole::ComboBox,
        // A plain group in both cases, and for one reason: what the shape
        // paints is a row of independent controls rather than members of a
        // collection. `Flags` paints checkboxes; `List` paints editable text
        // boxes and an add button.
        //
        // ★★★ R1693 — `List` was `AriaRole::List` until `scene/conform` asked
        // what the list HELD. A WAI-ARIA `list` promises `listitem`s a reader
        // moves through, so the role announced a collection whose members were a
        // different kind of thing — and a field with no entries yet announced a
        // collection with nothing in it at all. Measured on the reference tool's
        // first screen, where two endpoint fields open empty.
        FieldType::Flags { .. } | FieldType::List { .. } => AriaRole::Group,
        FieldType::Text | FieldType::Formatted { .. } => AriaRole::TextInput,
    }
}

/// The value a row's control announces.
///
/// A boolean announces the BIT rather than the word, because `aria-checked` is
/// what a reader's toggle command reads; everything else announces its text.
fn control_value(field: &ConfigField) -> AccessValue {
    match field.shape() {
        FieldType::Boolean => AccessValue::Bool(field.value().trim() == "true"),
        _ => AccessValue::Text(field.value().into_owned()),
    }
}

/// The state a row's control announces — the checked bit for a boolean, so the
/// role and the state agree.
fn control_state(field: &ConfigField) -> AccessState {
    let mut state = AccessState::default();
    if matches!(field.shape(), FieldType::Boolean) {
        state.checked = Some(field.value().trim() == "true");
    }
    // ★★★ R1716 — `read_only` and not `disabled`, which is the distinction
    // R1544 wrote this field's documentation around: a derived row is fully
    // reachable, its value is worth hearing and copying, and it simply refuses
    // to change. Marking it disabled would take it out of a reader's walk
    // entirely — the value they came for.
    state.read_only = !field.source().writable();
    state
}

/// One affordance inside a control, named from the suffix the painter gave it.
///
/// The suffix vocabulary is [`RowBox::parts`]' own — `option.<key>.<word>`,
/// `step.<key>.up`, `toggle.<key>`, `item.<key>.<n>`, `item.<key>.add` — and it
/// is matched on its LEADING word so a key containing a dot (which every
/// configuration path does) cannot be mistaken for a shape.
fn part_access_node(tag_prefix: &str, field: &ConfigField, suffix: &str, seat: Rect) -> AccessNode {
    let tag = address::part(tag_prefix, suffix);
    let key = field.key();
    let last = suffix.rsplit('.').next().unwrap_or(suffix);
    let (role, name, checked) = match suffix.split('.').next().unwrap_or("") {
        // An option: a radio when exactly one may be chosen, a checkbox when
        // any subset may. The chosen set is the field's own value.
        "option" => {
            let on = FieldType::elements(&field.value()).any(|word| word == last);
            let role = if matches!(field.shape(), FieldType::Choice { .. }) {
                AriaRole::RadioButton
            } else {
                AriaRole::CheckBox
            };
            (role, format!("{last}, {key}"), Some(on))
        }
        // The checkbox of a boolean row. Its state is the control's, so it is
        // announced as the same bit rather than as a second opinion.
        "toggle" => (
            AriaRole::CheckBox,
            key.to_owned(),
            Some(field.value().trim() == "true"),
        ),
        // A stepper. "up"/"down" alone would announce as a direction with no
        // subject, which is what a reader hears when they land on it.
        "step" => (
            AriaRole::Button,
            format!(
                "{} {key}",
                if last == "up" { "increase" } else { "decrease" }
            ),
            None,
        ),
        // A list: the seat that appends, then the elements.
        "item" if last == "add" => (AriaRole::Button, format!("add one more to {key}"), None),
        "item" => (
            AriaRole::TextInput,
            format!("{key} element {}", element_ordinal(last)),
            None,
        ),
        // An unrecognised suffix still gets a voice — silence is the failure
        // this whole census exists to prevent, so the fallback is a named node
        // rather than a skip.
        _ => (AriaRole::Button, format!("{last}, {key}"), None),
    };
    let mut node = AccessNode::new(tag, role).with_name(name).with_bounds(seat);
    if let Some(on) = checked {
        node = node.with_state(AccessState {
            checked: Some(on),
            ..AccessState::default()
        });
    }
    if suffix.starts_with("item.") && last != "add" {
        let at = last.parse::<usize>().unwrap_or(usize::MAX);
        let shown = field.value();
        let element = FieldType::elements(&shown).nth(at).unwrap_or_default();
        node = node.with_value(AccessValue::Text(element.to_owned()));
        // ★★★ R1717 — and an element the derivation contributed says it is a
        // read-out and what worked it out, the way a whole derived row does. A
        // reader who cannot see that its box has no fill has no other way to
        // learn that this one line will not take an edit. It goes in the NAME
        // rather than through a described-by region: a region per element would
        // put a status node beside every line of every list on the screen, and
        // what a reader needs here is one clause.
        if let Some(from) = field.element_source(at).derived_from() {
            // ★ R1954 — the third site that spelled this clause. An element
            // carries no badge of its own, so there is nothing here for
            // label-in-name to compare — but a second spelling of one clause is
            // what let the first two drift, and the count of sites was three.
            node = node
                .with_name(format!(
                    "{key} element {}, {}",
                    element_ordinal(last),
                    provenance_sentence(from, None)
                ))
                .with_state(AccessState {
                    read_only: true,
                    ..AccessState::default()
                });
        }
    }
    node
}

/// A list element's position as a person counts them, from the zero-based index
/// the tag carries.
///
/// One-based because the number is read out loud: "element 0" is an index and
/// "element 1" is the first one.
fn element_ordinal(index: &str) -> usize {
    index.parse::<usize>().unwrap_or(0).saturating_add(1)
}

/// What the status region for `key` says, for a caller checking a claim about
/// what a reader is told rather than about what is on screen.
#[must_use]
pub fn row_description(nodes: &[AccessNode], tag_prefix: &str, key: &str) -> Option<String> {
    let tag = address::child(tag_prefix, "said", key);
    nodes
        .iter()
        .find(|n| n.tag == tag)
        .and_then(|n| n.name.clone())
}

#[cfg(test)]
mod tests {
    /// ★★★★★ R2050 — **this painter composes a control's address in ONE
    /// place**, and this counts.
    ///
    /// The debt behind it is that an address had no declaring site: the
    /// producing sites each spelled one and every consumer spelled it again, so
    /// one wrong letter compiled, painted, and made every query looking for the
    /// mark answer nothing. Eight sites in this file alone composed it.
    ///
    /// ⚠ The needle is assembled rather than written, because this file is the
    /// one being read — a gate that counts by reading source has its own source
    /// in the population, and assembling is what puts it there on the same
    /// terms as the rest rather than excusing it by name.
    #[test]
    fn r2050_a_form_part_address_is_composed_in_one_place() {
        // ★★★★★ R2052 — WIDENED from the control family to the whole
        // namespace. R2050 moved one family and left twenty-two compositions of
        // the same shape beside it, each free to spell a separator or a part
        // its own way; the needle is the prefix interpolation itself now, so
        // every part of a form is composed in one place or this refuses.
        const NEEDLE: &str = concat!("{tag_", "prefix}.");
        const BODY: &str = include_str!("config_form.rs");
        assert_eq!(
            BODY.matches(NEEDLE).count(),
            0,
            "★★★★★ a form part's address is composed by `address::child` and \
             `address::part`; this file composes one itself"
        );
        // ★ And the composition round-trips through its own inverse, which is
        // the half a consumer's router would otherwise spell a second time.
        let tag = super::address::control("lab.form", "listen.endpoints");
        assert_eq!(tag, "lab.form.control.listen.endpoints");
        assert_eq!(
            super::address::control_key("lab.form", &tag),
            Some("listen.endpoints")
        );
        assert_eq!(
            super::address::control_key("other.form", &tag),
            None,
            "★ an address under another form's prefix is not this form's row"
        );
        assert_eq!(
            super::address::roster("lab.form", "mode"),
            "lab.form.roster.mode"
        );
        assert_eq!(super::address::part("lab.form", "body"), "lab.form.body");
    }

    /// ★★★★★ R2020 — **which state each applies-scope is painted in, PINNED
    /// against the behaviour canon.**
    ///
    /// This is a re-spelling of [`applies_state`] and it is deliberate, because
    /// the source it is checked against is not in this tree. The consumer gates
    /// derive their expectation from that function, so they are silent about
    /// whether the mapping is RIGHT — they only check the screen agrees with
    /// it. Something has to hold the mapping itself, and what it is held to is a
    /// measurement of the canon's own inspector: its policy chip is drawn `HOT`
    /// on the right-state ground and `RESTART` on the caution one.
    ///
    /// ⚠ And a negative, which is the half a pin usually omits: neither is
    /// `Accent`'s family. `HOT` was painted in the accent until this round —
    /// this vocabulary's INTERACTIVE tone — so the badge saying *this edit lands
    /// immediately* read as a thing to press.
    #[test]
    fn r2020_each_applies_scope_is_painted_in_the_state_the_canon_uses() {
        use pinion_core::theme::StateTone;

        assert_eq!(super::applies_state(Applies::Hot), StateTone::Success);
        assert_eq!(super::applies_state(Applies::Restart), StateTone::Warning);
        // The population, so an arm added to `Applies` is unpinned loudly.
        assert_eq!(Applies::ALL.len(), 2);
        for scope in Applies::ALL {
            let tone = super::applies_state(scope);
            assert_ne!(
                tone.container(),
                pinion_core::theme::ColorRole::Accent,
                "`{}` is painted in the interactive tone's family, which says \
                 *press me* about a read-out",
                scope.wire()
            );
        }
    }

    /// ★★★★★ R2020 — **a defect that stops a launch and one that does not are
    /// not painted alike.**
    ///
    /// The claim R1651 argued the caution tier into existence for, asserted at
    /// last on the thing a person sees. Until this round both were an ink on the
    /// shared raised tier, so the two severities differed by the colour of eight
    /// small letters; a reader scanning a form for *what stops me* had to read
    /// each badge.
    ///
    /// Stated as a partition over [`ConfigDefect::all`] rather than two
    /// equalities, so it is about the vocabulary rather than about three
    /// particular arms: a fourth defect joining either side inherits the claim.
    #[test]
    fn r2020_a_blocking_defect_is_not_painted_like_one_that_warns() {
        use pinion_core::widgets::config_form::ConfigDefect;
        use std::collections::BTreeSet;

        let mut blocking: BTreeSet<&str> = BTreeSet::new();
        let mut warning: BTreeSet<&str> = BTreeSet::new();
        for defect in ConfigDefect::all() {
            let word = super::defect_state(&defect).word();
            if defect.blocks() {
                blocking.insert(word);
            } else {
                warning.insert(word);
            }
        }
        assert!(
            !blocking.is_empty() && !warning.is_empty(),
            "one side is empty, so the partition asserts nothing: \
             {blocking:?} / {warning:?}"
        );
        assert!(
            blocking.is_disjoint(&warning),
            "a defect that stops a launch is painted the same as one that does \
             not: {blocking:?} / {warning:?}"
        );
        // And which side is which, because *different* is not enough: the one
        // that stops you is the wrong-state tone.
        assert_eq!(blocking, ["error"].into_iter().collect::<BTreeSet<_>>());
        assert_eq!(warning, ["warning"].into_iter().collect::<BTreeSet<_>>());
    }

    /// A form holding one choice row over `words`, for the picker checks.
    fn choosing(words: &[&'static str]) -> ConfigForm {
        ConfigForm::new(
            vec![
                ConfigField::new("severity", "level", Applies::Hot, words[0]).with_shape(
                    FieldType::Choice {
                        of: words.iter().map(|w| (*w).into()).collect(),
                    },
                ),
            ],
            Vec::new(),
        )
    }

    /// The room a pane the form's own size would give it.
    const ROOMY: Rect = Rect::new(0, 0, 400, 900);

    /// ★★★★★ R1732 — **the defect this round is about, as a number.**
    ///
    /// Before this round a choice row laid one chip per option from the
    /// control's left edge, with no wrap, no clip and no scroll: measured, a
    /// six-word roster ran 50 px past its own control and a seven-word one 113
    /// px, while the analysis tool's own three-word row spent 229 px of a 284
    /// px pane saying one word.
    ///
    /// The collapsed control cannot do that at any length, and the assertion is
    /// written over a range so a roster nobody thought of is covered too.
    #[test]
    fn r1732_a_choice_row_stays_inside_its_control_however_long_the_roster() {
        let style = FormStyle::default();
        for n in 1..=12 {
            let words: Vec<&'static str> = [
                "trace", "debug", "info", "warn", "error", "fatal", "silent", "verbose", "audit",
                "notice", "alert", "panic",
            ][..n]
                .to_vec();
            let form = choosing(&words);
            let geometry = form_geometry(&form, (14, 40), &style);
            let row = geometry.row("severity").expect("shown");
            for (suffix, rect) in &row.parts {
                assert!(
                    rect.x >= row.control.x && rect.x + rect.w <= row.control.x + row.control.w,
                    "{suffix} with {n} options is laid outside its own control \
                     ({rect:?} vs {:?})",
                    row.control,
                );
            }
            assert_eq!(
                row.control.w, style.width,
                "★ and the control is one field wide whatever the roster holds",
            );
        }
    }

    /// ★★★★★ R1732 — the roster is a **layer**, and the layering is a declared
    /// fact rather than a consequence of what a hit test happens to iterate
    /// first.
    #[test]
    fn r1732_an_open_roster_is_over_the_rows_it_covers() {
        let style = FormStyle::default();
        let form = ConfigForm::new(
            vec![
                ConfigField::new("severity", "level", Applies::Hot, "warn").with_shape(
                    FieldType::Choice {
                        of: vec!["info".into(), "warn".into(), "error".into()],
                    },
                ),
                ConfigField::new("note", "text", Applies::Hot, "anything"),
            ],
            Vec::new(),
        );
        let picker = Picker::over(["info", "warn", "error"], "warn").expect("a roster");
        let geometry = form_geometry_showing(
            &form,
            (14, 40),
            &style,
            Some(OpenPicker {
                key: "severity",
                picker: &picker,
                room: ROOMY,
            }),
        );
        let popup = geometry.popup.as_ref().expect("a roster is open");
        assert!(!popup.above, "there is room below");
        assert_eq!(popup.options.len(), 3);
        let under = geometry.row("note").expect("shown");
        let (_, second) = &popup.options[1];
        assert!(
            second.y >= under.row.y && second.y < under.row.y + under.row.h,
            "★ the roster really does cover the row below — {second:?} over {:?}",
            under.row,
        );
        assert_eq!(
            geometry.option_at(second.x + 2, second.y + 2),
            Some("option.severity.warn"),
            "★★ and the layer answers first, under the name the press always had",
        );
        assert!(
            geometry.on_popup(popup.rect.x + 1, popup.rect.y + 1),
            "★ a press between two options is still the roster's",
        );
        assert!(!geometry.on_popup(under.row.x, under.row.y + under.row.h + 40));
    }

    /// The direction is derived from the room, not chosen — and both answers
    /// are checked, because a flip that never fires is a flip nobody has.
    #[test]
    fn r1732_a_roster_with_no_room_below_opens_upward() {
        let style = FormStyle::default();
        let form = choosing(&["info", "warn", "error", "fatal"]);
        let picker = Picker::over(["info", "warn", "error", "fatal"], "info").expect("a roster");
        let open = |room: Rect| {
            form_geometry_showing(
                &form,
                (14, 200),
                &style,
                Some(OpenPicker {
                    key: "severity",
                    picker: &picker,
                    room,
                }),
            )
        };
        let roomy = open(ROOMY);
        let popup = roomy.popup.as_ref().expect("open");
        let row = roomy.row("severity").expect("shown");
        assert!(!popup.above);
        assert!(popup.rect.y > row.control.y);

        let cramped = open(Rect::new(0, 0, 400, 260));
        let popup = cramped.popup.as_ref().expect("open");
        assert!(popup.above, "★ no room below, so it opens up");
        assert!(
            popup.rect.y + popup.rect.h < row.control.y,
            "★★ and it is wholly above the control, not overlapping it",
        );
    }

    /// A shift moves the roster with everything else — the property R1662 wrote
    /// for the rows, which a new layer would otherwise quietly not have.
    #[test]
    fn r1732_a_shift_moves_the_roster_too() {
        let form = choosing(&["info", "warn"]);
        let picker = Picker::over(["info", "warn"], "info").expect("a roster");
        let local = form_geometry_showing(
            &form,
            (14, 40),
            &FormStyle::default(),
            Some(OpenPicker {
                key: "severity",
                picker: &picker,
                room: ROOMY,
            }),
        );
        let moved = local.translated(300, -10);
        let here = local.popup.as_ref().expect("open");
        let there = moved.popup.as_ref().expect("still open");
        assert_eq!(there.rect.x, here.rect.x + 300);
        assert_eq!(there.rect.y, here.rect.y - 10);
        assert_eq!(there.options.len(), here.options.len());
        assert_eq!(there.options[1].1.x, here.options[1].1.x + 300);
    }

    /// A key that names a row with no roster describes a state the form cannot
    /// be in, and nothing is invented for it.
    #[test]
    fn r1732_a_row_with_no_roster_opens_none() {
        let form = ConfigForm::new(
            vec![ConfigField::new("note", "text", Applies::Hot, "anything")],
            Vec::new(),
        );
        let picker = Picker::over(["info"], "info").expect("a roster");
        let geometry = form_geometry_showing(
            &form,
            (14, 40),
            &FormStyle::default(),
            Some(OpenPicker {
                key: "note",
                picker: &picker,
                room: ROOMY,
            }),
        );
        assert!(geometry.popup.is_none());
        assert_eq!(geometry.option_at(20, 60), None);
    }

    /// ★★★★ The roster shows two different facts and must not derive one from
    /// the other: **where the reader is** and **what the document holds**.
    #[test]
    fn r1732_the_roster_says_which_option_is_held_and_which_is_highlighted() {
        let form = choosing(&["info", "warn", "error"]);
        let mut picker = Picker::over(["info", "warn", "error"], "info").expect("a roster");
        picker.key("ArrowDown");
        picker.key("ArrowDown");
        assert_eq!(picker.highlighted(), "error");
        let geometry = form_geometry_showing(
            &form,
            (14, 40),
            &FormStyle::default(),
            Some(OpenPicker {
                key: "severity",
                picker: &picker,
                room: ROOMY,
            }),
        );
        let nodes = row_access_nodes("f", &form, &geometry);
        let control = nodes
            .iter()
            .find(|n| n.tag == "f.control.severity")
            .expect("announced");
        assert_eq!(control.expanded, Some(true));
        assert_eq!(control.controls.as_deref(), Some("f.roster.severity"));
        let held = nodes
            .iter()
            .find(|n| n.tag == "f.option.severity.info")
            .expect("announced");
        assert_eq!(
            held.selected,
            Some(true),
            "★ the document still holds `info` — moving is not writing",
        );
        let moved_to = nodes
            .iter()
            .find(|n| n.tag == "f.option.severity.error")
            .expect("announced");
        assert_eq!(moved_to.selected, Some(false));
        // And a shut control says so rather than saying nothing.
        let shut = row_access_nodes(
            "f",
            &form,
            &form_geometry(&form, (14, 40), &FormStyle::default()),
        );
        let control = shut
            .iter()
            .find(|n| n.tag == "f.control.severity")
            .expect("announced");
        assert_eq!(control.expanded, Some(false));
        assert_eq!(control.controls, None);
        assert!(
            !shut.iter().any(|n| n.tag == "f.roster.severity"),
            "★★ and no roster is announced when none is drawn",
        );
    }

    /// ★★★ R1732 — the count the reference puts on the type badge, which a
    /// collapsed control is exactly what makes necessary.
    #[test]
    fn r1732_the_type_badge_says_how_many_words_are_on_offer() {
        for (shape, want) in [
            (
                FieldType::Choice {
                    of: vec!["a".into(), "b".into(), "c".into()],
                },
                "level \u{b7} 3",
            ),
            (
                FieldType::Flags {
                    of: vec!["read".into(), "write".into()],
                },
                "level \u{b7} 2",
            ),
            (FieldType::Text, "level"),
        ] {
            let field = ConfigField::new("k", "level", Applies::Hot, "a").with_shape(shape);
            assert_eq!(super::type_word(&field), want);
        }
    }

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
        // R1686 — the remove seat is a rectangle like the others and moves like
        // one. It is asserted by name because it is the one that is NOT in
        // `parts`, and a field a translation forgets is a press that lands on
        // the row above it.
        assert_eq!(b.seat.rect().x, a.seat.rect().x + 300);
        assert_eq!(b.seat.rect().y, a.seat.rect().y - 10);
        assert_eq!(
            (b.seat.rect().w, b.seat.rect().h),
            (a.seat.rect().w, a.seat.rect().h)
        );
        // ★ R1716 — and it is still the same ACT after the move. A translation
        // that answered `Remove` for a row the form will not remove would put
        // the press back where R1686 took it from.
        assert_eq!(b.seat.verb(), a.seat.verb());
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
        // ★ R1797 — the framework's own stand-in, not a third copy of it. This
        // held a byte-identical duplicate of `screen_ink::stand_in_ink`, and it
        // duplicated the defect too: both asked `TextOverflow::shortens()` —
        // which is about the CONTENT — to decide how far the INK reaches, so a
        // clipped run's hidden glyphs counted as ink outside its box. Fixing it
        // in one place and leaving a copy behind is how this tree grew three
        // private quantiles, which the same round had to lift.
        let escapes = pinion_core::containment::escapes(
            &scene,
            &mut pinion_core::test_fixtures::screen_ink::stand_in_ink,
        );
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
    /// ★★★★★ R1686 — **the same form lays out the same way whether or not the
    /// caller stands in an owner scope.**
    ///
    /// This module's header claims [`form_geometry`] is "the single source both
    /// [`view_config_form`] and a consumer's hit test read", and until this test
    /// that claim was false in a way no gate could see: the widths come from
    /// `measured_text_extent`, which answers `None` **outside an `Owner`
    /// scope** and falls back to a per-character estimate. A shell paints
    /// inside the scope and routes a pointer outside it, so the two passes
    /// wrapped the chip row differently — and a chip's rect ended up under a
    /// different chip's tag.
    ///
    /// Measured on the running analyser screen, which is how it was found: the
    /// estimate makes `qos.priority` 99px and the shaper makes it 92px, so the
    /// hit test fitted one fewer chip per row, and pressing the chip painted
    /// for `control.permissions` added `plugins`. The seat this round built is
    /// what made it reachable — every removal offers one more key, and one more
    /// chip is what re-wraps the row.
    ///
    /// The repair is [`pinion_core::measured_text_extent`]'s, not this file's:
    /// the scope that HAS the provider records it, and a reader outside any
    /// scope measures with the same face. That is R1684.4's shape exactly, one
    /// axis over ([[debt-a-widget-cannot-read-its-own-size-outside-a-scope]]).
    #[test]
    fn r1686_a_form_lays_out_the_same_inside_and_outside_an_owner_scope() {
        use pinion_core::{Owner, TEXT_METRICS, TextExtent, TextMetrics};
        use std::rc::Rc;

        /// A face whose advance is nothing like the per-character estimate, so
        /// "the seam was consulted" is distinguishable from "the fallback ran".
        #[derive(Debug)]
        struct Narrow;
        impl TextMetrics for Narrow {
            fn measure(
                &self,
                text: &str,
                style: &pinion_core::style::TextStyle,
                max_width: Option<u32>,
            ) -> Option<TextExtent> {
                let w = u32::try_from(text.chars().count()).unwrap_or(0) * 3;
                Some(TextExtent::new(
                    max_width.map_or(w, |m| w.min(m)),
                    style.font_size_px,
                ))
            }
        }

        let style = FormStyle::default();
        let owner = Owner::new();
        TEXT_METRICS.provide(&owner, Rc::new(Narrow));
        let inside = owner.run(|| form_geometry(&inspector(), (14, 40), &style));
        let outside = form_geometry(&inspector(), (14, 40), &style);

        assert_eq!(
            inside.chips.len(),
            outside.chips.len(),
            "the same form offers the same keys either way"
        );
        assert_eq!(
            inside.chips, outside.chips,
            "★★★ a chip's rectangle is where it is painted, whoever asks. The \
             paint runs inside the scope and a pointer arrives outside it, so \
             a difference here IS a press that lands on the wrong key"
        );
        assert_eq!(
            inside.rows.iter().map(|r| r.seat).collect::<Vec<_>>(),
            outside.rows.iter().map(|r| r.seat).collect::<Vec<_>>(),
            "★ and so is the seat that takes a row away"
        );
        assert_eq!(inside.height, outside.height, "and so is the whole form");
    }

    /// ★★ R1686 — the seat that takes a row away is inside its row, clear of
    /// the header the badges are laid into, and clear of the control.
    ///
    /// Under **both** wrap policies, because they place the header differently:
    /// wrapped it spans the pane and the control is below, beside it is the key
    /// column and the control is to the right. A seat cut from the wrong edge
    /// is invisible in one of the two and covers a control in the other.
    #[test]
    fn r1686_the_remove_seat_is_clear_of_the_header_and_the_control() {
        for wrap in [RowWrap::WrapAll, RowWrap::Beside, RowWrap::WrapLong] {
            let style = FormStyle::default().with_policy(wrap, FieldGrowth::AllGrow);
            let geometry = form_geometry(&inspector(), (14, 40), &style);
            assert!(!geometry.rows.is_empty());
            for row in &geometry.rows {
                let seat = row.seat.rect();
                assert!(seat.w > 0 && seat.h > 0, "{wrap:?} {} has no seat", row.key);
                let inside = |a: pinion_core::scene::Rect, b: pinion_core::scene::Rect| {
                    a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h
                };
                assert!(
                    inside(seat, row.row),
                    "{wrap:?} {}: seat {seat:?} left the row {:?}",
                    row.key,
                    row.row
                );
                let overlaps = |a: pinion_core::scene::Rect, b: pinion_core::scene::Rect| {
                    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
                };
                assert!(
                    !overlaps(seat, row.header),
                    "{wrap:?} {}: seat {seat:?} is on the header {:?}, whose \
                     badges are laid out by the flex pass and would be painted \
                     under it",
                    row.key,
                    row.header
                );
                assert!(
                    !overlaps(seat, row.control),
                    "{wrap:?} {}: seat {seat:?} is on the control {:?}",
                    row.key,
                    row.control
                );
            }
        }
    }

    /// ★★ R1686 — **the affordance does not lie**: every row the geometry lays
    /// a seat for is a row [`ConfigForm::remove`] accepts.
    ///
    /// The seat is drawn unconditionally today because the form holds the row
    /// and removing a held row therefore succeeds. That is a *derivation*, and
    /// this is what keeps it one: a refusal added to `remove` without the
    /// painter learning about it turns every seat into a press that does
    /// nothing, which is the failure mode a boolean flag on the field would
    /// have made silent.
    #[test]
    fn r1686_every_row_with_a_seat_is_a_row_the_form_will_remove() {
        let mut form = inspector();
        let geometry = form_geometry(&form, (14, 40), &FormStyle::default());
        let keys: Vec<String> = geometry.rows.iter().map(|r| r.key.clone()).collect();
        assert!(keys.len() > 1, "the fixture has rows to remove");
        for key in keys {
            assert_eq!(
                form.remove(&key),
                Ok(()),
                "a seat was painted for {key} and the form refused it"
            );
        }
    }

    /// A form whose middle row the screen works out for itself, plus a row that
    /// is about where the node runs rather than about its configuration.
    fn with_derived() -> ConfigForm {
        ConfigForm::new(
            vec![
                ConfigField::new("id", "id", Applies::Restart, "a1"),
                ConfigField::new("mode", "mode", Applies::Restart, "peer")
                    .with_shape(FieldType::Choice {
                        of: vec!["peer".into(), "client".into(), "router".into()],
                    })
                    .derived_from("role"),
                ConfigField::new("connect.endpoints", "locator[]", Applies::Hot, "t/2.1:3")
                    .with_shape(FieldType::List {
                        of: Box::new(FieldType::Text),
                    })
                    .derived_from("wire"),
                ConfigField::new("host", "text", Applies::Restart, "10.0.0.2")
                    .derived_from("kind default")
                    .goes_aside("placement"),
            ],
            Vec::new(),
        )
    }

    /// Every tag the painter wrote, in paint order.
    fn painted_tags(scene: &Scene) -> Vec<String> {
        let mut tags = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let Some(tag) = visit.node.tag() {
                tags.push(tag.to_owned());
            }
        });
        tags
    }

    /// ★★★★★ R1716 — the seat offers the act that is actually available.
    #[test]
    fn r1716_a_derived_rows_seat_takes_it_over_rather_than_away() {
        let form = with_derived();
        let geometry = form_geometry(&form, (14, 40), &FormStyle::default());
        let seats: Vec<(&str, &str)> = geometry
            .rows
            .iter()
            .map(|row| (row.key.as_str(), row.seat.verb()))
            .collect();
        assert_eq!(
            seats,
            [
                ("id", "remove"),
                ("mode", "take over"),
                ("connect.endpoints", "take over"),
                ("host", "take over"),
            ],
            "★ the seat is derived from who owns the value, not from a flag"
        );
        let nodes = row_access_nodes("f", &form, &geometry);
        let seat = nodes
            .iter()
            .find(|n| n.tag == "f.author.mode")
            .expect("the take-over seat is announced");
        assert_eq!(seat.role, pinion_a11y::AriaRole::Button);
        assert_eq!(seat.name.as_deref(), Some("take over mode"));
        assert!(
            !nodes.iter().any(|n| n.tag == "f.remove.mode"),
            "★ and it is NOT announced as a remove — a reader decides by the name"
        );
        let painted = painted_tags(&view_config_form(
            "f",
            &form,
            &geometry,
            &pinion_core::Theme::dark(),
        ));
        assert!(painted.contains(&"f.author.mode".to_owned()));
        assert!(painted.contains(&"f.remove.id".to_owned()));
        assert!(
            !painted.contains(&"f.remove.mode".to_owned()),
            "★★ the press the form would refuse is not painted anywhere: {painted:?}"
        );
    }

    /// ★★★★★ R1716 — no way to write into a value nobody owns. The chips, the
    /// toggle and the stepper are all invitations the form refuses.
    #[test]
    fn r1716_a_derived_row_paints_no_way_to_write_into_it() {
        let form = with_derived();
        let geometry = form_geometry(&form, (14, 40), &FormStyle::default());
        let mode = geometry.row("mode").expect("shown");
        assert!(
            mode.parts.is_empty(),
            "★ a Choice row would otherwise paint one chip per option: {:?}",
            mode.parts
        );
        let list = geometry.row("connect.endpoints").expect("shown");
        assert!(list.parts.is_empty(), "and a list its element boxes");
        assert_eq!(
            list.control.h,
            FormStyle::default().control_h,
            "★ a read-out is one line, not a column of editing rows"
        );
        let nodes = row_access_nodes("f", &form, &geometry);
        let control = nodes
            .iter()
            .find(|n| n.tag == "f.control.mode")
            .expect("announced");
        assert!(
            control.state.read_only,
            "★★ read-only and not disabled — the value is still worth hearing"
        );
        assert!(!control.state.disabled);
        assert!(
            nodes
                .iter()
                .find(|n| n.tag == "f.control.id")
                .is_some_and(|n| !n.state.read_only),
            "and a row somebody wrote says nothing of the kind"
        );
    }

    /// ★★★★★ R1716 — **a read-out is not painted as a place to type**, and
    /// this pins the decision rather than a colour.
    ///
    /// The reason it exists is the whole reason it is starred: the first draft
    /// asked the theme for the panel's own tone and got a box BRIGHTER than the
    /// editable rows beside it — measured off a photograph of the real screen,
    /// `(255,255,255)` against `(236,230,240)` on a panel of `(22,24,29)`. Every
    /// test in this file was green, because none of them had ever looked at a
    /// fill. A theme is free to resolve its roles however it likes; what must
    /// not vary is that the row nobody may write into has **no fill at all**.
    #[test]
    fn r1716_a_derived_row_is_not_painted_as_a_place_to_type() {
        let form = with_derived();
        let geometry = form_geometry(&form, (14, 40), &FormStyle::default());
        for theme in [pinion_core::Theme::dark(), pinion_core::Theme::light()] {
            let scene = view_config_form("f", &form, &geometry, &theme);
            let fill = |tag: &str| -> Option<pinion_core::style::Color> {
                let mut found = None;
                scene.for_each_node(&mut |visit| {
                    if visit.node.tag() == Some(tag) {
                        if let Scene::Container(node) = visit.node {
                            found = Some(node.style.fill);
                        }
                    }
                });
                found
            };
            assert_eq!(
                fill("f.control.mode"),
                Some(pinion_core::style::Color::TRANSPARENT),
                "★ a value nobody may write into wears no fill",
            );
            let authored = fill("f.control.id").expect("the authored row is painted");
            assert_ne!(
                authored,
                pinion_core::style::Color::TRANSPARENT,
                "★★ and the row somebody CAN type into does — otherwise the \
                 distinction is a badge and nothing else",
            );
        }
    }

    /// ★★★ R1716 — the badge says WHERE FROM, and the badge that answers a
    /// question this row cannot be asked is not painted.
    #[test]
    fn r1716_a_derived_row_shows_the_source_where_the_cost_of_an_edit_would_be() {
        let form = with_derived();
        let geometry = form_geometry(&form, (14, 40), &FormStyle::default());
        let painted = painted_tags(&view_config_form(
            "f",
            &form,
            &geometry,
            &pinion_core::Theme::dark(),
        ));
        assert!(painted.contains(&"f.source.mode".to_owned()));
        assert!(
            !painted.contains(&"f.applies.mode".to_owned()),
            "★ a restart-scoped row nobody can edit does not say what an edit \
             would cost: {painted:?}"
        );
        assert!(
            painted.contains(&"f.applies.connect.endpoints".to_owned()),
            "★★ but a LIVE one still does — its value reaches the running node \
             when its source moves, which is the canon's own rule"
        );
        assert!(
            painted.contains(&"f.aside.host".to_owned())
                && !painted.contains(&"f.aside.mode".to_owned()),
            "★ and 'this is not configuration' is a separate badge on a \
             separate axis: {painted:?}"
        );
        assert!(
            !painted.contains(&"f.source.id".to_owned()),
            "a row somebody wrote has no source badge"
        );
        let said = row_description(&row_access_nodes("f", &form, &geometry), "f", "mode")
            .expect("described");
        assert!(
            said.contains("worked out from the role"),
            "★★★ and a reader who cannot see the badge is told the same thing: {said}"
        );
        let aside = row_description(&row_access_nodes("f", &form, &geometry), "f", "host")
            .expect("described");
        assert!(aside.contains("placement, not configuration"), "{aside}");
    }

    /// ★ R1686 — the seat is a named button in the tree, not a bare glyph.
    #[test]
    fn r1686_the_remove_seat_announces_which_row_it_takes_away() {
        let form = inspector();
        let geometry = form_geometry(&form, (14, 40), &FormStyle::default());
        let nodes = row_access_nodes("f", &form, &geometry);
        for row in &geometry.rows {
            let tag = format!("f.remove.{}", row.key);
            let node = nodes
                .iter()
                .find(|n| n.tag == tag)
                .unwrap_or_else(|| panic!("no access node at {tag}"));
            assert_eq!(node.role, pinion_a11y::AriaRole::Button);
            assert_eq!(node.name.as_deref(), Some(&*format!("remove {}", row.key)));
            assert_eq!(
                node.bounds,
                Some(row.seat.rect()),
                "and it says where it is"
            );
        }
    }

    use pinion_core::widgets::config_form::{Applies, ConfigField, ConfigForm, FieldType};
    use pinion_core::widgets::text_format::{CharClass, CharSet, Span, TextFormat};

    use pinion_core::Scene;

    use pinion_core::widgets::picker::Picker;

    use super::{
        CONTROL_FRAME, FieldGrowth, FormStyle, OpenPicker, Rect, RowWrap, Theme, control_frame,
        form_geometry, form_geometry_showing, inset_by, row_access_nodes, row_description,
        view_config_form, view_config_form_showing,
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
        let hiding = ConfigForm::new(
            fields,
            shown.addable().into_iter().cloned().collect::<Vec<_>>(),
        );

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
                .find(|n| n.tag == super::address::control("insp", &row.key))
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
                tags.contains(&super::address::control("insp", &row.key)),
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
                ConfigField::new("ident", "id", Applies::Hot, "ab").with_shape(
                    FieldType::Formatted {
                        of: TextFormat::Chars {
                            allow: CharSet::of(&[CharClass::LowerHex]),
                            len: Span::between(1, 4),
                        },
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
        // Affordances published inside a row's control; three shapes answer 0.
        let bare = |key: &str| geometry.row(key).expect("shown").parts.len();

        assert!(has("f.control.free"), "text: the control IS the box");
        assert_eq!(bare("free"), 0, "and it has no parts inside it");
        assert!(
            has("f.step.count.up") && has("f.step.count.down"),
            "{tags:?}"
        );
        // ★★★★★ R1837 — a SWITCH; this read `f.toggle.on`, a pill this file rolled.
        assert!(has("f.switch.on"), "a boolean is a switch: {tags:?}");
        assert_eq!(bare("on"), 0, "the control IS the switch, so it holds none");
        // ★★★★★ R1732 — a choice is COLLAPSED: one chevron, and no option in
        // the row at all. The roster is a layer of its own and is drawn only
        // while it is open, which is what the two assertions below are about.
        assert!(has("f.pick.mode"), "{tags:?}");
        assert!(
            !has("f.option.mode.a") && !has("f.option.mode.b"),
            "★ the roster is not in the row while the control is shut: {tags:?}",
        );
        let open = Picker::over(["a", "b"], "b").expect("a roster");
        let shown = form_geometry_showing(
            &form,
            (0, 0),
            &FormStyle::default(),
            Some(OpenPicker {
                key: "mode",
                picker: &open,
                room: Rect::new(0, 0, 400, 900),
            }),
        );
        let mut open_tags = Vec::new();
        view_config_form_showing("f", &form, &shown, &Theme::dark(), Some(&open)).for_each_node(
            &mut |node| {
                if let Some(tag) = node.node.tag() {
                    open_tags.push(tag.to_string());
                }
            },
        );
        assert!(
            open_tags.iter().any(|t| t == "f.option.mode.a")
                && open_tags.iter().any(|t| t == "f.option.mode.b"),
            "★★ and it IS there once it is opened, under the names it always had: {open_tags:?}",
        );
        // A set stays a set: every option on screen, always.
        assert!(
            has("f.option.perm.read") && has("f.option.perm.write"),
            "{tags:?}"
        );
        assert!(
            has("f.item.hosts.0") && has("f.item.hosts.1") && has("f.item.hosts.add"),
            "a list is one row per element, then the row that adds one: {tags:?}"
        );

        // ★★★ R1690 — the seventh arm, and it IS drawn as a text box. That is
        // the outcome this assertion was written to make somebody argue for
        // rather than fall into, and the argument is: a shape says what a value
        // has to BE, a control says how it is entered, and for a string those
        // two coincide whether or not something downstream parses it. Giving a
        // formatted string a control of its own would teach a person a
        // distinction twice — once in the box and once in the refusal — for a
        // difference that only shows when the value is wrong.
        //
        // So what is asserted is the DECISION: no parts, like free text, and a
        // count that still says how many shapes carry affordances.
        assert!(
            has("f.control.ident"),
            "formatted: the control is the box too"
        );
        assert_eq!(bare("ident"), 0, "and it has no parts inside it either");
        assert_eq!(FieldType::ARMS, 7);
        let with_parts = geometry.rows.iter().filter(|r| !r.parts.is_empty()).count();
        // ★★★★★ R1837 — FOUR: the boolean left, third shape whose control is all of it.
        assert_eq!(with_parts, 4, "four of seven shapes carry affordances");
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
                    .any(|n| n.tag == super::address::control("insp", &row.key)
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
            ConfigField::new("a.formatted", "id", Applies::Hot, "ab").with_shape(
                FieldType::Formatted {
                    of: TextFormat::Chars {
                        allow: CharSet::of(&[CharClass::LowerHex]),
                        len: Span::between(1, 4),
                    },
                },
            ),
        ];
        // ★ R1690 — the array is sized by the type's own arm count rather than
        // a literal, so a new shape widens the census by construction instead
        // of by somebody remembering to widen it.
        let mut seen = [false; FieldType::ARMS];
        for field in &fields {
            seen[match field.shape() {
                FieldType::Text => 0,
                FieldType::Integer { .. } => 1,
                FieldType::Boolean => 2,
                FieldType::Choice { .. } => 3,
                FieldType::Flags { .. } => 4,
                FieldType::List { .. } => 5,
                FieldType::Formatted { .. } => 6,
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
            let tag = super::address::control("f", field.key());
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

    /// ★★ R1674 — the crate gate ([`crate::frame_gate`]), which this painter is
    /// one of the two founding cases for.
    ///
    /// R1672 gave this module a containment test with a stand-in metric of its
    /// own; the gate now asks the same question of every bordered painter here,
    /// with the metric the layout used, so the fifteen are judged by one rule.
    /// The long-key case above stays as it is — it asks a different question
    /// (does the KEY give way rather than the badges) that this cannot.
    #[test]
    fn r1674_a_config_form_keeps_its_rows_inside_its_frame() {
        let theme = pinion_core::theme::Theme::default();
        let style = FormStyle::default();
        let form = ConfigForm::new(
            vec![
                ConfigField::new("transport.link.tx", "int", Applies::Hot, "65535"),
                ConfigField::new("transport.link.mode", "enum", Applies::Restart, "unicast"),
            ],
            Vec::new(),
        );
        crate::frame_gate::assert_frame_contained("config form", &mut |_w, _h| {
            let geometry = form_geometry(&form, (0, 0), &style);
            view_config_form("f", &form, &geometry, &theme)
        });
    }

    /// ★★★★★ R1691 — **every shape's control announces the kind it is**, over
    /// the whole vocabulary rather than over the shapes one screen happens to
    /// hold.
    ///
    /// This test exists because a counterfactual passed. Flipping the boolean
    /// arm to `TextInput` — the exact defect the round set out to fix, where a
    /// reader is told to type into a toggle — was caught by nothing: the
    /// consumer whose gate checked the mapping has five fields and not one of
    /// them is a boolean, so the arm was never reached. The corpus, not the
    /// gate, was the hole ([[debt-a-gate-that-only-sees-correct-code-is-unproven]],
    /// fifth occurrence).
    ///
    /// So the population is the TYPE's, here, in the crate that owns the
    /// mapping — and a seventh shape fails to compile rather than going
    /// unchecked.
    /// One row of the shape census: what the field holds, what its control
    /// announces as, a value of that shape, and every affordance the shape
    /// paints inside the control with the kind each announces as.
    struct ShapeVoice {
        shape: FieldType,
        control: pinion_a11y::AriaRole,
        value: &'static str,
        parts: &'static [(&'static str, pinion_a11y::AriaRole)],
        /// ★★★★★ R1732 — painted parts that deliberately have **no** node,
        /// and are folded into the control's announcement instead.
        ///
        /// Declared rather than implied by absence. A part that quietly fell
        /// out of `parts` would look exactly like this, and the difference —
        /// a reader who receives it at the control, versus a reader who never
        /// receives it at all — is the whole of what the census is for. Each
        /// one is checked twice below: no node under its own tag, and a
        /// declared silence on the node the painter drew.
        silent: &'static [&'static str],
    }

    /// Every shape a configuration document can hold, and the voice its editor
    /// gives a reader.
    ///
    /// ★★ The PARTS are declared too, and by role. A counterfactual proved that
    /// half: announcing a single-choice set as checkboxes — telling a reader
    /// they may pick several when picking one un-picks another — was caught by
    /// nothing while the test checked only that each part had a name. A named
    /// node satisfies a census whatever it calls itself.
    fn shape_voices() -> [ShapeVoice; 7] {
        use pinion_a11y::AriaRole;
        [
            ShapeVoice {
                shape: FieldType::Text,
                control: AriaRole::TextInput,
                value: "free",
                parts: &[],
                silent: &[],
            },
            ShapeVoice {
                shape: FieldType::Formatted {
                    of: pinion_core::widgets::text_format::TextFormat::number(0, 9),
                },
                control: AriaRole::TextInput,
                value: "7",
                parts: &[],
                silent: &[],
            },
            ShapeVoice {
                shape: FieldType::Integer { min: 0, max: 9 },
                control: AriaRole::SpinButton,
                value: "3",
                parts: &[
                    ("step.k.down", AriaRole::Button),
                    ("step.k.up", AriaRole::Button),
                ],
                silent: &[],
            },
            ShapeVoice {
                shape: FieldType::Boolean,
                control: AriaRole::CheckBox,
                value: "true",
                // ★★★★★ R1837 — none. The control IS the switch, so there is
                // no affordance inside it to announce; the mark used to be
                // published here as a second checkbox carrying the control's
                // own name and bit.
                parts: &[],
                silent: &[],
            },
            ShapeVoice {
                shape: FieldType::Choice {
                    of: vec!["a".into(), "b".into()],
                },
                // ★★★★★ R1732 — a combo box, because the control is now
                // COLLAPSED. A radio group promises members a reader can move
                // between, and until the roster is opened there are none.
                control: AriaRole::ComboBox,
                value: "a",
                parts: &[],
                silent: &["pick.k"],
            },
            ShapeVoice {
                shape: FieldType::Flags {
                    of: vec!["r".into(), "w".into()],
                },
                control: AriaRole::Group,
                value: "r",
                parts: &[
                    ("option.k.r", AriaRole::CheckBox),
                    ("option.k.w", AriaRole::CheckBox),
                ],
                silent: &[],
            },
            ShapeVoice {
                shape: FieldType::List {
                    of: Box::new(FieldType::Text),
                },
                // R1693 — a `group`. What this shape paints is text boxes, and
                // the parts below are the evidence: a `list` would promise
                // `listitem`s that do not exist.
                control: AriaRole::Group,
                value: "x, y",
                parts: &[
                    ("item.k.0", AriaRole::TextInput),
                    ("item.k.1", AriaRole::TextInput),
                    ("item.k.add", AriaRole::Button),
                ],
                silent: &[],
            },
        ]
    }

    #[test]
    fn r1691_every_field_shape_announces_the_kind_its_control_is() {
        let style = FormStyle::default();
        for ShapeVoice {
            shape,
            control: want,
            value,
            parts,
            silent,
        } in shape_voices()
        {
            let word = shape.clone();
            let form = ConfigForm::new(
                vec![ConfigField::new("k", "t", Applies::Hot, value).with_shape(shape)],
                Vec::new(),
            );
            let geometry = form_geometry(&form, (0, 0), &style);
            let nodes = row_access_nodes("f", &form, &geometry);
            let control = nodes
                .iter()
                .find(|n| n.tag == "f.control.k")
                .expect("the row announces its control");
            assert_eq!(
                control.role, want,
                "a {word:?} row announces as {:?}, and a reader is told what \
                 they can do with it",
                control.role,
            );
            // A bijection with what the painter laid out: a part with no voice
            // and a voice for a part nobody paints are both failures, and the
            // first is the one a count alone would hide.
            let painted: Vec<&str> = geometry.rows[0]
                .parts
                .iter()
                .map(|(suffix, _)| suffix.as_str())
                .collect();
            let declared: Vec<&str> = parts
                .iter()
                .map(|(suffix, _)| *suffix)
                .chain(silent.iter().copied())
                .collect();
            assert_eq!(
                painted, declared,
                "a {word:?} row paints affordances this test does not name",
            );
            // ★★★★★ R1732 — the folded ones, checked in BOTH directions: no
            // node of their own, and a declared silence on the node the painter
            // drew. Either check alone passes for a part that was simply
            // forgotten, which is the case this pair exists to tell apart.
            let scene = view_config_form("f", &form, &geometry, &Theme::dark());
            for suffix in silent {
                let tag = format!("f.{suffix}");
                assert!(
                    !nodes.iter().any(|n| n.tag == tag),
                    "{tag} is folded into its control and must not be announced twice",
                );
                let mut declared_silence = None;
                scene.for_each_node(&mut |node| {
                    if node.node.tag() == Some(tag.as_str()) {
                        declared_silence = node
                            .node
                            .layout_style()
                            .and_then(|layout| layout.silence.clone());
                    }
                });
                let silence = declared_silence
                    .unwrap_or_else(|| panic!("{tag} is painted and declares no silence"));
                assert_eq!(
                    silence.relay_target(),
                    Some("f.control.k"),
                    "{tag} says a reader receives it at the control, and names which",
                );
            }
            for (suffix, part_role) in parts {
                let tag = format!("f.{suffix}");
                let part = nodes
                    .iter()
                    .find(|n| n.tag == tag)
                    .unwrap_or_else(|| panic!("{tag} is painted and has no voice"));
                assert_eq!(
                    part.role, *part_role,
                    "{tag} announces as {:?} — the kind is what tells a reader \
                     whether picking one un-picks another",
                    part.role,
                );
                let name = part.name.as_deref().unwrap_or_default();
                assert!(
                    name.contains('k'),
                    "{tag} announces as {name:?}, which names no subject",
                );
            }
        }
    }

    /// ★★★★★ R1837 — **a boolean row wears a switch whose knob moves.**
    ///
    /// The defect this ends: the control drew a bordered pill carrying
    /// `U+2713` when on and a SPACE when off, so the two states differed by a
    /// glyph and a colour and by nothing a person catches at a glance. Reported
    /// from a running window as "is that a text edit or a button".
    ///
    /// [`crate::switch`] was lifted at R1574 out of twelve bindings that had
    /// each hand-rolled a track and a knob, and this file hand-rolled a
    /// thirteenth in the same crate.
    ///
    /// Asserted as a **relation**, never as a coordinate: the knob has to sit
    /// at a DIFFERENT x in the two states. A pinned pair of numbers would pass
    /// for a track whose knob never moved if the metrics were ever restyled.
    #[test]
    fn r1837_a_boolean_rows_knob_moves_with_its_bit() {
        let style = FormStyle::default();
        let track_of = |value: &str| {
            let form = ConfigForm::new(
                vec![
                    ConfigField::new("on", "bool", Applies::Hot, value)
                        .with_shape(FieldType::Boolean),
                ],
                Vec::new(),
            );
            let geometry = form_geometry(&form, (0, 0), &style);
            let scene = view_config_form("f", &form, &geometry, &Theme::dark());
            let mut found = None;
            scene.for_each_node(&mut |node| {
                if node.node.tag() == Some("f.switch.on") {
                    found = node.node.layout_style().cloned();
                }
            });
            found.expect("the switch is painted and addressable")
        };

        let off = track_of("false");
        let on = track_of("true");
        let want = super::boolean_switch_style();
        assert_eq!(
            off.size,
            pinion_core::style::Size::px(want.track_w, want.track_h),
            "the track is not the size this form asked the painter for",
        );
        assert_eq!(off.size, on.size, "the TRACK must not change size");
        assert_ne!(
            off.justify_content, on.justify_content,
            "the knob sits at the same end in both states — a mark that only \
             changes colour is what a person could not read at a glance",
        );
        assert!(
            !off.focusable,
            "the control owns the Tab stop; a second stop inside it makes a \
             reader press Tab twice to leave one control",
        );
    }

    /// ★★★★★ R1837 — and the row wears the **same box** its neighbours do.
    ///
    /// Measured off the behaviour canon: its boolean control carries the filled,
    /// bordered, rounded box a text row carries, and what separates them is the
    /// switch inside. Ours painted into an unstyled container, so the boolean
    /// row was the one row on the panel with no box at all — which is what put
    /// the question "text edit or button" in a person's mouth in the first
    /// place, from the CONTRAST with the rows around it.
    ///
    /// The declared frame and the painted one are held together by
    /// `r1672_every_shapes_control_frame_is_the_one_it_paints`; this asserts the
    /// half that test cannot see, which is that the skin is there at all.
    #[test]
    fn r1837_a_boolean_control_wears_the_box_its_neighbours_wear() {
        let style = FormStyle::default();
        let form = ConfigForm::new(
            vec![
                ConfigField::new("on", "bool", Applies::Hot, "false")
                    .with_shape(FieldType::Boolean),
                ConfigField::new("name", "text", Applies::Hot, "x"),
            ],
            Vec::new(),
        );
        let geometry = form_geometry(&form, (0, 0), &style);
        let scene = view_config_form("f", &form, &geometry, &Theme::dark());
        let skin_of = |tag: &str| {
            let mut found = None;
            scene.for_each_node(&mut |node| {
                if node.node.tag() == Some(tag) {
                    found = Some(node.node.box_style().cloned());
                }
            });
            found.expect("the control is painted")
        };
        let boolean = skin_of("f.control.on").expect("a boolean control wears a skin");
        let text = skin_of("f.control.name").expect("a text control wears a skin");
        assert_eq!(
            (boolean.corner_radius, boolean.border.map(|b| b.width)),
            (text.corner_radius, text.border.map(|b| b.width)),
            "the two rows wear different boxes — the canon distinguishes them by \
             the control inside, not by whether there is a box",
        );
    }

    /// The boolean row's state is the BIT, so a reader's toggle command reads
    /// the same fact the ink does — announced **once**.
    ///
    /// ★★★★★ R1837 — this test used to be named `…_in_both_places` and required
    /// the second announcement. It is rebuilt rather than deleted, because the
    /// claim it made was a design decision and the way to retire one of those
    /// here is to state the replacement: R1691 gave the mark its own checkbox
    /// node to end a silence, and that was right at the time; what nobody asked
    /// afterwards is whether the control above already said it. It did — same
    /// role, same name, same bit — so a reader met one checkbox twice, at two
    /// rectangles, and the second was a square inside the first.
    #[test]
    fn r1837_a_boolean_row_announces_its_bit_exactly_once() {
        use pinion_a11y::{AccessValue, AriaRole};

        let style = FormStyle::default();
        for (value, want) in [("true", true), ("false", false)] {
            let form = ConfigForm::new(
                vec![
                    ConfigField::new("on", "bool", Applies::Hot, value)
                        .with_shape(FieldType::Boolean),
                ],
                Vec::new(),
            );
            let geometry = form_geometry(&form, (0, 0), &style);
            let nodes = row_access_nodes("f", &form, &geometry);
            let control = nodes
                .iter()
                .find(|n| n.tag == "f.control.on")
                .expect("announced");
            assert_eq!(control.state.checked, Some(want));
            assert_eq!(control.value, Some(AccessValue::Bool(want)));
            let checkboxes: Vec<&str> = nodes
                .iter()
                .filter(|n| n.role == AriaRole::CheckBox)
                .map(|n| n.tag.as_str())
                .collect();
            assert_eq!(
                checkboxes,
                ["f.control.on"],
                "one control, one checkbox — a reader who hears it twice cannot \
                 tell two controls from one said twice",
            );
            // ★★★★★ And the rectangle it announces is the one a press lands
            // in — which is the whole control, because the canon's boolean box
            // takes the pointer across all of it and this form's consumer
            // presses it that way. The square that used to be announced was a
            // fraction of it.
            let row = geometry.row("on").expect("shown");
            assert_eq!(
                control.bounds,
                Some(row.control),
                "the checkbox announces the box it presses",
            );
            assert!(
                row.parts.is_empty(),
                "a boolean publishes no part — the control IS the switch: {:?}",
                row.parts,
            );
        }
    }
}
