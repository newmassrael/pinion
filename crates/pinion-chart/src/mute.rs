//! **Muting** — the half of a cross-filter that reaches the marks.
//!
//! # The defect this exists to remove
//!
//! [`LinkGroup`](crate::LinkGroup) (R1806) made the *declaration* of a
//! cross-filter a value: a board names the views it reaches, publishes a
//! [`Selection`], and gets back a [`Reach`] that accounts for every declared
//! view. What it deliberately left in each view was the other half — what a
//! view *does* with the selection it was handed.
//!
//! Measured at R1824 by building every chart kind this crate ships and reading
//! the fill alphas of the marks it emitted: **three of ten** kinds dimmed
//! anything. `BarChart`, `LineChart` and `ScatterChart` each carried their own
//! hand-written selection field, their own predicate and their own
//! `with_alpha(MUTED_ALPHA)` call; the other seven — `DonutChart`, `Treemap`,
//! `Sparkline`, `Timeline`, `BoxPlotChart`, `CandlestickChart`, `PolarChart` —
//! had no way to be told about a selection at all. A board could therefore
//! *declare* that a ring chart participates, publish a selection, receive a
//! `Reach` naming it as reached, and paint it entirely unchanged. The
//! declaration was checkable; the drawing was not.
//!
//! Two more consequences of the same gap, both measured the same way: the
//! [`Domain::Sector`] and [`Domain::LaneWindow`] arms of the selection
//! vocabulary had **no reader** — `Selection` could be *constructed* in either
//! domain but offered no accessor to get the sector or the lane back out, so
//! nothing could have consumed one.
//!
//! # What replaces it
//!
//! One trait, [`Mute`], implemented by every kind that carries marks. A kind
//! supplies two things and nothing else:
//!
//! * [`mark_keys`](Mute::mark_keys) — its marks, in the order its tags number
//!   them, each saying what it can be *tested* by: a label, a numeric span, an
//!   angular sector, a lane.
//! * a place to keep the resulting mask ([`MuteState`]).
//!
//! Everything else — which domains the kind accepts, whether a published
//! selection is refused, which marks survive it, how many were dimmed — is one
//! algorithm in this module's provided methods. There is no per-kind copy of it
//! to diverge.
//!
//! # The domains a kind accepts are DERIVED, never declared
//!
//! [`Mute::mute_domains`] is the intersection, over the kind's marks, of the
//! domains each mark can answer. A kind cannot claim to speak `Sector` while
//! emitting marks with no angular extent, because it does not get to claim
//! anything: the marks are the claim. This is the same rule
//! [`LinkGroup`](crate::LinkGroup) applies one level up — a declaration that
//! cannot be contradicted by what is drawn is a declaration nobody can check —
//! and it is why [`Mute::link`] can build a view's [`Link`] *from the chart*
//! rather than from a hand-written domain list beside it.
//!
//! A kind holding no marks answers nothing, and [`Mute::link`] declares it
//! [`inert`](Link::inert) with that reason rather than producing a view that
//! silently never narrows.
//!
//! # What "muted" means in the scene
//!
//! A dimmed mark keeps its geometry, its tag and its hue, and loses alpha: its
//! colour is multiplied by the crate's one muted-alpha constant. Multiplied,
//! not
//! replaced — a box plot's box is already drawn at the style's area alpha, and
//! *setting* it to the muted alpha would make a filtered-out mark more solid
//! than an unfiltered one. A composite node drawn over several marks (a
//! polyline over its samples, a radar's area fill) is dimmed when **every** mark
//! it composes is dimmed, so a series with one sample still in the selection
//! keeps its line.
//!
//! # Object safety
//!
//! The trait is **dyn-compatible** for everything that reads or applies a
//! selection ([`mark_keys`](Mute::mark_keys),
//! [`mute_domains`](Mute::mute_domains), [`mute`](Mute::mute),
//! [`muted`](Mute::muted)); only the three by-value / generic conveniences are
//! `Sized`-bound. That is deliberate and load-bearing: it is what lets a test
//! hold `Vec<Box<dyn Mute>>` and assert one property of every kind at once
//! rather than repeating the assertion per kind — which is how
//! `tests/cross_filter_mute.rs` states that the whole selection vocabulary has
//! a consumer, reading the domains off the list instead of off a written-down
//! set.
//!
//! ⚠ What that does NOT give is completeness: the list is written by hand, and
//! **nothing here forces an eleventh kind into it**. Adding one and forgetting
//! the list is a real and unguarded omission — recorded rather than implied,
//! because the paragraph above is exactly the kind of sentence a reader would
//! take for a guarantee.
//!
//! # Relationship to the older per-kind selections
//!
//! `BarChart::select` / `select_x_range`, `LineChart::select_x_range` and
//! `ScatterChart::select_x_range` are unchanged and still supported: they are
//! *finer* than this trait on their own axis (a line chart's x-window mutes the
//! polyline and overdraws the in-window portion; this trait's unit for a line
//! chart is the series). A mark is dimmed when **either** says so, which is the
//! same rule `BarChart` already applied to its own two selections.

