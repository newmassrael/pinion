//! R1722 §5.38 §5.39 §5.40 §2 #2 §2 #7 — **a chart's legend is a declared chip
//! row over the chart's own named parts, and what may be done to it is one word
//! every other property of the row is derived from.**
//!
//! ## The defect this exists for, measured before it was built
//!
//! Seven chart kinds in this crate name their parts. Measured on 2026-08-19
//! against the public surface rather than by reading the paint:
//!
//! | chart | named parts | legend before this round | could a caller toggle a part from it |
//! |---|---|---|---|
//! | line | series | static row, or an interactive one on opt-in | yes |
//! | scatter | series | static row, or an interactive one on opt-in | yes |
//! | polar | series | static row only | **no** |
//! | donut | slices | static row only | **no** |
//! | bar | one set over categories | none | **no** |
//! | box plot | one set over categories | none | **no** |
//! | candlestick | one session series | none | **no** |
//!
//! Two of the seven offered the gesture, five did not, and **nothing anywhere
//! said which was which** — so a board assembling several charts could not ask,
//! and had to know by having read this crate. The two that did offer it each
//! *chose*, per paint: a chart held an `Option<Vec<String>>` of caller-supplied
//! tags and branched on it, so the interactive row and the static row were two
//! painters a chart picked between, with nothing relating the pick to anything a
//! caller could read. The entries themselves were derived **six** times over
//! four files — line twice, scatter twice, polar once, donut once — each copy
//! spelling `color.unwrap_or_else(|| palette.color(i))` again.
//!
//! ## The floor this is built to beat, measured rather than read
//!
//! The mature toolkit at 6.11.1 carries two chart modules and its legend was
//! measured in both. Its charting module is not among the ones built on this
//! machine, so that measurement is of its source; this crate's own claims below
//! are asserted by tests instead.
//!
//! Its older module is **uniform where this crate was not**: six legend-marker
//! kinds, one per series family, so every chart type there has legend entries.
//! That is the one axis on which it was ahead, and this module's breadth exists
//! to answer it. On every other axis it is a floor:
//!
//! * a marker emits a *clicked* notification and **nothing more** — hiding the
//!   series is left to whoever wired the signal, so "toggling a series from the
//!   legend" is application work there and no two applications need agree;
//! * a marker is **not focusable**: the item drawing it handles a hover event
//!   and no key event at all, so the row is reachable only by pointer;
//! * **neither chart module contains a single accessibility call** — measured
//!   across both trees, the count is zero. A legend there is invisible to a
//!   screen reader except as loose text;
//! * nothing declares whether a part *may* be hidden, so a caller cannot ask
//!   before offering the gesture;
//! * and its newer module went further back: a series there publishes a list of
//!   `{colour, border colour, label}` carrying no identity, no state and no
//!   signal, so its legend is paint an application redraws.
//!
//! ## What is derived, and from what
//!
//! [`LegendInteraction`] is the whole declaration. A legend has nowhere to state
//! any of the following separately:
//!
//! | | [`Paint`](LegendInteraction::Paint) | [`Toggle`](LegendInteraction::Toggle) |
//! |---|---|---|
//! | what the row is | swatches and labels | a chip row where any subset may be on |
//! | the row's accessibility node | none | a `group` |
//! | an entry's accessibility node | none | `button` carrying its on-state |
//! | Tab stops | **none** | **one per drawn entry** |
//! | what a press does | [refused, with the reason](StaticLegend) | turns that part off or on |
//! | an off entry | (nothing is off) | grey swatch, dimmed label, slot kept |
//!
//! The row and the paint come from the same value, so a chart **cannot** paint
//! an interaction it did not declare: [`Legend::scene`] is the only painter, the
//! two row painters beneath it are private to this module, and the chip row
//! [`Legend::group`] hands the accessibility layer is derived from the same word
//! that chose the painter. This is R1721's shape one layer down — there a screen
//! announced a rule it did not obey, and the repair was to derive the
//! announcement and the behaviour from one declaration.
//!
//! ## What this module is not
//!
//! It does not mutate a chart. A press answers with the on-set that *would*
//! follow and the caller rebuilds the chart with it — the posture the rest of
//! this crate has, where a chart is a value projected from application state
//! rather than a thing holding its own.

use pinion_a11y::AccessNode;
use pinion_a11y::chip_group::chip_group_nodes;
use pinion_core::Scene;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextAlign, TextOverflow,
    TextStyle,
};
use pinion_core::widgets::chip_group::{Chip, ChipGroup, ChipPosture, Choice, Outcome};
use pinion_core::widgets::interaction::InteractionState;

use crate::draw::{LEGEND_OVERFLOW_SLOT, label_node, legend_fit, legend_row_width};
use crate::style::ChartStyle;

