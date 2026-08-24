//! Which role is **ink on which ground**, the standard each pairing is held
//! to, and whether the two palettes agree about it.
//!
//! # The fact that lived only in a name
//!
//! [`ColorRole::OnSurface`] is ink on [`ColorRole::Surface`]. Every reader of
//! this crate knows that, and until this module nothing in the tree could act
//! on it: the pairing was carried by the word `On` in an identifier and by
//! prose in a doc comment, so no gate could ask *is this palette legible?* and
//! none did. [`contrast_ratio`](crate::contrast::contrast_ratio) has existed
//! since R1546 — built for a chart's ink on a colour ramp — and had no table of
//! palette pairs to run over, so the instrument was here and the question was
//! never put to it.
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
//! honest while an absolute shortfall is carried openly ([`Floor::Boundary`]
//! and `outline` are exactly that case today, measured `1.82` light and `1.81`
//! dark: the two agree precisely, and this crate has one `outline` role where a
//! full design system separates a component boundary from a decorative
//! divider).
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
    text(ColorRole::InverseOnSurface, ColorRole::InverseSurface),
    // A snackbar's action label: the accent re-toned for the inverted ground.
    text(ColorRole::InversePrimary, ColorRole::InverseSurface),
    // Tones used as inline text on the plain surface.
    text(ColorRole::Accent, ColorRole::Surface),
    text(ColorRole::Error, ColorRole::Surface),
    text(ColorRole::Warning, ColorRole::Surface),
    // The one boundary pairing. See the module docs: this crate has a single
    // `outline` where a full design system separates the component boundary
    // from the decorative divider, and it is measurably short of the boundary
    // floor in BOTH palettes — an absolute shortfall the two agree about, not a
    // parity defect.
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// ★ The absolute shortfall this crate carries **openly**, so that closing
    /// the parity gate above cannot be mistaken for "every pairing is legible".
    ///
    /// `outline` is one role where a full design system separates a component
    /// boundary from a decorative divider. It is short of the boundary floor in
    /// both palettes, by almost exactly the same amount — the two agree, which
    /// is why it is not a parity defect, and it is short, which is why it is
    /// named here rather than left out of the table.
    #[test]
    fn r1807_the_one_absolute_shortfall_is_named_rather_than_hidden() {
        let report = canonical_parity();
        assert_eq!(
            report
                .short_in_both()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["outline/surface"]),
            "the shortfalls are exactly the ones this crate has written down"
        );
        let reading = &report.short_in_both()["outline/surface"];
        assert!(
            reading.light < Floor::Boundary.ratio() && reading.dark < Floor::Boundary.ratio(),
            "and it really is short in both: {}",
            reading.say()
        );
        assert!(reading.agrees(), "the two palettes agree that it is short");
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
}