use std::collections::BTreeSet;

use pinion_core::style::Color;

use crate::draw::{MUTED_ALPHA, mul_alpha};
use crate::link::{Domain, Link, Reach, Refusal, Selection};

/// Dim `color` to the cross-filter's muted strength, keeping its hue and
/// **multiplying** its alpha rather than replacing it.
///
/// See the module docs: a mark already drawn translucent (a box plot's box, a
/// radar's area fill) would come out *more* solid than its unfiltered
/// neighbours if the muted alpha were assigned.
#[must_use]
pub(crate) fn dim(color: Color) -> Color {
    color.with_alpha(mul_alpha(color.a, MUTED_ALPHA))
}

/// What one mark of a chart can be tested by.
///
/// A kind describes each of its marks with one of these, in the order its tags
/// number them. Every field is optional, and which ones are present is exactly
/// what decides the [`Domain`]s the kind accepts — see
/// [`Mute::mute_domains`].
///
/// The borrows are into the chart's own data, so building the list allocates
/// only the vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkKey<'a> {
    label: Option<&'a str>,
    x: Option<(f64, f64)>,
    sector: Option<((f64, f64), (f64, f64))>,
    lane: Option<&'a str>,
}

impl Default for MarkKey<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> MarkKey<'a> {
    /// A mark that can be tested by nothing yet — the start of a builder chain.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            label: None,
            x: None,
            sector: None,
            lane: None,
        }
    }

    /// This mark's name in a nominal vocabulary — what a
    /// [`Selection::Category`] is compared against.
    #[must_use]
    pub const fn labelled(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// The closed numeric interval this mark occupies on the axis a
    /// [`Selection::XRange`] speaks about.
    ///
    /// Pass `lo == hi` for a mark that sits at a point (a scatter sample, a
    /// candlestick session's instant); the coverage test treats a point and a
    /// span differently at the window's edges — see [`Self::covered_by`].
    #[must_use]
    pub const fn spanning(mut self, lo: f64, hi: f64) -> Self {
        self.x = Some((lo, hi));
        self
    }

    /// The point this mark sits at on the numeric axis — [`spanning`](Self::spanning)
    /// with both edges the same.
    #[must_use]
    pub const fn at_x(self, x: f64) -> Self {
        self.spanning(x, x)
    }

    /// The angular interval and radial band this mark occupies, in the chart's
    /// own angular units — what a [`Selection::Sector`] is compared against.
    ///
    /// A ring chart's slice reports the sweep it fills and the band between its
    /// hole and its rim; a polar sample reports its own angle and radius as
    /// degenerate intervals.
    #[must_use]
    pub const fn in_sector(mut self, angle: (f64, f64), radius: (f64, f64)) -> Self {
        self.sector = Some((angle, radius));
        self
    }

    /// The lane this mark belongs to. A [`Selection::LaneWindow`] tests the
    /// lane **and** the numeric span, so a mark that names a lane without also
    /// [`spanning`](Self::spanning) a window cannot answer that domain.
    #[must_use]
    pub const fn in_lane(mut self, lane: &'a str) -> Self {
        self.lane = Some(lane);
        self
    }

    /// This mark's name, when it has one.
    #[must_use]
    pub const fn label(&self) -> Option<&'a str> {
        self.label
    }

    /// The domains this mark can be tested in.
    ///
    /// Derived from which coordinates it carries, never declared. See the
    /// module docs.
    #[must_use]
    pub fn answers(&self) -> BTreeSet<Domain> {
        let mut out = BTreeSet::new();
        if self.label.is_some() {
            out.insert(Domain::Category);
        }
        if self.x.is_some() {
            out.insert(Domain::XRange);
        }
        if self.sector.is_some() {
            out.insert(Domain::Sector);
        }
        if self.lane.is_some() && self.x.is_some() {
            out.insert(Domain::LaneWindow);
        }
        out
    }

    /// Whether `selection` covers this mark — `None` when this mark carries
    /// nothing testable in that selection's domain, which is the case
    /// [`Mute::mute`] turns into a [`Refusal`] before any mark is consulted.
    #[must_use]
    pub fn covered_by(&self, selection: &Selection) -> Option<bool> {
        match selection {
            Selection::XRange { lo, hi } => self.x.map(|span| in_window(span, (*lo, *hi))),
            Selection::Category(name) => self.label.map(|l| l == name.as_str()),
            Selection::Sector { angle, radius } => self
                .sector
                .map(|(a, r)| overlaps(a, *angle) && overlaps(r, *radius)),
            Selection::LaneWindow { lane, window } => match (self.lane, self.x) {
                (Some(l), Some(span)) => Some(l == lane.as_str() && in_window(span, *window)),
                _ => None,
            },
        }
    }
}

