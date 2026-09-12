//! Which role is **ink on which ground**, the standard each pairing is held
//! to, and whether the two palettes agree about it.
//!
//! # The fact that lived only in a name
//!
//! [`ColorRole::OnSurface`] is ink on [`ColorRole::Surface`]. Every reader of
//! this crate knows that, and until this module nothing in the tree could act
//! on it: the pairing was carried by the word `On` in an identifier and by
//! prose in a doc comment, so no gate could ask *is this palette legible?* and
//! none did. [`crate::contrast::contrast_ratio`] has existed since R1546 —
//! built for a chart's ink on a colour ramp — and had no table of palette pairs
//! to run over, so the instrument was here and the question was never put to it.
//!
//! That is this repository's recurring class — a fact stated in a name or a
//! comment, which reads like a rule and cannot be enforced like one. Here it
//! had a measurable cost: the dark palette's own constructor documents itself
//! as *"the accent lightened so the dark surface keeps WCAG AA contrast on
//! every paired role"*, and measured at R1807 that claim was **false for one
//! pair** — `inverse_primary` on `inverse_surface` was `3.56`, under the `4.5`
//! its own documentation says it clears, while the light palette's same pairing
//! was `7.75`. Nothing failed, because nothing asked.
//!
//! # Two different questions, deliberately kept apart
//!
//! A palette can be wrong in two unrelated ways, and collapsing them produces a
//! gate that cannot be acted on:
//!
//! * **Absolute** — a pairing does not clear its floor. That is a defect in
//!   that palette, whatever the other one does.
//! * **Parity** — the two palettes *disagree*: a pairing clears its floor in
//!   one and fails in the other. That is the defect a person meets by switching
//!   theme and finding the interface got worse, and it is what an application
//!   claiming light/dark parity is claiming not to have.
//!
//! [`parity`] reports them separately. A pairing that fails **identically** in
//! both palettes is not a parity defect — the two agree, and what is wrong with
//! it is absolute. Keeping that distinct is what lets the parity gate stay
//! honest while an absolute shortfall is carried openly.
//!
//! ★★★★★ R1839 — **the one shortfall it was carrying is repaid, and the
//! premise for carrying it was measured false.** R1807 wrote here that
//! `outline` was short at `1.82` light / `1.81` dark and that the reason not
//! to simply raise it was that *this crate has one `outline` role where a full
//! design system separates a component boundary from a decorative divider* —
//! so any single floor would be wrong for half the uses. That is a claim about
//! a POPULATION and nobody had counted it.
//!
//! Counted at R1839 with [`stroke_census`], over the six painted screens of
//! the analysis tool this vocabulary is judged against: **97 boundary marks
//! and 2 divider marks.** The role does one job, not two. The split would have
//! served two marks and cost a palette field, a wire name, `ColorRole::all()`
//! and two exhaustive gates — and the decorative half is already outside the
//! role wherever a screen needs it (`hello-analyzer-shell`'s canvas hairline
//! is an app constant whose own comment says it "is not a theme role").
//!
//! So the repair is the one R1807 could not justify without the count: the
//! floor stands, and the VALUE moves to clear it. ⇒ **a shortfall carried for
//! a reason is only carried honestly while the reason is measured.**
//!
//! ★★★★★ R2152 — **and the count that settled it was taken with the wrong
//! predicate.** R1839's own sentence for what it counted was *WCAG 1.4.11 holds
//! a component boundary to 3:1 and asks nothing of a decorative divider*, and
//! 1.4.11 says no such thing: what it holds to 3:1 is *visual information
//! **required to identify** user interface components and states*. A box edge
//! on a box the reader already finds by its fill is not required to identify
//! anything, and the `border` slot cannot tell that box from a text field whose
//! hairline is all there is. See [`StrokeKind`] for the third position the
//! vocabulary was missing.
//!
//! ⚠ The correction does **not** overturn R1839's conclusion, and saying so is
//! the point: re-measured at R2152 the role still does one job — 66
//! `Identifying` and 7 `Redundant` box edges against 3 dividers, where R1839
//! read 97 and 2 over a tool that then had six screens and now has eight. A
//! right answer reached through a wrong predicate stays wrong as a *method*,
//! because the next question it is asked is the one it gets wrong: *does this
//! palette clear what it owes* has a different population from *is this role a
//! boundary role*, and only the second was ever asked.
//!
//! # Why a table and not a function on the role
//!
//! Ink-over-ground is a **relation**, not a map. `OnSurface` is ink on
//! `Surface` *and* on all four container tiers; `Accent` is a ground for
//! `OnAccent` and itself ink on `Surface`. A `fn ground(self) -> ColorRole`
//! would have to pick one and would silently stop checking the rest, so the
//! pairing is a declared table every consumer iterates.

use std::collections::{BTreeMap, BTreeSet};

use crate::contrast::contrast_ratio;
use crate::theme::{ColorRole, Theme};

/// The legibility standard a pairing is held to.
///
/// WCAG 2.x numbers, named for what the pairing *is* rather than for the
/// number, so a reader can tell a mis-declared pairing from a mis-typed
/// threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Floor {
    /// Body text on its ground — WCAG AA, `4.5:1`.
    Text,
    /// A component boundary a person must be able to find: the edge of a
    /// field, a focus ring, the frame of a control — WCAG AA non-text,
    /// `3.0:1`.
    Boundary,
}

impl Floor {
    /// Every floor, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 2] = [Self::Text, Self::Boundary];

    /// The ratio a pairing must reach.
    #[must_use]
    pub const fn ratio(self) -> f32 {
        match self {
            Self::Text => 4.5,
            Self::Boundary => 3.0,
        }
    }

    /// Stable name, for a report line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Boundary => "boundary",
        }
    }
}

/// One declared pairing: `ink` is painted on `ground`, and must clear `floor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pairing {
    /// The foreground role.
    pub ink: ColorRole,
    /// The role it is painted on.
    pub ground: ColorRole,
    /// The standard this pairing is held to.
    pub floor: Floor,
}

impl Pairing {
    /// The measured ratio for this pairing in `theme`.
    #[must_use]
    pub fn ratio_in(&self, theme: &Theme) -> f32 {
        contrast_ratio(theme.resolve(self.ink), theme.resolve(self.ground))
    }