/// One legend entry: the colour its part is drawn in, what that part is called,
/// and whether the part is currently shown.
///
/// Derived from the chart's own data — a series, a slice — rather than supplied,
/// so an entry cannot name a part the chart does not have or carry a colour the
/// chart does not paint that part in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegendEntry {
    /// The colour the part is drawn in: its own override, else its palette slot.
    pub swatch: Color,
    /// The part's name, which is also the entry's accessible name.
    pub label: String,
    /// Whether the part is drawn. An off part keeps its entry, its slot and its
    /// palette index, so turning it back on lands it where it was (R1379).
    pub on: bool,
}

impl LegendEntry {
    /// An entry for a part that is currently shown.
    #[must_use]
    pub fn new(swatch: Color, label: impl Into<String>) -> Self {
        Self {
            swatch,
            label: label.into(),
            on: true,
        }
    }

    /// Whether the part this entry stands for is drawn.
    #[must_use]
    pub fn shown(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
}

/// What may be done to a legend's entries — the one word every other property of
/// the row derives from. See the module doc's table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LegendInteraction {
    /// The row is paint: swatches and labels, no focus stop, no announced
    /// control, and a press [refused with its reason](StaticLegend).
    ///
    /// The default, because a chart whose parts nobody has said are hideable is
    /// a chart whose legend must not claim that they are.
    #[default]
    Paint,
    /// Each entry is a chip in a row where any subset may be on: its own Tab
    /// stop, an announced toggle button carrying its on-state, and a press that
    /// turns its part off or on.
    ///
    /// The rule is [`Choice::Any`] and is not a policy chosen here — a legend
    /// over which no part, some parts or every part may be hidden *is* the
    /// any-subset rule, and the two composite rules would each be a claim this
    /// crate has no grounds to make about a caller's data.
    Toggle,
}

/// Why a press on a [`LegendInteraction::Paint`] legend changed nothing.
///
/// A distinct type rather than a fourth reason inside the chip row's own
/// refusals: that enum answers *for a chip row*, and a paint legend has no chip
/// row to answer with. This is the prior question — the declaration is a
/// precondition of dispatch (R1637) — so this is what "the precondition did not
/// hold" reads as, and it carries the sentence a person is shown (R1720) rather
/// than leaving each caller to invent one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticLegend;

impl core::fmt::Display for StaticLegend {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(
            "this legend is paint: its entries are not focus stops, are not announced as controls, and cannot be pressed",
        )
    }
}

impl core::error::Error for StaticLegend {}

/// The question **every** chart kind in this crate answers: what is your
/// legend, and what may be done to it?
///
/// A trait rather than seven inherent methods, because the question is asked by
/// a board holding charts of several kinds. Seven inherent methods can only be
/// called by code that already knows which chart it is holding — which is the
/// position a dashboard is *not* in, and is why "which chart kinds support
/// toggling a series from the legend" was folklore before this round.
///
/// A chart holding one unnamed set answers with an empty roster. That is a
/// truthful answer rather than a missing one, and it is the answer that was
/// unavailable: a board could previously only discover that a bar chart has no
/// legend by finding that nothing appeared.
///
/// `every_chart_kind_in_this_crate_declares_its_legend` reads this crate's own
/// source and fails if a chart kind is added without an implementation, so the
/// roster is derived from the code rather than remembered here.
pub trait ChartLegend {
    /// This chart's legend. The same value its paint is derived from, so a
    /// caller reading it cannot be told something the row does not do.
    fn legend(&self) -> Legend;

    /// Where this chart seats its legend row inside `rect` — the chart's own
    /// decision, and the only part of a legend that is per-chart.
    ///
    /// Published rather than kept private because a consumer building the
    /// accessibility tree needs the same width the paint used: the roster a
    /// keyboard walks is the roster the row drew ([`Legend::group`]), so a
    /// consumer guessing the width could announce a different set of entries
    /// from the one on screen.
    ///
    /// **The default is the top margin band**, from the plot's left edge to the
    /// chart's right one — which is where six of this crate's seven chart kinds
    /// put it. Written once here rather than six times: R1722's own obligation-3b
    /// self-grep found six byte-identical bodies it had just created, and the
    /// only chart with an opinion is the donut, which centres its row in a
    /// reserved bottom band and says why in its override.
    fn legend_seat(&self, rect: Rect, style: &ChartStyle) -> LegendSeat {
        LegendSeat {
            x: rect.x + style.margin.left,
            y: rect.y + 6,
            avail: rect.w.saturating_sub(style.margin.left),
        }
    }

