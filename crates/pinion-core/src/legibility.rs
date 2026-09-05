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
    // The one boundary pairing, and one is the right number: measured at
    // R1839 over six painted screens, `outline` draws 97 component boundaries
    // and 2 dividers, so a single `Floor::Boundary` is right for 98% of what
    // it paints. See the module docs for the count that replaced the
    // assumption, and `stroke_census` for how it is taken.
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
/// WCAG 1.4.11 holds a *component boundary* to 3:1 and asks nothing of a
/// *decorative divider*. So "does `outline` clear its floor" is not one
/// question until it is known which of the two a given mark is — and the same
/// colour does both jobs wherever a design system has only one outline role.
///
/// # Why this is derived from the frame and not from the source
///
/// The obvious census is `grep ColorRole::Outline`, and it answers the wrong
/// question: measured at R1839 it finds 145 MENTIONS, of which the large
/// majority are `theme.resolve(...)` binding a local that is then used several
/// times, several times differently. A mention is not a use. What the standard
/// is about is the mark on the frame, so the mark on the frame is what this
/// counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrokeKind {
    /// The colour is a [`Border`](crate::style::Border) on a box — it strokes
    /// the edge of something, which is what WCAG 1.4.11 calls a boundary.
    Boundary,
    /// The colour fills a box or strokes a path: a rule, a hairline, a grid
    /// line, a tick. Not the edge of a component.
    Divider,
}

impl StrokeKind {
    /// The floor this kind of mark is held to, or `None` where the standard
    /// asks nothing.
    #[must_use]
    pub const fn floor(self) -> Option<Floor> {
        match self {
            Self::Boundary => Some(Floor::Boundary),
            Self::Divider => None,
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
    /// Tagged boundary marks, by tag.
    pub boundary: BTreeSet<String>,
    /// Tagged divider marks, by tag.
    pub divider: BTreeSet<String>,
    /// Boundary marks carrying no tag of their own.
    pub boundary_untagged: usize,
    /// Divider marks carrying no tag of their own.
    pub divider_untagged: usize,
}

impl StrokeCensus {
    /// How many boundary marks there are in all.
    #[must_use]
    pub fn boundaries(&self) -> usize {
        self.boundary.len() + self.boundary_untagged
    }

    /// How many divider marks there are in all.
    #[must_use]
    pub fn dividers(&self) -> usize {
        self.divider.len() + self.divider_untagged
    }

    /// Fold another scene's census into this one, so a sweep over several
    /// screens answers about the application rather than about a frame.
    pub fn absorb(&mut self, other: &Self) {
        self.boundary.extend(other.boundary.iter().cloned());
        self.divider.extend(other.divider.iter().cloned());
        self.boundary_untagged += other.boundary_untagged;
        self.divider_untagged += other.divider_untagged;
    }
}

/// Census one painted scene for marks drawn in `colour`.
///
/// ★ The rule is one line and it is the whole judgment: a colour in the
/// `border` slot strokes the edge of a box, and a colour anywhere else does
/// not. It is derivable, which is the point — a hand classification of 145
/// sites would be one person's unreviewed reading, and this repository has a
/// standing debt about exactly that.
///
/// ⚠ Its stated limit: a box FILLED with the colour and one pixel tall is a
/// rule, and a box filled with it and forty pixels tall is a block — both count
/// as `Divider` here. That is deliberate rather than missed. Neither is the
/// edge of a component, so neither is what 1.4.11 is about, and inventing a
/// height threshold would put a number in the classifier that no standard
/// supports.
#[must_use]
pub fn stroke_census(scene: &crate::Scene, colour: crate::style::Color) -> StrokeCensus {
    use crate::Scene;
    let mut out = StrokeCensus::default();
    scene.for_each_node(&mut |visit| {
        let tag = visit.node.tag();
        let mut note = |kind: StrokeKind| match (kind, tag) {
            (StrokeKind::Boundary, Some(t)) => {
                out.boundary.insert(t.to_owned());
            }
            (StrokeKind::Boundary, None) => out.boundary_untagged += 1,
            (StrokeKind::Divider, Some(t)) => {
                out.divider.insert(t.to_owned());
            }
            (StrokeKind::Divider, None) => out.divider_untagged += 1,
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
                note(StrokeKind::Boundary);
            }
            if style.fill == colour {
                note(StrokeKind::Divider);
            }
        }
    });
    out
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