/// Whether the closed interval `span` meets the half-open window `[lo, hi)`.
///
/// A degenerate span — a mark that sits at a point — is tested as a point, so a
/// sample exactly on the window's lower edge is inside and one on its upper
/// edge is not. A span with width is tested as an overlap, so a bin ending
/// exactly at `lo` does not count as reaching into the window.
fn in_window(span: (f64, f64), window: (f64, f64)) -> bool {
    let (a, b) = span;
    let (lo, hi) = window;
    // An exact comparison, deliberately: `a` and `b` are the SAME value copied
    // by `at_x`, not two results of arithmetic, so there is no accumulated
    // error for a margin to absorb — and a margin would silently reclassify a
    // genuinely narrow bin as a point.
    #[allow(clippy::float_cmp, reason = "identity, not the result of arithmetic")]
    let degenerate = a == b;
    if degenerate {
        a >= lo && a < hi
    } else {
        a < hi && b > lo
    }
}

/// Whether two closed intervals meet. Used for the sector domain, where both
/// axes are bands rather than windows and neither end is privileged.
fn overlaps(a: (f64, f64), b: (f64, f64)) -> bool {
    let (a0, a1) = (a.0.min(a.1), a.0.max(a.1));
    let (b0, b1) = (b.0.min(b.1), b.0.max(b.1));
    a0 <= b1 && b0 <= a1
}

/// What applying a [`Selection`] did to a kind's marks.
///
/// The value [`Mute::mute`] hands back: a summary a caller can assert on and
/// print, without having to walk the produced scene to find out whether
/// anything happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Muted {
    domain: Option<Domain>,
    marks: usize,
    dimmed: usize,
}

impl Muted {
    /// The domain of the selection that was applied — `None` when the
    /// selection was cleared and every mark draws full.
    #[must_use]
    pub const fn domain(self) -> Option<Domain> {
        self.domain
    }

    /// How many marks the kind carries.
    #[must_use]
    pub const fn marks(self) -> usize {
        self.marks
    }

    /// How many of them the selection did **not** cover, and which therefore
    /// draw dimmed.
    #[must_use]
    pub const fn dimmed(self) -> usize {
        self.dimmed
    }

    /// How many draw at full strength.
    #[must_use]
    pub const fn lit(self) -> usize {
        self.marks - self.dimmed
    }

    /// Whether nothing is muted — either because no selection is applied, or
    /// because the one applied covers every mark.
    #[must_use]
    pub const fn is_clear(self) -> bool {
        self.dimmed == 0
    }
}

/// A kind's stored cross-filter mask: which of its marks the active selection
/// covers.
///
/// Held as a field by every [`Mute`] implementor. Resolved once when a
/// selection is applied rather than per frame, so a builder's per-mark question
/// is an index into a `Vec<bool>` and the coverage predicate runs `marks` times
/// per *selection change* instead of `marks` times per *paint*.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MuteState {
    /// `None` = no selection applied; every mark draws full. `Some` = one
    /// entry per mark, `true` for covered.
    lit: Option<Vec<bool>>,
    outcome: Muted,
}