    /// The legend row, painted where this chart seats it.
    ///
    /// Provided, so the four charts that seat a row in the top band do not each
    /// re-derive `avail = rect.w - margin.left` — which they did, in four
    /// copies, before this round.
    fn legend_scene(&self, rect: Rect, style: &ChartStyle) -> Vec<Scene> {
        let seat = self.legend_seat(rect, style);
        self.legend().scene(seat.x, seat.y, seat.avail, style)
    }

    /// This chart's legend as accessibility nodes, seated exactly as its paint
    /// is — empty unless the legend declared [`LegendInteraction::Toggle`].
    ///
    /// One call, because the width is the thing a consumer would otherwise have
    /// to reconstruct. The reference toolkit has no counterpart: measured across
    /// both of its chart modules, neither makes a single accessibility call.
    fn legend_access_nodes(
        &self,
        rect: Rect,
        style: &ChartStyle,
        postures: &LegendPostures,
        focused: Option<&str>,
    ) -> Vec<AccessNode> {
        self.legend()
            .access_nodes(self.legend_seat(rect, style).avail, postures, focused)
    }
}

/// Where a chart seats its legend row: the row's left edge, its top, and the
/// width it has from there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegendSeat {
    /// The row's left edge.
    pub x: u32,
    /// The row's top.
    pub y: u32,
    /// The width the row has from `x`. The row shrinks its slots and then drops
    /// entries to fit this rather than running past the chart into whatever is
    /// beside it (R1396).
    pub avail: u32,
}

/// How many of a legend's entries a row `avail` px wide actually seats, and how
/// many the `+N` marker stands in for (R1396).
///
/// Published rather than left inside the painter because it is the one fact the
/// paint and the accessibility tree must agree on: a keyboard can reach exactly
/// the entries that were drawn, so [`Legend::group`] is built from `shown` and
/// [`Legend::scene`] draws `shown`, both from this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegendSeating {
    /// How many entries are drawn, in roster order.
    pub shown: usize,
    /// How many entries the `+N` marker stands in for. `0` = the row is whole.
    pub dropped: usize,
}

/// Where the pointer is on each legend entry, in roster order.
///
/// A chart cannot know this — it is a pure scene producer over application
/// state, and hover and press live with whoever is running the pointer. So a
/// legend derives everything *about the parts* and takes this back from the
/// consumer, rather than pretending every entry is at rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LegendPostures(Vec<ChipPosture>);

impl LegendPostures {
    /// Every entry at rest.
    #[must_use]
    pub fn at_rest() -> Self {
        Self(Vec::new())
    }

    /// Report where the pointer is on entry `index`, from any of the framework's
    /// interaction states — a toggle's, a button's, whatever the consumer is
    /// already tracking.
    ///
    /// Indexed rather than taking a parallel list, because a list of the wrong
    /// length is the hazard R1722 removed from the tags: the previous surface
    /// zipped a caller's `Vec<String>` against the series and silently dropped
    /// the tail. An index past the roster is stored and simply never matches an
    /// entry, so it cannot displace another entry's posture.
    #[must_use]
    pub fn under(mut self, index: usize, state: &impl InteractionState) -> Self {
        let posture = if state.is_disabled() {
            ChipPosture::Locked
        } else if state.is_pressed() {
            ChipPosture::Pressed
        } else if state.is_hovered() {
            ChipPosture::Hover
        } else {
            ChipPosture::Idle
        };
        if self.0.len() <= index {
            self.0.resize(index + 1, ChipPosture::Idle);
        }
        self.0[index] = posture;
        self
    }

    /// Where the pointer is on entry `index` — at rest unless reported.
    #[must_use]
    pub fn at(&self, index: usize) -> ChipPosture {
        self.0.get(index).copied().unwrap_or_default()
    }
}

/// A chart's legend: the chart's named parts, and one word for what may be done
/// to them.
///
/// Every chart kind in this crate derives one, including the kinds whose parts
/// are a single unnamed set — those answer with an empty roster, which is the
/// answer a board could not get before this round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Legend {
    tag_prefix: String,
    name: String,
    entries: Vec<LegendEntry>,
    interaction: LegendInteraction,
}

impl Legend {
    /// A legend over `entries`, painted under `tag_prefix`, named `name` for a
    /// screen reader.
    ///
    /// [`Paint`](LegendInteraction::Paint) until a caller says otherwise —
    /// see that variant's doc for why the default is the inert one.
    #[must_use]
    pub fn new(
        tag_prefix: impl Into<String>,
        name: impl Into<String>,
        entries: Vec<LegendEntry>,
    ) -> Self {
        Self {
            tag_prefix: tag_prefix.into(),
            name: name.into(),
            entries,
            interaction: LegendInteraction::Paint,
        }
    }

    /// Declare what may be done to this legend's entries.
    #[must_use]
    pub fn with_interaction(mut self, interaction: LegendInteraction) -> Self {
        self.interaction = interaction;
        self
    }