    /// Whether this pairing clears its floor in `theme`.
    #[must_use]
    pub fn clears_in(&self, theme: &Theme) -> bool {
        self.ratio_in(theme) >= self.floor.ratio()
    }
}

const fn text(ink: ColorRole, ground: ColorRole) -> Pairing {
    Pairing {
        ink,
        ground,
        floor: Floor::Text,
    }
}

const fn boundary(ink: ColorRole, ground: ColorRole) -> Pairing {
    Pairing {
        ink,
        ground,
        floor: Floor::Boundary,
    }
}

/// **Every ink-over-ground pairing this palette vocabulary declares.**
///
/// The `On*` roles pair with the ground their name states. Beyond those, three
/// pairings exist because the interface actually paints them and no name says
/// so: an accent, an error and a warning tone are all used as *text on the
/// plain surface* (a link, an inline error, an inline caution), and body ink is
/// painted on every container tier, not only on `Surface`.
///
/// A role that is only ever a ground appears here only as one.
pub const PAIRINGS: &[Pairing] = &[
    // Body ink, on the plain surface and on every container tier it is drawn on.
    text(ColorRole::OnSurface, ColorRole::Surface),
    text(ColorRole::OnSurface, ColorRole::SurfaceContainerLow),
    text(ColorRole::OnSurface, ColorRole::SurfaceContainer),
    text(ColorRole::OnSurface, ColorRole::SurfaceContainerHigh),
    text(ColorRole::OnSurface, ColorRole::SurfaceContainerHighest),
    // Secondary ink. The container-highest pairing is the tightest in the
    // vocabulary and is pinned deliberately: it is where a palette tweak first
    // stops being legible.
    text(ColorRole::OnSurfaceMuted, ColorRole::Surface),
    text(
        ColorRole::OnSurfaceMuted,
        ColorRole::SurfaceContainerHighest,
    ),
    // The named `On*` pairings.
    text(ColorRole::OnAccent, ColorRole::Accent),
    text(ColorRole::OnError, ColorRole::Error),
    text(ColorRole::OnErrorContainer, ColorRole::ErrorContainer),
    text(ColorRole::OnWarning, ColorRole::Warning),
    text(ColorRole::OnSuccess, ColorRole::Success),
    text(ColorRole::OnInfo, ColorRole::Info),
    // ★★★★★ R2020 — the other three container pairs, which arrived with a
    // painted consumer: a status badge is drawn on its state's container now,
    // so these are grounds a person reads words off rather than tokens.
    //
    // ⚠ The pairing declared is the FOREGROUND on the container and NOT the
    // container on the surface, and that is a deliberate limit rather than an
    // oversight. Measured on this crate's own palettes, a container reads about
    // **1.3** on the light surface and **2.0** on the dark one — `error`'s has
    // read that way since R590 — so a boundary pairing here would declare a
    // floor the whole tier fails. A filled badge is found by its WORD, which
    // clears the text floor with headroom to spare; the tint groups the word,
    // it does not carry it. A design that needed the ground itself to be
    // findable would have to move four container values, which is a decision
    // about a palette and not a rule about a vocabulary ⇒ the residue is stated
    // in `Theme::light`'s own comment rather than hidden behind a pairing
    // nobody could hold.
    text(ColorRole::OnWarningContainer, ColorRole::WarningContainer),
    text(ColorRole::OnSuccessContainer, ColorRole::SuccessContainer),
    text(ColorRole::OnInfoContainer, ColorRole::InfoContainer),
    text(ColorRole::InverseOnSurface, ColorRole::InverseSurface),
    // A snackbar's action label: the accent re-toned for the inverted ground.
    text(ColorRole::InversePrimary, ColorRole::InverseSurface),
    // Tones used as inline text on the plain surface.
    text(ColorRole::Accent, ColorRole::Surface),
    text(ColorRole::Error, ColorRole::Surface),
    text(ColorRole::Warning, ColorRole::Surface),
    // ★★★★★ R2012 — and these two are declared because a mark that was ALREADY
    // being painted here had no entry.
    //
    // The analysis shell's toast draws a small filled disc on the status band
    // to say which kind of thing was said, and R1719's own comment calls it the
    // only thing that tells a confirmation from a refusal. It was painted in
    // `inverse_primary` — a role whose declared ground is `inverse_surface` —
    // on the plain surface, which against THESE palettes measures **1.70**
    // light and **2.17** dark, under even the non-text floor in both. (That
    // screen binds a magenta of its own for the role and was legible by
    // accident; the pairing is what was wrong, not its palette.) This table
    // could not see it either way: `inverse_primary on surface` is not a
    // pairing anybody declared, and a pairing nobody declares is a pairing
    // nobody checks.
    //
    // ⚠ The residue, stated rather than hidden: declaring the two new tones
    // fixes the mark that moved onto them, and it does NOT close the class.
    // Nothing here reads what the screens paint, so the next undeclared
    // ink-over-ground pairing will be just as quiet ⇒
    // [[debt-a-painted-pairing-outside-this-table-is-checked-by-nothing]].
    text(ColorRole::Success, ColorRole::Surface),
    text(ColorRole::Info, ColorRole::Surface),
    // The one boundary pairing, and one is the right number: re-measured at
    // R2152 over the analysis tool's eight painted screens, `outline` draws 73
    // box edges and 3 dividers, so a single `Floor::Boundary` is right for 96%
    // of what it paints. (R1839 read 99 marks over six screens where R2152
    // reads 76 over eight; the population moved over 313 rounds and nothing
    // re-took the count.)
    //
    // ⚠ 73 is the count that decides the VOCABULARY question — is this role a
    // boundary role at all. It is NOT the count that decides whether a palette
    // clears what it owes: 66 of the 73 are `StrokeKind::Identifying` and 7 are
    // `Redundant`, and only the first carry the floor. A pairing is the wrong
    // grain to ask the second question at, because a pairing has no marks; ask
    // `stroke_census`.
    boundary(ColorRole::Outline, ColorRole::Surface),
];

/// How one pairing behaves across the two palettes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// The pairing read.
    pub pairing: Pairing,
    /// Its ratio in the light palette.
    pub light: f32,
    /// Its ratio in the dark palette.
    pub dark: f32,
}