impl MuteState {
    /// Whether mark `i` draws dimmed.
    ///
    /// `false` when no selection is applied, and `false` for an index past the
    /// mask — a builder that skips a mark (a non-finite datum, a slot the axis
    /// could not place) must not have its numbering silently reinterpreted.
    #[must_use]
    pub fn dimmed_at(&self, i: usize) -> bool {
        self.lit
            .as_ref()
            .is_some_and(|l| l.get(i).is_some_and(|covered| !covered))
    }

    /// Whether **every** mark in `indices` draws dimmed — the test a composite
    /// node (a polyline over its samples, a radar's area fill) is dimmed by.
    ///
    /// `false` for an empty range, so a series with no placed samples does not
    /// report itself uniformly muted.
    #[must_use]
    pub fn all_dimmed(&self, indices: impl IntoIterator<Item = usize>) -> bool {
        let mut any = false;
        for i in indices {
            any = true;
            if !self.dimmed_at(i) {
                return false;
            }
        }
        any
    }

    /// What the applied selection did. See [`Muted`].
    #[must_use]
    pub const fn outcome(&self) -> Muted {
        self.outcome
    }

    /// `dim(color)` when mark `i` is muted, `color` unchanged otherwise — the
    /// one call a builder makes per mark.
    #[must_use]
    pub(crate) fn shade(&self, i: usize, color: Color) -> Color {
        if self.dimmed_at(i) { dim(color) } else { color }
    }
}

/// A chart kind whose marks a cross-filter [`Selection`] can dim.
///
/// See the module documentation for why this is one trait rather than a
/// selection field per kind, and for what a kind is and is not asked to
/// supply.
pub trait Mute {
    /// This kind's marks, in the order its tags number them, each saying what
    /// it can be tested by.
    ///
    /// The order is load-bearing: [`MuteState::dimmed_at`] is indexed with it,
    /// and a builder asking about mark `i` must mean the same mark this list's
    /// `i`-th entry describes.
    fn mark_keys(&self) -> Vec<MarkKey<'_>>;

    /// The stored mask. Implementors return their own field.
    fn mute_state(&self) -> &MuteState;

    /// The stored mask, mutably. Implementors return their own field.
    fn mute_state_mut(&mut self) -> &mut MuteState;

    /// The domains this kind accepts — the intersection, over its marks, of
    /// what each can be tested by.
    ///
    /// Empty when the kind holds no marks: a chart with nothing drawn answers
    /// no question about the population.
    #[must_use]
    fn mute_domains(&self) -> BTreeSet<Domain> {
        domains_of(&self.mark_keys())
    }

    /// Whether this kind's marks can be tested by `domain`.
    #[must_use]
    fn mute_accepts(&self, domain: Domain) -> bool {
        self.mute_domains().contains(&domain)
    }

    /// Apply `selection` — every mark it does not cover draws dimmed. `None`
    /// clears, restoring every mark to full strength.
    ///
    /// # Errors
    ///
    /// [`Refusal::Domain`] when the selection speaks a domain this kind's marks
    /// cannot be tested in, with both sides named. Nothing is changed when a
    /// selection is refused: a refused view keeps whatever it was showing,
    /// rather than silently clearing to "everything".
    fn mute(&mut self, selection: Option<&Selection>) -> Result<Muted, Refusal> {
        let Some(selection) = selection else {
            *self.mute_state_mut() = MuteState::default();
            return Ok(Muted::default());
        };
        let domain = selection.domain();
        // Resolved while `keys` still borrows `self`; everything carried out of
        // this block is owned, so the mask can be stored below.
        let (accepts, lit) = {
            let keys = self.mark_keys();
            let accepts = domains_of(&keys);
            if accepts.contains(&domain) {
                let lit: Vec<bool> = keys
                    .iter()
                    .map(|k| k.covered_by(selection).unwrap_or(false))
                    .collect();
                (accepts, Some(lit))
            } else {
                (accepts, None)
            }
        };
        let Some(lit) = lit else {
            return Err(Refusal::Domain {
                selected: domain,
                accepts: accepts.into_iter().collect(),
            });
        };
        let marks = lit.len();
        let dimmed = lit.iter().filter(|covered| !**covered).count();
        let outcome = Muted {
            domain: Some(domain),
            marks,
            dimmed,
        };
        *self.mute_state_mut() = MuteState {
            lit: Some(lit),
            outcome,
        };
        Ok(outcome)
    }