    /// What may be done to this legend's entries — the word everything else is
    /// derived from, and the question a board assembling several charts asks
    /// before it offers the gesture.
    #[must_use]
    pub const fn interaction(&self) -> LegendInteraction {
        self.interaction
    }

    /// The chart's named parts, in the order the chart draws them, whether or
    /// not a row of any particular width can seat them all.
    #[must_use]
    pub fn entries(&self) -> &[LegendEntry] {
        &self.entries
    }

    /// How many parts this legend names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this legend names no parts at all — a chart holding one unnamed
    /// set, which is a truthful answer rather than a missing one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// This legend's accessible name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The tag the row itself carries — the group's node when this legend
    /// toggles, and the prefix every entry tag extends.
    #[must_use]
    pub fn tag(&self) -> String {
        format!("{}.legend", self.tag_prefix)
    }

    /// The tag entry `index` carries.
    ///
    /// Derived from the chart's tag prefix rather than supplied by the caller,
    /// which is what makes a mismatch unrepresentable: the previous surface took
    /// a `Vec<String>` of tags and zipped it against the entries, so a caller
    /// passing too few tags silently lost the tail of its own legend.
    #[must_use]
    pub fn entry_tag(&self, index: usize) -> String {
        format!("{}.legend.{index}", self.tag_prefix)
    }

    /// How many entries a row `avail` px wide seats, and how many it drops.
    #[must_use]
    pub fn seating(&self, avail: u32) -> LegendSeating {
        let fit = legend_fit(avail, self.entries.len());
        LegendSeating {
            shown: fit.shown,
            dropped: fit.hidden,
        }
    }

    /// The width (px) this legend's row occupies inside `avail` px — always
    /// `<= avail`. A chart that centres its row needs this to place its left
    /// edge; a chart that starts at a fixed margin does not.
    #[must_use]
    pub fn width(&self, avail: u32) -> u32 {
        legend_row_width(avail, self.entries.len())
    }

    /// The chip row this legend *is*, for a row `avail` px wide — `None` when it
    /// declared itself [paint](LegendInteraction::Paint).
    ///
    /// One chip per **seated** entry, so the roster a keyboard walks is the
    /// roster the paint drew: an entry the `+N` marker stands in for is not a
    /// chip, because announcing a focusable control that was not painted is the
    /// defect this whole module exists to make unrepresentable.
    #[must_use]
    pub fn group(&self, avail: u32, postures: &LegendPostures) -> Option<ChipGroup> {
        match self.interaction {
            LegendInteraction::Paint => None,
            LegendInteraction::Toggle => {
                let shown = self.seating(avail).shown;
                let chips: Vec<Chip> = self
                    .entries
                    .iter()
                    .take(shown)
                    .enumerate()
                    .map(|(index, entry)| {
                        Chip::new(self.entry_tag(index), entry.label.clone(), entry.on)
                            .with_posture(postures.at(index))
                    })
                    .collect();
                Some(ChipGroup::new(
                    self.tag(),
                    self.name.clone(),
                    chips,
                    Choice::Any,
                ))
            }
        }
    }

    /// This legend's accessibility subtree for a row `avail` px wide, given the
    /// tag the shell reports as focused.
    ///
    /// Empty for a [paint](LegendInteraction::Paint) legend: its swatches and
    /// labels are already text in the tree, and adding control nodes for things
    /// no keyboard can reach would be the same lie the roster rule above avoids.
    /// The reference toolkit's two chart modules emit nothing here at all.
    #[must_use]
    pub fn access_nodes(
        &self,
        avail: u32,
        postures: &LegendPostures,
        focused: Option<&str>,
    ) -> Vec<AccessNode> {
        self.group(avail, postures)
            .map(|row| chip_group_nodes(&row, focused))
            .unwrap_or_default()
    }

    /// Press entry `index`: the on-set that would follow, or the reason nothing
    /// would.
    ///
    /// Otherwise the chip row's own rule answers, including for an index the
    /// roster does not have — which comes back as an `Ok` carrying that rule's
    /// own refusal, because "this row has no such member" is the row speaking
    /// and not a failure of the precondition below.
    ///
    /// # Errors
    ///
    /// [`StaticLegend`] when this legend declared itself
    /// [paint](LegendInteraction::Paint). The declaration is a precondition of
    /// dispatch, so the refusal names it rather than the press quietly doing
    /// nothing, and it carries the sentence a person is shown.
    ///
    /// The roster here is **every** part, not only the parts a particular width
    /// seated: an agent driving the chart over the wire can turn off a part the
    /// row was too narrow to draw, while a pointer and a keyboard cannot, and
    /// the `+N` marker is what says the row is short (§2 #7).
    ///
    /// It mutates nothing. The caller rebuilds the chart with the on-set it gets
    /// back, which is how the rest of this crate works.
    pub fn press(&self, index: usize) -> Result<Outcome, StaticLegend> {
        match self.interaction {
            LegendInteraction::Paint => Err(StaticLegend),
            LegendInteraction::Toggle => {
                let on: Vec<bool> = self.entries.iter().map(|entry| entry.on).collect();
                Ok(Choice::Any.apply(&on, index))
            }
        }
    }