impl Reading {
    /// Whether the two palettes reach the same verdict about this pairing —
    /// both clearing its floor, or both failing it.
    #[must_use]
    pub fn agrees(&self) -> bool {
        let floor = self.pairing.floor.ratio();
        (self.light >= floor) == (self.dark >= floor)
    }

    /// Whether it clears its floor in *both*.
    #[must_use]
    pub fn clears_both(&self) -> bool {
        let floor = self.pairing.floor.ratio();
        self.light >= floor && self.dark >= floor
    }

    /// The reading as a sentence, for a report or a failure message.
    #[must_use]
    pub fn say(&self) -> String {
        format!(
            "{} on {} ({} floor {:.1}): light {:.2}, dark {:.2}",
            self.pairing.ink.name(),
            self.pairing.ground.name(),
            self.pairing.floor.name(),
            self.pairing.floor.ratio(),
            self.light,
            self.dark,
        )
    }
}

/// What comparing two palettes over [`PAIRINGS`] found.
///
/// Every declared pairing lands in exactly one of the three sets, so the report
/// accounts for the whole table rather than listing only what went wrong.
#[derive(Debug, Clone, Default)]
pub struct Parity {
    clear: BTreeSet<String>,
    disagree: BTreeMap<String, Reading>,
    short_in_both: BTreeMap<String, Reading>,
}

impl Parity {
    /// Pairings that clear their floor in both palettes.
    #[must_use]
    pub const fn clear(&self) -> &BTreeSet<String> {
        &self.clear
    }

    /// **The parity defects**: pairings one palette clears and the other does
    /// not. This is the set an application claiming light/dark parity is
    /// claiming to be empty.
    #[must_use]
    pub const fn disagree(&self) -> &BTreeMap<String, Reading> {
        &self.disagree
    }

    /// Pairings short of their floor in **both** palettes — an absolute
    /// shortfall the two agree about. Not a parity defect; a different one,
    /// reported rather than folded in.
    #[must_use]
    pub const fn short_in_both(&self) -> &BTreeMap<String, Reading> {
        &self.short_in_both
    }

    /// Whether the two palettes agree about every declared pairing.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.disagree.is_empty()
    }

    /// Every pairing considered — the three sets together, which equals
    /// [`PAIRINGS`] by construction.
    #[must_use]
    pub fn accounted(&self) -> BTreeSet<&str> {
        self.clear
            .iter()
            .map(String::as_str)
            .chain(self.disagree.keys().map(String::as_str))
            .chain(self.short_in_both.keys().map(String::as_str))
            .collect()
    }

    /// The disagreements as sentences, in a stable order.
    #[must_use]
    pub fn faults(&self) -> Vec<String> {
        self.disagree.values().map(Reading::say).collect()
    }
}

/// The name a pairing is addressed by in a [`Parity`] report.
#[must_use]
pub fn pairing_name(pairing: &Pairing) -> String {
    format!("{}/{}", pairing.ink.name(), pairing.ground.name())
}

/// Read every declared [`PAIRINGS`] entry in both palettes and sort it.
///
/// The three sets are filled in one pass over the table, so
/// [`Parity::accounted`] equals the table by construction — a pairing cannot
/// fall out of the report.
#[must_use]
pub fn parity(light: &Theme, dark: &Theme) -> Parity {
    let mut out = Parity::default();
    for pairing in PAIRINGS {
        let reading = Reading {
            pairing: *pairing,
            light: pairing.ratio_in(light),
            dark: pairing.ratio_in(dark),
        };
        let name = pairing_name(pairing);
        if !reading.agrees() {
            out.disagree.insert(name, reading);
        } else if reading.clears_both() {
            out.clear.insert(name);
        } else {
            out.short_in_both.insert(name, reading);
        }
    }
    out
}

/// The parity of this crate's own canonical palettes — what a consumer that
/// has not replaced either palette is shipping.
#[must_use]
pub fn canonical_parity() -> Parity {
    parity(&Theme::light(), &Theme::dark())
}

/// ★★★★★ (R2019 §5.50) **Every declared pairing ONE palette does not clear**,
/// named and with the ratio it measured, in [`PAIRINGS`] order.
///
/// [`parity`] answers about a PAIR, because it exists to ask whether a light
/// and a dark palette agree. A screen that binds a palette of its own has one
/// palette per mode and no pair to compare, so until this existed there was no
/// way to ask the table about it — and measured at R2018.1, nothing did:
/// outside this module's own tests, `parity`, [`canonical_parity`] and
/// [`Pairing::clears_in`] had **no callers at all**. The canonical palettes
/// were held to the table and the shipped ones were not.
///
/// ⚠ **Read the answer as a LIST TO PIN, not as a thing to drive to zero.** A
/// palette is a design decision and several of its tones arrive authored from
/// outside this repository, so a gate that demanded an empty list would be
/// demanding the right to change somebody else's colours. What a gate can
/// honestly do is fix the list, so the day a pairing joins it, it is red.
#[must_use]
pub fn shortfalls(theme: &Theme) -> Vec<(String, f32)> {
    PAIRINGS
        .iter()
        .filter(|pairing| !pairing.clears_in(theme))
        .map(|pairing| (pairing_name(pairing), pairing.ratio_in(theme)))
        .collect()
}