    /// What the applied selection did, without re-applying it.
    #[must_use]
    fn muted(&self) -> Muted {
        self.mute_state().outcome()
    }

    /// [`mute`](Self::mute) in builder position, for a view function that
    /// constructs its chart inline.
    ///
    /// # Errors
    ///
    /// [`Refusal::Domain`], exactly as [`mute`](Self::mute). There is
    /// deliberately no infallible builder twin: a selection a chart cannot
    /// answer is the failure this whole module exists to stop being silent, and
    /// a `self`-returning form that swallowed it would reintroduce it.
    fn try_muted_by(mut self, selection: &Selection) -> Result<Self, Refusal>
    where
        Self: Sized,
    {
        self.mute(Some(selection))?;
        Ok(self)
    }

    /// Apply whatever a published [`Reach`] says this view gets — the form a
    /// board uses.
    ///
    /// The group has already decided whether this view participates, so there
    /// is nothing left to refuse here: a view the reach did not name keeps
    /// every mark full, and the reach itself carries the reason
    /// ([`Reach::reason`]). That is why this one is infallible where
    /// [`try_muted_by`](Self::try_muted_by) is not.
    ///
    /// `reach` is an [`Option`] because "nothing is published" is a real and
    /// common board state — no chip lit, no brush dragged — and it is NOT the
    /// same as an empty selection: every mark draws full. Taking it here rather
    /// than making each caller write the `match` is what keeps a view's whole
    /// participation one call in a view function.
    #[must_use]
    fn muted_by_reach(mut self, reach: Option<&Reach>, view: &str) -> Self
    where
        Self: Sized,
    {
        // `mute` can still refuse when the board's declaration is wider than
        // the chart's marks (an empty chart under a declared group). The reach
        // is the authority on participation, so a refusal here means "nothing
        // to narrow", and clearing is the honest rendering of that.
        let selection = reach.and_then(|r| r.selection_for(view)).cloned();
        if self.mute(selection.as_ref()).is_err() {
            let _ = self.mute(None);
        }
        self
    }

    /// This chart's own declaration for a [`LinkGroup`](crate::LinkGroup),
    /// built from its marks rather than written beside it.
    ///
    /// A chart with no marks declares itself [`inert`](Link::inert) with that
    /// reason, which is what `LinkGroup::new` requires of a view that accepts
    /// nothing — and is a truer statement than an empty domain list.
    #[must_use]
    fn link(&self, name: impl Into<String>) -> Link
    where
        Self: Sized,
    {
        let domains = self.mute_domains();
        if domains.is_empty() {
            Link::inert(name, "holds no marks, so there is nothing to narrow")
        } else {
            Link::new(name, domains)
        }
    }
}