    /// The row, painted: swatch + label per seated entry from `start_x` at
    /// `row_y` within `avail` px, plus a `+N` marker for any the width dropped.
    ///
    /// The **only** painter. Which of the two rows below it runs is derived from
    /// [`interaction`](Self::interaction), so a chart cannot paint focusable,
    /// hit-tested entries while declaring itself paint, nor declare the gesture
    /// and paint a row nothing can reach.
    #[must_use]
    pub fn scene(&self, start_x: u32, row_y: u32, avail: u32, style: &ChartStyle) -> Vec<Scene> {
        match self.interaction {
            LegendInteraction::Paint => paint_row(
                &self.entries,
                start_x,
                row_y,
                avail,
                style,
                &self.tag_prefix,
            ),
            LegendInteraction::Toggle => {
                let tags: Vec<String> = (0..self.entries.len())
                    .map(|index| self.entry_tag(index))
                    .collect();
                toggle_row(
                    &self.entries,
                    &tags,
                    start_x,
                    row_y,
                    avail,
                    style,
                    &self.tag_prefix,
                )
            }
        }
    }
}

/// The `+N` marker for a row that dropped `dropped` entries, seated at `x`
/// (R1396). Tagged `.legend.overflow` so an introspecting client can read "this
/// legend is incomplete, by this much" as scene data (§2 #7) rather than
/// inferring it from a missing index.
fn overflow_marker(dropped: usize, x: u32, row_y: u32, style: &ChartStyle, prefix: &str) -> Scene {
    let size = style.label_size_px.max(1);
    label_node(
        format!("+{dropped}"),
        x,
        row_y.saturating_sub(1),
        LEGEND_OVERFLOW_SLOT,
        TextAlign::Start,
        style.label,
        size,
        format!("{prefix}.legend.overflow"),
    )
}

/// The paint row (R1377): a colour swatch + a text label per entry, laid out
/// left-to-right on the shared slot grid from `start_x` at `row_y`, tagged
/// `.legend.{i}.swatch` / `.legend.{i}.label`.
///
/// Nothing here is focusable or hit-tested, which is what
/// [`LegendInteraction::Paint`] means. Private to this module since R1722: a
/// chart reaching this directly is how a legend used to paint one thing and
/// declare another.
fn paint_row(
    entries: &[LegendEntry],
    start_x: u32,
    row_y: u32,
    avail: u32,
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let size = style.label_size_px.max(1);
    let swatch = size;
    let fit = legend_fit(avail, entries.len());
    let mut out = Vec::new();
    for (i, entry) in entries.iter().take(fit.shown).enumerate() {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the legend index is small; the slot offset stays within u32"
        )]
        let entry_x = start_x + (i as u32) * fit.slot;
        out.push(crate::draw::box_node(
            Rect::new(entry_x, row_y, swatch, swatch),
            entry.swatch,
            format!("{prefix}.legend.{i}.swatch"),
        ));
        out.push(label_node(
            entry.label.clone(),
            entry_x + swatch + 4,
            row_y.saturating_sub(1),
            fit.slot.saturating_sub(swatch + 4),
            TextAlign::Start,
            style.label,
            size,
            format!("{prefix}.legend.{i}.label"),
        ));
    }
    if fit.hidden > 0 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the legend index is small; the slot offset stays within u32"
        )]
        let marker_x = start_x + (fit.shown as u32) * fit.slot;
        out.push(overflow_marker(fit.hidden, marker_x, row_y, style, prefix));
    }
    out
}