/// ★★★★★ R1839 — **what a mark painted in a role's colour is DOING**, which
/// is the question a floor cannot be chosen without answering.
///
/// # Why this is derived from the frame and not from the source
///
/// The obvious census is `grep ColorRole::Outline`, and it answers the wrong
/// question: measured at R1839 it finds 145 MENTIONS, of which the large
/// majority are `theme.resolve(...)` binding a local that is then used several
/// times, several times differently. A mention is not a use. What the standard
/// is about is the mark on the frame, so the mark on the frame is what this
/// counts.
///
/// # ★★★★★ R2152 — the third position, and why two were not enough
///
/// R1839 wrote here that *WCAG 1.4.11 holds a component boundary to 3:1 and
/// asks nothing of a decorative divider*, and split this enum on that sentence.
/// The sentence is a misreading, and it is the kind that cannot be caught by
/// reading the code: **1.4.11 has no category called "component boundary".**
/// Its predicate is
///
/// > Visual information **required to identify** user interface components and
/// > states.
///
/// *Required to identify* is a property of the mark's SETTING, not of the slot
/// it was declared in. A text field whose only mark is a hairline owes the
/// ratio because removing the hairline removes the field; the same hairline
/// around a card the reader already finds by its own fill owes nothing, because
/// what identifies that card is the fill and the fill is what the standard then
/// holds to 3:1. One box edge, two verdicts, and the `border` slot cannot tell
/// them apart.
///
/// So the classifier asks what the standard asks: **is there anything else?**
/// [`Identifying`](Self::Identifying) is a box edge with nothing else to lose;
/// [`Redundant`](Self::Redundant) is a box edge on a box its own fill already
/// separates. Only the first carries [`Floor::Boundary`].
///
/// ⚠ The population this splits is not a rounding difference. Measured at
/// R2152 over the analysis tool's eight painted screens: of 73 marks the old
/// classifier called `Boundary`, **66 are `Identifying` and 7 are
/// `Redundant`** — the seven being node cards whose type colour reads 3.94 to
/// 5.58 against the canvas, which owed the floor under the old reading and owe
/// nothing under the standard's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrokeKind {
    /// A box edge that is the **only** visual difference between the box and
    /// what is behind it: remove it and there is nothing there. WCAG 1.4.11's
    /// *visual information required to identify*, and the one kind that owes
    /// [`Floor::Boundary`].
    Identifying,
    /// A box edge on a box whose own fill already clears [`Floor::Boundary`]
    /// against its backdrop. The edge is painted and the standard asks nothing
    /// of it — the fill is what identifies the box, and the fill is what then
    /// carries the ratio.
    Redundant,
    /// The colour fills a box or strokes a path: a rule, a hairline, a grid
    /// line, a tick. Not the edge of anything.
    Divider,
}

impl StrokeKind {
    /// Every arm, so a consumer that must cover the vocabulary names its
    /// members rather than searching for what is missing.
    pub const ALL: [Self; 3] = [Self::Identifying, Self::Redundant, Self::Divider];

    /// The floor this kind of mark is held to, or `None` where the standard
    /// asks nothing.
    #[must_use]
    pub const fn floor(self) -> Option<Floor> {
        match self {
            Self::Identifying => Some(Floor::Boundary),
            Self::Redundant | Self::Divider => None,
        }
    }

    /// Whether this mark is a box edge at all — the question the `border` slot
    /// answers, which is the question R1839 mistook for the standard's.
    #[must_use]
    pub const fn is_box_edge(self) -> bool {
        matches!(self, Self::Identifying | Self::Redundant)
    }

    /// Stable name, for a report line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Identifying => "identifying",
            Self::Redundant => "redundant",
            Self::Divider => "divider",
        }
    }
}

/// Every mark in a painted scene drawn in one colour, by what it is doing.
///
/// The tag of each mark where it has one, so a report can say WHERE and not
/// only how many; untagged marks are counted rather than named, because a
/// count that silently dropped them would understate the population.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StrokeCensus {
    /// Tagged box edges the box has nothing else to be found by, by tag.
    pub identifying: BTreeSet<String>,
    /// Tagged box edges on boxes their own fill already separates, by tag.
    pub redundant: BTreeSet<String>,
    /// Tagged divider marks, by tag.
    pub divider: BTreeSet<String>,
    /// Identifying edges carrying no tag of their own.
    pub identifying_untagged: usize,
    /// Redundant edges carrying no tag of their own.
    pub redundant_untagged: usize,
    /// Divider marks carrying no tag of their own.
    pub divider_untagged: usize,
}

impl StrokeCensus {
    /// **How many marks owe [`Floor::Boundary`]** — the count the standard's
    /// predicate selects, and the one a palette decision has to be made
    /// against.
    #[must_use]
    pub fn identifying(&self) -> usize {
        self.identifying.len() + self.identifying_untagged
    }

    /// How many box edges are painted on boxes that are found without them.
    #[must_use]
    pub fn redundant(&self) -> usize {
        self.redundant.len() + self.redundant_untagged
    }

    /// How many box edges there are in all, owing the floor or not.
    ///
    /// This is what [`StrokeKind::Boundary`]'s count meant before R2152 split
    /// it, and it is still the right question for *is this role a boundary
    /// role at all* — which is what the vocabulary decision in [`PAIRINGS`]
    /// turns on. It is the wrong question for *what does this palette owe*.
    ///
    /// [`StrokeKind::Boundary`]: StrokeKind
    #[must_use]
    pub fn boundaries(&self) -> usize {
        self.identifying() + self.redundant()
    }

    /// How many divider marks there are in all.
    #[must_use]
    pub fn dividers(&self) -> usize {
        self.divider.len() + self.divider_untagged
    }

    /// Fold another scene's census into this one, so a sweep over several
    /// screens answers about the application rather than about a frame.
    pub fn absorb(&mut self, other: &Self) {
        self.identifying.extend(other.identifying.iter().cloned());
        self.redundant.extend(other.redundant.iter().cloned());
        self.divider.extend(other.divider.iter().cloned());
        self.identifying_untagged += other.identifying_untagged;
        self.redundant_untagged += other.redundant_untagged;
        self.divider_untagged += other.divider_untagged;
    }
}