/// The intersection, over `keys`, of the domains each mark can answer — empty
/// for an empty slice.
fn domains_of(keys: &[MarkKey<'_>]) -> BTreeSet<Domain> {
    let mut it = keys.iter();
    let Some(first) = it.next() else {
        return BTreeSet::new();
    };
    let mut acc = first.answers();
    for k in it {
        let theirs = k.answers();
        acc.retain(|d| theirs.contains(d));
        if acc.is_empty() {
            break;
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal implementor, so this module's own algorithm is tested without
    /// a chart. The per-kind proofs live in `tests/cross_filter_mute.rs`.
    #[derive(Default)]
    struct Marks {
        labels: Vec<String>,
        state: MuteState,
    }

    impl Marks {
        fn of(labels: &[&str]) -> Self {
            Self {
                labels: labels.iter().map(|s| (*s).to_string()).collect(),
                state: MuteState::default(),
            }
        }
    }

    impl Mute for Marks {
        fn mark_keys(&self) -> Vec<MarkKey<'_>> {
            self.labels
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "a test fixture's slot index, bounded by the fixture"
                    )]
                    MarkKey::new().labelled(l).at_x(i as f64)
                })
                .collect()
        }
        fn mute_state(&self) -> &MuteState {
            &self.state
        }
        fn mute_state_mut(&mut self) -> &mut MuteState {
            &mut self.state
        }
    }

    #[test]
    fn domains_are_derived_from_the_marks() {
        let m = Marks::of(&["a", "b"]);
        assert_eq!(
            m.mute_domains(),
            BTreeSet::from([Domain::Category, Domain::XRange]),
            "two coordinates carried, two domains answered"
        );
        assert!(
            !m.mute_accepts(Domain::Sector),
            "and nothing else, because no mark carries an angle"
        );
    }

    #[test]
    fn an_empty_kind_answers_nothing_and_declares_itself_inert() {
        let m = Marks::default();
        assert!(m.mute_domains().is_empty());
        assert_eq!(
            m.link("v").inert_reason(),
            Some("holds no marks, so there is nothing to narrow"),
            "a view that cannot narrow says why, rather than declaring an empty set"
        );
    }

    #[test]
    fn a_domain_the_marks_cannot_answer_is_refused_and_changes_nothing() {
        let mut m = Marks::of(&["a", "b"]);
        m.mute(Some(&Selection::Category("a".into()))).expect("ok");
        assert_eq!(m.muted().dimmed(), 1);
        let err = m
            .mute(Some(&Selection::Sector {
                angle: (0.0, 1.0),
                radius: (0.0, 1.0),
            }))
            .expect_err("no mark carries an angle");
        assert!(matches!(err, Refusal::Domain { selected, .. } if selected == Domain::Sector));
        assert_eq!(
            m.muted().dimmed(),
            1,
            "a refused selection leaves the view showing what it was showing"
        );
    }

    #[test]
    fn clearing_restores_every_mark() {
        let mut m = Marks::of(&["a", "b"]);
        m.mute(Some(&Selection::Category("a".into()))).expect("ok");
        let cleared = m.mute(None).expect("clear never refuses");
        assert!(cleared.is_clear() && cleared.domain().is_none());
        assert!(!m.mute_state().dimmed_at(1));
    }

    #[test]
    fn a_point_mark_is_inside_the_windows_lower_edge_and_outside_its_upper() {
        let k = MarkKey::new().at_x(2.0);
        assert_eq!(
            k.covered_by(&Selection::XRange { lo: 2.0, hi: 5.0 }),
            Some(true)
        );
        assert_eq!(
            k.covered_by(&Selection::XRange { lo: 0.0, hi: 2.0 }),
            Some(false)
        );
    }

    #[test]
    fn a_span_mark_needs_real_overlap_not_a_touching_edge() {
        let k = MarkKey::new().spanning(0.0, 2.0);
        assert_eq!(
            k.covered_by(&Selection::XRange { lo: 2.0, hi: 5.0 }),
            Some(false),
            "a bin ending where the window starts does not reach into it"
        );
        assert_eq!(
            k.covered_by(&Selection::XRange { lo: 1.0, hi: 5.0 }),
            Some(true)
        );
    }

    #[test]
    fn a_lane_window_tests_both_dimensions() {
        let k = MarkKey::new().in_lane("l0").spanning(0.0, 10.0);
        let same_lane = Selection::LaneWindow {
            lane: "l0".into(),
            window: (5.0, 15.0),
        };
        let other_lane = Selection::LaneWindow {
            lane: "l1".into(),
            window: (5.0, 15.0),
        };
        assert_eq!(k.covered_by(&same_lane), Some(true));
        assert_eq!(
            k.covered_by(&other_lane),
            Some(false),
            "the same window in another lane is not the same selection"
        );
    }

    #[test]
    fn a_mark_that_names_a_lane_without_a_span_cannot_answer_lane_window() {
        let k = MarkKey::new().in_lane("l0");
        assert!(
            !k.answers().contains(&Domain::LaneWindow),
            "the domain is two dimensions, and this mark carries one"
        );
    }

    #[test]
    fn all_dimmed_is_false_for_an_empty_range() {
        let mut m = Marks::of(&["a"]);
        m.mute(Some(&Selection::Category("z".into()))).expect("ok");
        assert!(m.mute_state().all_dimmed(0..1));
        assert!(
            !m.mute_state().all_dimmed(0..0),
            "a composite over no marks is not uniformly muted"
        );
    }

    #[test]
    fn dimming_multiplies_alpha_rather_than_assigning_it() {
        let translucent = Color::rgba(0x20, 0x40, 0x60, 0x40);
        let dimmed = dim(translucent);
        assert!(
            dimmed.a < translucent.a,
            "a mark already faint gets fainter, never more solid: {} -> {}",
            translucent.a,
            dimmed.a
        );
        assert_eq!(
            (dimmed.r, dimmed.g, dimmed.b),
            (translucent.r, translucent.g, translucent.b),
            "the hue is what identifies the mark; only its strength changes"
        );
    }
}