/// The toggle row (R1380/R1392): one focusable, hit-testable entry per entry, on
/// the same slot grid as [`paint_row`], each a `Container([swatch, label])`
/// carrying its chip's tag — so the router's deepest-tagged-ancestor hit test
/// resolves a click anywhere on the entry to that chip.
///
/// An off entry greys its swatch and dims its label — "this part is not drawn" —
/// while keeping its slot, so the toggle back on stays where it was. Entries a
/// too-narrow row cannot seat collapse into a non-interactive `+N` marker, and
/// [`Legend::group`] is built from the same seating so the keyboard's roster and
/// the paint agree.
fn toggle_row(
    entries: &[LegendEntry],
    tags: &[String],
    start_x: u32,
    row_y: u32,
    avail: u32,
    style: &ChartStyle,
    prefix: &str,
) -> Vec<Scene> {
    let size = style.label_size_px.max(1);
    // A little taller than the swatch so the whole slot is a comfortable
    // click / Tab target; the swatch + label centre inside it.
    let entry_h = size + 6;
    let zipped = entries.len().min(tags.len());
    let fit = legend_fit(avail, zipped);
    let mut out: Vec<Scene> = entries
        .iter()
        .zip(tags)
        .take(fit.shown)
        .enumerate()
        .map(|(i, (entry, tag))| {
            let swatch_color = if entry.on { entry.swatch } else { style.label };
            let ink = if entry.on {
                style.label
            } else {
                style.label.with_alpha(0x80)
            };
            #[allow(
                clippy::cast_possible_truncation,
                reason = "the legend index is small; the slot offset stays within u32"
            )]
            let entry_x = start_x + (i as u32) * fit.slot;
            let swatch = Scene::Box(
                BoxNode::new(
                    Rect::default(),
                    BoxStyle::filled(swatch_color).with_corner_radius(3),
                )
                .with_layout(LayoutStyle::new().with_size(Size::px(size, size))),
            );
            let label = Scene::Text(TextNode::styled(
                entry.label.clone(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(size)
                    .with_fg(ink)
                    .with_overflow(TextOverflow::Clip),
            ));
            Scene::Container(
                ContainerNode::new(vec![swatch, label])
                    .with_tag(tag.clone())
                    .with_layout(
                        LayoutStyle::new()
                            .flex(FlexDirection::Row)
                            .with_align_items(AlignItems::Center)
                            .with_gap(4)
                            .with_absolute_position(entry_x, row_y)
                            .with_size(Size::px(fit.slot.saturating_sub(8), entry_h))
                            .with_focusable(true),
                    ),
            )
        })
        .collect();
    if fit.hidden > 0 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the legend index is small; the slot offset stays within u32"
        )]
        let marker_x = start_x + (fit.shown as u32) * fit.slot;
        out.push(overflow_marker(fit.hidden, marker_x, row_y, style, prefix));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_probe::find;
    use pinion_a11y::AriaRole;
    use pinion_core::widgets::chip_group::Refusal;

    fn three(on: [bool; 3]) -> Legend {
        Legend::new(
            "chart",
            "Series",
            (0..3)
                .map(|i| {
                    LegendEntry::new(Color::rgb(0x10 * i, 0x20, 0x30), format!("s{i}"))
                        .shown(on[usize::from(i)])
                })
                .collect(),
        )
    }

    fn rooted(nodes: Vec<Scene>) -> Scene {
        Scene::Container(ContainerNode::new(nodes))
    }

    // ─── the declaration is what paints ──────────────────────────────────

    #[test]
    fn a_paint_legend_emits_no_focus_stop_and_a_toggle_legend_emits_one_per_entry() {
        // The headline. Only the WORD changes between these two builds; the
        // focusability of the row moves with it, and a chart has nowhere to
        // state the two separately.
        let style = ChartStyle::default();
        let painted = three([true; 3]).scene(0, 6, 400, &style);
        let toggling = three([true; 3])
            .with_interaction(LegendInteraction::Toggle)
            .scene(0, 6, 400, &style);

        let stops = |nodes: &[Scene]| {
            nodes
                .iter()
                .filter(|n| match n {
                    Scene::Container(c) => c.layout.focusable,
                    _ => false,
                })
                .count()
        };
        assert_eq!(stops(&painted), 0, "a paint legend is not reachable");
        assert_eq!(stops(&toggling), 3, "a toggle legend is one stop per entry");
    }

    #[test]
    fn a_paint_legend_emits_leaves_and_a_toggle_legend_emits_entry_containers() {
        let style = ChartStyle::default();
        let painted = rooted(three([true; 3]).scene(0, 6, 400, &style));
        let toggling = rooted(
            three([true; 3])
                .with_interaction(LegendInteraction::Toggle)
                .scene(0, 6, 400, &style),
        );
        // The paint row names each PART of an entry; the toggle row names the
        // entry itself, because the entry is the hit target.
        assert!(find(&painted, "chart.legend.0.swatch").is_some());
        assert!(find(&painted, "chart.legend.0").is_none());
        assert!(find(&toggling, "chart.legend.0").is_some());
        assert!(find(&toggling, "chart.legend.0.swatch").is_none());
    }

    // ─── the declaration is a precondition of dispatch ───────────────────

    #[test]
    fn pressing_a_paint_legend_is_refused_with_its_reason() {
        let refused = three([true; 3]).press(0).unwrap_err();
        // Not a silent no-op: the refusal carries the sentence a person is
        // shown, which is the difference from the floor's click notification
        // that means nothing until an application decides what it means.
        assert_eq!(refused, StaticLegend);
        assert!(
            refused.to_string().contains("cannot be pressed"),
            "the refusal says what did not happen: {refused}"
        );
    }

    #[test]
    fn pressing_a_toggle_legend_turns_exactly_that_entry_over() {
        let legend = three([true, true, true]).with_interaction(LegendInteraction::Toggle);
        let Ok(Outcome::Set { on, now }) = legend.press(1) else {
            panic!("a toggle legend answers with an on-set")
        };
        assert_eq!(on, vec![true, false, true], "only entry 1 moved");
        assert!(!now, "and it went off");
    }

    #[test]
    fn any_subset_of_a_toggle_legend_may_be_off_at_once() {
        // The rule is any-subset, so a legend never refuses to hide the last
        // part — an empty chart is a legitimate thing to ask a legend for,
        // unlike a radio group's last selection.
        let legend = three([false, false, true]).with_interaction(LegendInteraction::Toggle);
        let Ok(Outcome::Set { on, .. }) = legend.press(2) else {
            panic!("the row answers")
        };
        assert_eq!(on, vec![false, false, false], "every part may be off");
    }

    #[test]
    fn pressing_an_entry_the_roster_does_not_have_is_refused_by_the_rule() {
        let legend = three([true; 3]).with_interaction(LegendInteraction::Toggle);
        assert_eq!(
            legend.press(3),
            Ok(Outcome::Refused(Refusal::NoSuchMember)),
            "past the end is the row's own refusal, not the declaration's"
        );
    }

    // ─── the accessibility tree says what the declaration says ───────────

    #[test]
    fn a_toggle_legend_is_announced_as_a_group_of_toggle_buttons_carrying_their_state() {
        let nodes = three([true, false, true])
            .with_interaction(LegendInteraction::Toggle)
            .access_nodes(400, &LegendPostures::at_rest(), Some("chart.legend.1"));
        assert_eq!(nodes.len(), 4, "the group plus one node per entry");
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(nodes[0].name.as_deref(), Some("Series"));
        for (index, node) in nodes[1..].iter().enumerate() {
            assert_eq!(node.role, AriaRole::Button, "entry {index}");
            // `aria-pressed`, not `aria-checked` — the WAI-ARIA rule for a
            // button, and the reason the on-ness is on this axis.
            assert_eq!(
                node.state.checked,
                Some(index != 1),
                "entry {index} on-ness"
            );
        }
        assert!(nodes[2].state.focused, "the focused entry says so");
    }

    #[test]
    fn a_paint_legend_announces_no_control_at_all() {
        // Its swatches and labels are already text in the tree. Announcing
        // controls nothing can reach is the lie this module exists to prevent —
        // and it is what the floor does, in the other direction, by announcing
        // nothing whatever for a legend a pointer CAN click.
        let at_rest = LegendPostures::at_rest();
        assert!(
            three([true; 3])
                .access_nodes(400, &at_rest, None)
                .is_empty()
        );
        assert!(three([true; 3]).group(400, &at_rest).is_none());
    }

    #[test]
    fn an_entry_the_pointer_is_on_says_so_and_a_locked_one_is_announced_disabled() {
        // The one axis a chart cannot derive: hover and press live with whoever
        // runs the pointer, so the legend takes them back rather than announcing
        // every entry at rest. `Locked` reaches the tree as `disabled`, which is
        // what makes an entry the consumer has frozen discoverable rather than
        // silently inert.
        let postures = LegendPostures::at_rest()
            .under(0, &ChipPosture::Hover)
            .under(1, &ChipPosture::Locked);
        let nodes = three([true; 3])
            .with_interaction(LegendInteraction::Toggle)
            .access_nodes(400, &postures, None);
        assert!(nodes[1].state.hovered, "entry 0 is under the pointer");
        assert!(nodes[2].state.disabled, "entry 1 is locked");
        assert!(
            !nodes[3].state.hovered && !nodes[3].state.disabled,
            "entry 2 was not reported, so it is at rest"
        );
    }

    #[test]
    fn a_posture_reported_past_the_roster_displaces_nobody() {
        // The parallel-array hazard, closed by construction: reporting entry 9
        // on a three-entry legend cannot land on entry 2 the way a zipped list
        // of the wrong length used to lose the tail of its own legend.
        let postures = LegendPostures::at_rest().under(9, &ChipPosture::Pressed);
        let nodes = three([true; 3])
            .with_interaction(LegendInteraction::Toggle)
            .access_nodes(400, &postures, None);
        assert!(
            nodes[1..].iter().all(|n| !n.state.pressed),
            "no entry took a posture that was not addressed to it"
        );
    }

    // ─── the keyboard's roster is the roster that was drawn ──────────────

    #[test]
    fn a_row_too_narrow_to_seat_every_entry_announces_only_what_it_drew() {
        // 100px seats one entry beside the `+2` marker. The paint drops two, so
        // the chip row must drop the same two: a focusable control announced
        // for a part that was never painted is unreachable by pointer and by
        // keyboard alike, and would be the same defect one layer along.
        let legend = three([true; 3]).with_interaction(LegendInteraction::Toggle);
        let seating = legend.seating(100);
        assert_eq!(seating.shown, 1);
        assert_eq!(seating.dropped, 2);

        let row = legend
            .group(100, &LegendPostures::at_rest())
            .expect("a toggle legend has a chip row");
        assert_eq!(row.len(), seating.shown, "the roster is what was drawn");
        assert_eq!(row.stops().len(), 1, "one Tab stop, for the drawn entry");

        let scene = rooted(legend.scene(0, 6, 100, &ChartStyle::default()));
        assert!(find(&scene, "chart.legend.0").is_some(), "entry 0 drawn");
        assert!(find(&scene, "chart.legend.1").is_none(), "entry 1 dropped");
        assert!(
            find(&scene, "chart.legend.overflow").is_some(),
            "and the drop is named rather than silent"
        );
    }

    #[test]
    fn a_press_still_reaches_a_part_the_row_was_too_narrow_to_draw() {
        // Stated in `press`'s doc and asserted here: the wire is not bounded by
        // the pane's width. The pointer and the keyboard are, which is what the
        // `+N` marker reports.
        let legend = three([true; 3]).with_interaction(LegendInteraction::Toggle);
        assert_eq!(legend.seating(100).shown, 1);
        let Ok(Outcome::Set { on, .. }) = legend.press(2) else {
            panic!("the roster press works over every part")
        };
        assert_eq!(on, vec![true, true, false]);
    }

    // ─── an off part reads as off ────────────────────────────────────────

    #[test]
    fn an_off_entry_greys_its_swatch_and_keeps_its_slot() {
        let style = ChartStyle::default();
        let scene = rooted(
            three([true, false, true])
                .with_interaction(LegendInteraction::Toggle)
                .scene(0, 6, 400, &style),
        );
        let swatch_fill = |tag: &str| {
            let Some(Scene::Container(entry)) = find(&scene, tag) else {
                panic!("{tag} is an entry container")
            };
            let Scene::Box(swatch) = &entry.children[0] else {
                panic!("{tag}'s first child is its swatch")
            };
            swatch.style.fill
        };
        assert_eq!(swatch_fill("chart.legend.0"), Color::rgb(0x00, 0x20, 0x30));
        assert_eq!(
            swatch_fill("chart.legend.1"),
            style.label,
            "an off part greys rather than vanishing"
        );
        assert!(
            find(&scene, "chart.legend.1").is_some(),
            "and keeps its slot, which is the toggle back on"
        );
    }

    // ─── an empty legend is an answer ────────────────────────────────────

    #[test]
    fn a_legend_naming_no_parts_paints_nothing_and_says_so() {
        let empty = Legend::new("chart", "Bars", Vec::new());
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.width(300), 0);
        assert!(
            empty.scene(10, 6, 300, &ChartStyle::default()).is_empty(),
            "no row, and no `+0` marker either"
        );
    }

    // ─── every chart kind answers, and the roster is read from the source ─

    #[test]
    fn every_chart_kind_in_this_crate_declares_its_legend() {
        // The gate that makes the breadth claim survive a chart kind added
        // later. It reads the crate's own source rather than a list kept here,
        // because a hand-kept roster is what this round found the charting axis
        // relying on: "two of seven support the gesture" was true, unwritten,
        // and discoverable only by reading seven files.
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut declared: Vec<String> = Vec::new();
        let mut implemented: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&src).expect("the crate has a src directory") {
            let path = entry.expect("a readable directory entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a readable module");
            for line in text.lines() {
                // An anchored declaration, not a substring search: the name is
                // taken from the line that DEFINES the type, so a mention of a
                // chart in a doc comment or a `use` cannot enrol one.
                if let Some(rest) = line.strip_prefix("pub struct ") {
                    let name = rest.trim_end_matches(" {");
                    if name.ends_with("Chart") && !name.contains('<') {
                        declared.push(name.to_string());
                    }
                }
                if let Some(rest) = line.strip_prefix("impl ChartLegend for ") {
                    implemented.push(rest.trim_end_matches(" {").to_string());
                }
            }
        }
        declared.sort();
        implemented.sort();
        assert!(
            declared.len() >= 7,
            "the census found only {declared:?} — it is reading the wrong thing"
        );
        assert_eq!(
            declared, implemented,
            "every chart kind must answer what its legend is; a kind in the \
             first list and not the second cannot be asked"
        );
    }
}