/// Census one painted scene for marks drawn in `colour`, over `ground`.
///
/// ★ Two derivable rules, and together they are the whole judgment: a colour in
/// the `border` slot strokes the edge of a box and a colour anywhere else does
/// not; and a box edge is *required to identify* its box exactly when the box's
/// own fill does not clear [`Floor::Boundary`] against what is behind it. Both
/// are read off the frame, which is the point — a hand classification of 145
/// sites would be one person's unreviewed reading, and this repository has a
/// standing debt about exactly that.
///
/// `ground` is what is behind the whole scene: the census resolves each box's
/// backdrop as its nearest enclosing filled ancestor, and `ground` is the
/// answer for a box that has none. A caller passes the surface its window is
/// cleared to.
///
/// # What a translucent fill is compared as
///
/// Composited over its backdrop first, per [`contrast`](crate::contrast)'s
/// stated contract — that module ignores alpha and says so, and tells a caller
/// holding a translucent layer to composite it against its own backdrop and
/// pass the result. A half-transparent tint is nearer its backdrop than its
/// nominal colour is, so comparing the nominal colour would report a
/// distinction the reader cannot see and file the mark as `Redundant`.
///
/// ⚠ Its stated limits, both deliberate:
///
/// * A box FILLED with the colour and one pixel tall is a rule, and a box
///   filled with it and forty pixels tall is a block — both count as `Divider`.
///   Neither is the edge of a component, so neither is what 1.4.11 is about,
///   and inventing a height threshold would put a number in the classifier
///   that no standard supports.
/// * The backdrop is the nearest filled **ancestor**, not whatever pixel
///   happens to be under the box. A box absolutely positioned over a sibling's
///   fill is compared against the container behind them both. Reading the true
///   backdrop needs a rasteriser, and this census runs on the scene.
#[must_use]
pub fn stroke_census(
    scene: &crate::Scene,
    colour: crate::style::Color,
    ground: crate::style::Color,
) -> StrokeCensus {
    use crate::Scene;
    let mut out = StrokeCensus::default();
    scene.for_each_node(&mut |visit| {
        let tag = visit.node.tag();
        let mut note = |kind: StrokeKind| {
            let (tagged, untagged) = match kind {
                StrokeKind::Identifying => (&mut out.identifying, &mut out.identifying_untagged),
                StrokeKind::Redundant => (&mut out.redundant, &mut out.redundant_untagged),
                StrokeKind::Divider => (&mut out.divider, &mut out.divider_untagged),
            };
            match tag {
                Some(t) => {
                    tagged.insert(t.to_owned());
                }
                None => *untagged += 1,
            }
        };
        let box_style = match visit.node {
            Scene::Box(painted) => Some(&painted.style),
            Scene::Container(painted) => Some(&painted.style),
            Scene::Path(painted) => {
                // A path's stroke is never a box edge, and neither is its fill.
                if painted.style.stroke.is_some_and(|s| s.color == colour)
                    || painted.style.fill == Some(colour)
                {
                    note(StrokeKind::Divider);
                }
                None
            }
            _ => None,
        };
        if let Some(style) = box_style {
            if style.border.is_some_and(|b| b.color == colour) {
                let backdrop = backdrop_of(visit.ancestors, ground);
                note(if separates(style.fill, backdrop) {
                    StrokeKind::Redundant
                } else {
                    StrokeKind::Identifying
                });
            }
            if style.fill == colour {
                note(StrokeKind::Divider);
            }
        }
    });
    out
}

/// What is behind a node: its nearest enclosing filled ancestor, or `ground`.
///
/// Innermost first, because the nearest opaque fill is what the reader sees —
/// an outer container's colour is covered by any inner one that paints.
fn backdrop_of(ancestors: &[&crate::Scene], ground: crate::style::Color) -> crate::style::Color {
    use crate::Scene;
    ancestors
        .iter()
        .rev()
        .find_map(|node| {
            let fill = match node {
                Scene::Box(painted) => painted.style.fill,
                Scene::Container(painted) => painted.style.fill,
                _ => return None,
            };
            (fill.a == u8::MAX).then_some(fill)
        })
        .unwrap_or(ground)
}

/// Whether `fill` on its own separates a box from `backdrop` well enough that
/// a reader finds the box without its edge — [`Floor::Boundary`], which is the
/// ratio 1.4.11 asks of whatever the identifying information turns out to be.
fn separates(fill: crate::style::Color, backdrop: crate::style::Color) -> bool {
    if fill.a == 0 {
        // Nothing was painted, so nothing can separate: the edge is all there
        // is. Short-circuited rather than left to the composite below, which
        // would reach the same verdict through a ratio of exactly 1.0.
        return false;
    }
    let composited = if fill.a == u8::MAX {
        fill
    } else {
        backdrop.lerp(fill, f32::from(fill.a) / 255.0)
    };
    contrast_ratio(composited, backdrop) >= Floor::Boundary.ratio()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★★★★★ R1839 — **the boundary floor is CLEARED, and the value that
    /// clears it is derived rather than chosen.**
    ///
    /// R1807 measured `outline` at `1.82` light and `1.81` dark against a `3.0`
    /// floor and reported the shortfall openly rather than hiding it. This is
    /// the repayment, and the two greys are the NEAREST value to each palette's
    /// previous one that clears — a search, not a taste: from `#c0c0c0`
    /// darkening one step at a time, and from `#404040` lightening one step at
    /// a time, stopping at the first that reaches `3.0`.
    ///
    /// Re-deriving them is this test's second half, so the pinned values in
    /// `theme.rs` cannot drift away from the rule that produced them: a
    /// nearer-to-the-old value that also clears would fail here.
    #[test]
    fn r1839_the_boundary_floor_is_cleared_by_the_nearest_grey_that_clears_it() {
        use crate::style::Color;
        let light = Theme::light();
        let dark = Theme::dark();

        // The floor is actually cleared, which is the claim.
        let boundary: Vec<&Pairing> = PAIRINGS
            .iter()
            .filter(|p| p.floor == Floor::Boundary)
            .collect();
        assert!(!boundary.is_empty(), "there is a boundary pairing to check");
        for pairing in &boundary {
            assert!(
                pairing.clears_in(&light) && pairing.clears_in(&dark),
                "{} must clear {} in both palettes \u{2014} light {:.2}, dark {:.2}",
                pairing_name(pairing),
                Floor::Boundary.ratio(),
                pairing.ratio_in(&light),
                pairing.ratio_in(&dark),
            );
        }

        // And the value is the nearest one to the old that does. Searching
        // from the OLD value in the direction the repair moved it: the first
        // grey to clear is the one shipped, so a darker light outline or a
        // lighter dark one would be a choice this rule does not support.
        let nearest = |from: u8, toward_dark: bool, ground: Color| -> u8 {
            let mut v = from;
            loop {
                let c = Color::rgb(v, v, v);
                if contrast_ratio(c, ground) >= Floor::Boundary.ratio() {
                    return v;
                }
                v = if toward_dark { v - 1 } else { v + 1 };
            }
        };
        assert_eq!(
            light.outline,
            {
                let v = nearest(0xc0, true, light.surface);
                Color::rgb(v, v, v)
            },
            "the light outline is the nearest grey below #c0c0c0 that clears",
        );
        assert_eq!(
            dark.outline,
            {
                let v = nearest(0x40, false, dark.surface);
                Color::rgb(v, v, v)
            },
            "the dark outline is the nearest grey above #404040 that clears",
        );
    }

    /// ★★★★★ R1839 — **no declared pairing is short of its floor in both
    /// palettes**, which is the ratchet R1807 could not set.
    ///
    /// `short_in_both` was built to CARRY a shortfall openly rather than fold
    /// it into the parity verdict, and it carried exactly one: `outline`. With
    /// that repaid the set is empty, and asserting it is what stops the next
    /// palette edit from re-opening the same hole silently.
    ///
    /// ⚠ It is the whole set and not `outline` by name, deliberately: a gate
    /// naming one pairing would pass while a different one went short.
    #[test]
    fn r1839_no_pairing_is_short_of_its_floor_in_both_palettes() {
        let report = canonical_parity();
        assert!(
            report.short_in_both().is_empty(),
            "an absolute shortfall the two palettes agree about: {:?}",
            report
                .short_in_both()
                .values()
                .map(Reading::say)
                .collect::<Vec<_>>(),
        );
        assert!(report.holds(), "and the parity verdict still holds");
    }

    /// ★★★★★ R2020 — **every state's own two pairings are declared, and the
    /// question is asked of the STATE rather than of this table.**
    ///
    /// A table is a list somebody keeps, and this repository's standing finding
    /// about lists is that they rot in the direction that hides work: the tier
    /// went from one state to two to four while `error` was the only one whose
    /// container pair was declared here, and nothing said so, because a pairing
    /// nobody declares is a pairing nobody checks — this module's own opening
    /// sentence. R2012 met the same shape from the other side and wrote it down:
    /// *"the next undeclared ink-over-ground pairing will be just as quiet."*
    ///
    /// So the population is [`StateTone::ALL`], which is derived from the enum
    /// by `VariantCensus`, and the claim is that a state's tone pairing AND its
    /// container pairing are both in the table. A fifth state added to the
    /// vocabulary fails this the moment it exists, which is the point: it is
    /// the *shape* of a state that is being asserted, not the membership of
    /// eight particular rows.
    #[test]
    fn r2020_every_state_declares_both_of_its_pairings() {
        use crate::theme::StateTone;

        // Keyed by the roles' wire names rather than by the roles: `ColorRole`
        // is `Eq + Hash` and not `Ord`, and a name is what the message has to
        // print anyway.
        let declared: BTreeSet<(&str, &str)> = PAIRINGS
            .iter()
            .map(|p| (p.ink.name(), p.ground.name()))
            .collect();
        let mut asked = 0_usize;
        for tone in StateTone::ALL {
            for (ink, ground) in [
                (tone.on_tone(), tone.tone()),
                (tone.on_container(), tone.container()),
            ] {
                assert!(
                    declared.contains(&(ink.name(), ground.name())),
                    "`{}` is painted on `{}` by every screen that shows a `{}` \
                     state, and this table does not declare the pairing — so \
                     nothing holds it to a floor",
                    ink.name(),
                    ground.name(),
                    tone.word(),
                );
                asked += 1;
            }
        }
        // The denominator, so a `StateTone::ALL` that shrank could not pass by
        // having less to ask about.
        assert_eq!(
            asked,
            2 * StateTone::ARMS,
            "two pairings for each of the vocabulary's states"
        );
        println!("[r2020] {asked} state pairing(s) declared");
    }

    #[test]
    fn r1807_every_declared_pairing_is_accounted_for_exactly_once() {
        let report = canonical_parity();
        let declared: BTreeSet<String> = PAIRINGS.iter().map(pairing_name).collect();
        assert_eq!(
            report.accounted(),
            declared.iter().map(String::as_str).collect::<BTreeSet<_>>(),
            "the report covers the whole table"
        );
        let counted = report.clear().len() + report.disagree().len() + report.short_in_both().len();
        assert_eq!(
            counted,
            PAIRINGS.len(),
            "and each pairing lands in exactly one set"
        );
    }

    /// ★★★★★ The property `dashboard.t2.9` claims — *light and dark parity* —
    /// asserted as a SET rather than as a sentence.
    ///
    /// Measured at R1807 this was FALSE: `inverse_primary` on `inverse_surface`
    /// read `7.75` light and `3.56` dark, so a snackbar's action label cleared
    /// WCAG AA in one theme and not the other, while the dark palette's own
    /// constructor documented itself as keeping AA "on every paired role".
    /// Nothing failed, because nothing asked.
    #[test]
    fn r1807_the_two_palettes_agree_about_every_declared_pairing() {
        let report = canonical_parity();
        assert!(
            report.holds(),
            "a pairing legible in one palette and not the other:\n  {}",
            report.faults().join("\n  ")
        );
    }

    /// ★★★★★ **The value the fix replaced still fails this gate.**
    ///
    /// A gate that goes green the same round its subject is repaired proves
    /// nothing on its own — the repair could be what is green, and the gate a
    /// tautology beside it. This puts the pre-R1807 dark `inverse_primary`
    /// back and requires the report to name it, so the gate is pinned against
    /// the real defect and not only against a synthetic one.
    #[test]
    fn r1807_the_value_this_round_replaced_would_still_be_caught() {
        let light = Theme::light();
        let mut dark = Theme::dark();
        dark.inverse_primary = crate::style::Color::rgb(0x19, 0x76, 0xd2);
        let report = parity(&light, &dark);
        assert!(
            !report.holds(),
            "the pre-R1807 palette must not pass the gate that found it"
        );
        let reading = report
            .disagree()
            .get("inverse_primary/inverse_surface")
            .expect("and it is named, not merely counted");
        assert!(
            reading.light >= Floor::Text.ratio() && reading.dark < Floor::Text.ratio(),
            "legible in one palette and not the other: {}",
            reading.say()
        );
    }

    /// The gate above passes vacuously if the table is empty or if every
    /// pairing is trivially identical, so the fixture is pinned here.
    #[test]
    fn r1807_the_pairing_table_is_not_vacuous() {
        assert!(PAIRINGS.len() >= 15, "the table covers the vocabulary");
        let light = Theme::light();
        let dark = Theme::dark();
        for pairing in PAIRINGS {
            assert!(
                (pairing.ratio_in(&light) - pairing.ratio_in(&dark)).abs() > f32::EPSILON,
                "{} reads identically in both palettes, so it tests nothing",
                pairing_name(pairing)
            );
        }
    }

    /// ★ An absolute shortfall is carried **openly**, so that closing the
    /// parity gate above cannot be mistaken for "every pairing is legible".
    ///
    /// ★★★★★ R1839 — **this test used to assert that `outline` IS short, and
    /// that shortfall is repaid, so the assertion moved rather than died.**
    ///
    /// R1807 wrote it as a census of the shortfalls *this crate has written
    /// down*, and the honest reading of that intent once the list is empty is
    /// not to delete the test: it is to keep asking whether `short_in_both` and
    /// `disagree` remain DIFFERENT drawers. A shortfall that got folded into
    /// the parity verdict, or a parity defect that got filed as a shortfall,
    /// would make the empty list above meaningless, and only a report built
    /// from a palette that HAS both can tell them apart. So the two are
    /// manufactured here, in one palette pair, and the report must sort them
    /// into different drawers.
    ///
    /// (The live claim that the canonical palettes carry no shortfall at all
    /// is `r1839_no_pairing_is_short_of_its_floor_in_both_palettes`. This one
    /// is about the machinery that would have to keep working for that claim
    /// to mean anything.)
    #[test]
    fn r1807_an_absolute_shortfall_and_a_parity_defect_land_in_different_sets() {
        let mut light = Theme::light();
        let mut dark = Theme::dark();
        // (1) An absolute shortfall the two agree about: body ink at the
        // ground's own colour in both.
        light.on_surface = light.surface;
        dark.on_surface = dark.surface;
        // (2) A parity defect: the muted ink flattened in the DARK palette
        // only, so the two disagree about it.
        dark.on_surface_muted = dark.surface;

        let report = parity(&light, &dark);
        assert!(
            report.short_in_both().contains_key("on_surface/surface"),
            "the pairing both palettes fail is an absolute shortfall: {:?}",
            report.short_in_both().keys().collect::<Vec<_>>(),
        );
        assert!(
            report.disagree().contains_key("on_surface_muted/surface"),
            "the pairing only one palette fails is a parity defect: {:?}",
            report.disagree().keys().collect::<Vec<_>>(),
        );
        assert!(
            !report
                .short_in_both()
                .contains_key("on_surface_muted/surface")
                && !report.disagree().contains_key("on_surface/surface"),
            "\u{2605} and neither is filed as the other \u{2014} folding them \
             together is what would make an empty shortfall list mean nothing",
        );
        assert!(
            report.short_in_both()["on_surface/surface"].agrees(),
            "the two palettes agree that the shortfall is a shortfall",
        );
        assert!(!report.holds(), "and the parity verdict reports the defect");
    }

    #[test]
    fn r1807_a_pairing_reads_the_theme_it_is_given_not_a_constant() {
        let pairing = text(ColorRole::OnSurface, ColorRole::Surface);
        let light = pairing.ratio_in(&Theme::light());
        let dark = pairing.ratio_in(&Theme::dark());
        assert!(light > 4.5 && dark > 4.5);
        // A theme whose ink equals its ground reads 1.0 — the floor of the
        // scale — so the function is answering from the values it was handed.
        let mut flat = Theme::light();
        flat.on_surface = flat.surface;
        assert!((pairing.ratio_in(&flat) - 1.0).abs() < 1e-4);
        assert!(!pairing.clears_in(&flat));
    }

    #[test]
    fn r1807_every_floor_has_a_distinct_name_and_ratio() {
        let names: BTreeSet<&str> = Floor::ALL.iter().map(|f| f.name()).collect();
        assert_eq!(names.len(), Floor::ALL.len());
        assert!(Floor::Text.ratio() > Floor::Boundary.ratio());
    }

    /// A scene of one box with an `outline` border and the fill it was given,
    /// inside a container filled with `backdrop`. The shape every case below
    /// varies one thing in.
    fn one_bordered_box(
        fill: crate::style::Color,
        backdrop: crate::style::Color,
        edge: crate::style::Color,
    ) -> crate::Scene {
        use crate::scene::{BoxNode, ContainerNode, Rect};
        use crate::style::{Border, BoxStyle};

        let inner = BoxNode::new(
            Rect::new(0, 0, 40, 20),
            BoxStyle {
                fill,
                border: Some(Border::new(edge, 1)),
                ..BoxStyle::default()
            },
        )
        .with_tag("inner");
        let mut outer = ContainerNode::new(vec![crate::Scene::Box(inner)]);
        outer.rect = Rect::new(0, 0, 80, 40);
        outer.style = BoxStyle {
            fill: backdrop,
            ..BoxStyle::default()
        };
        outer.tag = Some("outer".into());
        crate::Scene::Container(outer)
    }

    /// ★★★★★ R2152 — **the same border, on two boxes, gets two verdicts**, and
    /// nothing about the border is what decides.
    ///
    /// This is the whole of the correction in one assertion. Before R2152 both
    /// of these were `Boundary` and both owed `Floor::Boundary`, because the
    /// classifier read the slot the colour sat in. WCAG 1.4.11 reads the
    /// setting: the first box is *only* its edge, the second is found by its
    /// own fill and the edge is decoration.
    #[test]
    fn r2152_an_edge_owes_the_floor_only_where_nothing_else_identifies_the_box() {
        use crate::style::Color;
        const GROUND: Color = Color::rgb(0xF6, 0xF7, 0xF9);
        const EDGE: Color = Color::rgb(0xC9, 0xD0, 0xD8);
        // A field: white on the near-white page, 1.07 apart — invisible without
        // its hairline.
        let invisible = one_bordered_box(Color::rgb(0xFF, 0xFF, 0xFF), GROUND, EDGE);
        let census = stroke_census(&invisible, EDGE, GROUND);
        assert_eq!(census.identifying(), 1, "the edge is all there is");
        assert_eq!(census.redundant(), 0);

        // A node card: its type colour reads 4.38 against the same page, so the
        // reader finds it before the edge is drawn.
        let found = one_bordered_box(Color::rgb(0x1F, 0x8A, 0x4C), GROUND, EDGE);
        let census = stroke_census(&found, EDGE, GROUND);
        assert_eq!(census.identifying(), 0, "the fill identifies it");
        assert_eq!(census.redundant(), 1);

        // And both are still box edges, which is the question the vocabulary
        // decision in `PAIRINGS` turns on and the one R1839 answered.
        assert_eq!(census.boundaries(), 1);
        assert_eq!(census.dividers(), 0);
    }

    /// ★★★★★ R2152 — **a translucent tint is compared as the reader sees it.**
    ///
    /// The `contrast` module ignores alpha and says so, and tells a caller
    /// holding a translucent layer to composite it first. A fill that clears
    /// the floor at full opacity and is painted at a tenth of it does not
    /// separate anything, and reading the nominal colour would file the mark
    /// `Redundant` — the direction that loses the finding.
    #[test]
    fn r2152_a_translucent_fill_is_composited_before_it_is_judged() {
        use crate::style::Color;
        const GROUND: Color = Color::rgb(0xF6, 0xF7, 0xF9);
        const EDGE: Color = Color::rgb(0xC9, 0xD0, 0xD8);
        let opaque = Color::rgb(0x1F, 0x8A, 0x4C);
        assert_eq!(
            stroke_census(&one_bordered_box(opaque, GROUND, EDGE), EDGE, GROUND).redundant(),
            1,
            "at full opacity this fill separates",
        );
        let faint = opaque.with_alpha(0x1A);
        assert_eq!(
            stroke_census(&one_bordered_box(faint, GROUND, EDGE), EDGE, GROUND).identifying(),
            1,
            "and at a tenth of it, it does not",
        );
        // A fill nothing was painted with is not a distinction either.
        let none = opaque.with_alpha(0);
        assert_eq!(
            stroke_census(&one_bordered_box(none, GROUND, EDGE), EDGE, GROUND).identifying(),
            1,
        );
    }

    /// ★★★★★ R2152 — **the backdrop is the nearest filled ancestor, not the
    /// window's ground**, so a card inside a card is judged against the card.
    ///
    /// A box that clears the floor against the page and not against the panel
    /// it is actually sitting in is the case a scene-wide ground would get
    /// wrong, and it is the common case: this vocabulary has four container
    /// tiers precisely so panels nest.
    #[test]
    fn r2152_a_box_is_judged_against_what_is_actually_behind_it() {
        use crate::style::Color;
        const PAGE: Color = Color::rgb(0x00, 0x00, 0x00);
        const PANEL: Color = Color::rgb(0x1F, 0x8A, 0x4C);
        const EDGE: Color = Color::rgb(0xC9, 0xD0, 0xD8);
        // The inner fill clears 3.0 against the black page (4.38) and reads
        // 1.00 against the panel it is nested in.
        let scene = one_bordered_box(PANEL, PANEL, EDGE);
        let census = stroke_census(&scene, EDGE, PAGE);
        assert_eq!(
            census.identifying(),
            1,
            "judged against the panel, not against the window's ground",
        );
    }

    /// The floor is carried by exactly one arm, and `ALL` covers the enum — so
    /// a fourth position cannot be added without deciding what it owes.
    #[test]
    fn r2152_only_an_identifying_edge_owes_a_floor() {
        let owing: Vec<StrokeKind> = StrokeKind::ALL
            .into_iter()
            .filter(|kind| kind.floor().is_some())
            .collect();
        assert_eq!(owing, vec![StrokeKind::Identifying]);
        assert_eq!(
            StrokeKind::Identifying.floor(),
            Some(Floor::Boundary),
            "and it is the non-text floor it owes",
        );
        let edges: Vec<StrokeKind> = StrokeKind::ALL
            .into_iter()
            .filter(|kind| kind.is_box_edge())
            .collect();
        assert_eq!(edges, vec![StrokeKind::Identifying, StrokeKind::Redundant]);
        let names: BTreeSet<&str> = StrokeKind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(names.len(), StrokeKind::ALL.len(), "distinct names");
    }

    #[test]
    fn r1807_a_disagreeing_palette_is_reported_with_both_ratios() {
        // Break one pairing in the dark palette only, and the report must move
        // it out of `clear` and into `disagree` naming both sides.
        let light = Theme::light();
        let mut dark = Theme::dark();
        dark.on_surface = dark.surface;
        let report = parity(&light, &dark);
        assert!(!report.holds());
        let reading = report
            .disagree()
            .get("on_surface/surface")
            .expect("the broken pairing is named");
        assert!(reading.light > 4.5 && reading.dark < 4.5);
        assert!(reading.say().contains("light") && reading.say().contains("dark"));
    }

    /// ★★★★★ R2019 — **one palette can be asked what it does not clear**,
    /// which is the question a screen binding its own palette has and `parity`
    /// cannot answer.
    #[test]
    fn r2019_a_single_palette_names_the_pairings_it_does_not_clear() {
        for (word, palette) in [("light", Theme::light()), ("dark", Theme::dark())] {
            assert!(
                shortfalls(&palette).is_empty(),
                "the {word} canonical palette clears the whole table: {:?}",
                shortfalls(&palette)
            );
        }

        // The detector's own failing path: two pairings broken, one per floor,
        // so neither the text floor nor the boundary floor can be the only one
        // it reads. Without this an empty `Vec` satisfies everything above.
        let mut broken = Theme::light();
        broken.on_surface = broken.surface;
        broken.outline = broken.surface;
        let found = shortfalls(&broken);
        let named: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
        assert!(
            named.contains(&"on_surface/surface"),
            "the text pairing is named: {named:?}"
        );
        assert!(
            named.contains(&"outline/surface"),
            "and so is the boundary one: {named:?}"
        );
        // The two pairings whose ink was set to their own ground read 1.00,
        // and every entry reported is under the loosest floor in the table —
        // an inverted filter would report the clearing ones instead.
        for (name, ratio) in &found {
            if name == "on_surface/surface" || name == "outline/surface" {
                assert!(
                    (*ratio - 1.0).abs() < 0.01,
                    "{name} is its own ground, so it reads 1.00, not {ratio:.2}"
                );
            }
            assert!(
                *ratio < Floor::Text.ratio(),
                "{name} is reported as short, so it cannot read {ratio:.2}"
            );
        }
        // The order is the table's, which a map-backed implementation would
        // lose: `on_surface/surface` is the table's first entry and
        // `outline/surface` its last, so they must come out that way round.
        assert_eq!(
            named.first().copied(),
            Some("on_surface/surface"),
            "reported in PAIRINGS order: {named:?}"
        );
        assert_eq!(
            named.last().copied(),
            Some("outline/surface"),
            "reported in PAIRINGS order: {named:?}"
        );
    }
}
