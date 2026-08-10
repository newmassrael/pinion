//! R1629 §5.11 §2 #7 — **what a drawing did that the drawing cannot give
//! back**, as data.
//!
//! # The gap this closes
//!
//! A picture is produced from two sources: the data it was given and the
//! request that said how to draw it. Neither survives into the pixels. A
//! smooth line chart draws a dip below a plateau that no sample took; a log
//! axis silently drops every non-positive sample; a kernel density estimate
//! chooses a bandwidth that decides the whole shape of the outline; a builder
//! accepts `with_caps(true)` and then draws a mark that has no caps. In every
//! one of those the reader — human or agent — is looking at a drawing that
//! disagrees with its sources, and **the drawing does not say so**.
//!
//! Those facts do exist while the chart is being built. Before this module
//! they existed *only* there: `LineChart::overshoot()`, `Density::spill()`,
//! `BoxPlotChart::without_density()` are in-process Rust calls, so an
//! RPC client — the §2 #2 primary path — held a `Scene` and could not ask any
//! of them. §2 #7 says the scene is what a client reads, so the statement has
//! to be *in* the scene.
//!
//! # The four kinds are a closed 2×2, not a list
//!
//! A derivation is a disagreement between the picture and one of its two
//! sources, and a disagreement has a direction. That is two axes with two
//! values each, and the product is complete by construction:
//!
//! |  | the picture has what the source does not | the source has what the picture does not |
//! |---|---|---|
//! | **the data** | [`Invented`](DerivationKind::Invented) | [`Omitted`](DerivationKind::Omitted) |
//! | **the request** | [`Chosen`](DerivationKind::Chosen) | [`Discarded`](DerivationKind::Discarded) |
//!
//! This is why [`DerivationKind`] is a plain enum rather than a
//! `#[non_exhaustive]` one: a fifth kind would have to name a third source or
//! a third direction, and there is not one. A client can therefore write an
//! exhaustive match and know it stays exhaustive — the property R1623 had to
//! buy with a second closed type is free here.
//!
//! Each arm is also a different **action**, which is the test of whether a
//! taxonomy earns its arms: an `Invented` value asks the reader to be warned,
//! an `Omitted` one asks for a different scale, a `Chosen` one asks for the
//! control that would change it, and a `Discarded` one says the caller's code
//! is wrong. Collapsing them into "notes" would make all four unactionable.
//!
//! # Past the reference
//!
//! The reference toolkit's chart module is a **property bag with runtime
//! reflection**: its object system will read a series' `capsVisible` back to
//! you by name, so "what did I ask for" is answerable there. What is not
//! answerable — not by the reflection, not by any signal, not by the series
//! API — is **what the drawing did with it**. Its spline series has one
//! algorithm, no choice of it, and no report; its candlestick series accepts
//! the caps flag on a series drawn without caps and says nothing; its
//! logarithmic axis drops non-positive points with no accounting. Echoing the
//! request back is not the same fact as stating the picture's relationship to
//! it, and this module publishes the second one.
//!
//! # Spans share the marks vocabulary
//!
//! When a derivation is localized — an overshoot covers samples 4 through 6 —
//! it carries a [`span`](Derivation::span) counted in the set's
//! [`domain`](DerivationSet::domain), which is the same index-space vocabulary
//! [`MarkSet`](crate::marks::MarkSet) states. One vocabulary of index spaces,
//! not two that could disagree about what `sample` means.

use std::borrow::Cow;
use std::fmt;

/// R1629 §2 #7 — which of the four disagreements between a picture and its
/// sources this statement is.
///
/// See the [module docs](self) for the 2×2 this closes over. Not
/// `#[non_exhaustive]`: the product of {data, request} × {picture has more,
/// picture has less} has exactly four cells, so a client's match on this is
/// exhaustive and stays exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DerivationKind {
    /// **The picture shows a value the data does not contain.** A spline
    /// through a plateau and then a rise dips below the plateau; a Gaussian
    /// kernel puts density below zero for a quantity that cannot be negative.
    ///
    /// A reader acts on this by distrusting the excursion — or by asking for
    /// an interpolation that cannot make one.
    Invented,
    /// **The data contains a value the picture does not show.** A sample at or
    /// below zero has no pixel on a log axis; a candle whose whole range sits
    /// outside an explicit domain draws nothing.
    ///
    /// A reader acts on this by changing the scale, because the alternative —
    /// a mark pinned to the axis floor — is indistinguishable from a real
    /// measurement there.
    Omitted,
    /// **The picture rests on a decision the request left open.** A bandwidth,
    /// a kernel, a fence multiplier, a join between samples: each one changes
    /// the drawing and each one has a default that nothing in the picture
    /// reveals.
    ///
    /// A reader acts on this by reaching for the control — which it can only
    /// do if it knows the control exists and what it currently is.
    Chosen,
    /// **The picture ignores a decision the request made.** A setting that has
    /// no meaning for the mark that was actually drawn, or a derivation the
    /// data could not support.
    ///
    /// A reader acts on this by fixing its own code. This is the arm that
    /// makes a silently-dropped builder call into a queryable fact instead of
    /// a bug that reproduces as "the option does nothing".
    Discarded,
}

impl DerivationKind {
    /// Every kind, for a consumer that must cover the vocabulary — and for the
    /// wire-schema declaration that publishes the accepted filter values.
    pub const ALL: [Self; 4] = [Self::Invented, Self::Omitted, Self::Chosen, Self::Discarded];

    /// The wire spelling, and the word a client matches on.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Invented => "invented",
            Self::Omitted => "omitted",
            Self::Chosen => "chosen",
            Self::Discarded => "discarded",
        }
    }

    /// The kind named by its [`wire_name`](Self::wire_name), or `None`.
    ///
    /// The inverse of the spelling above, so a wire filter is parsed by the
    /// same table that writes it and the two cannot drift.
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.wire_name() == name)
    }

    /// Which source the picture is being compared against.
    ///
    /// One of the two axes of the 2×2, exposed because a client that wants
    /// "everything the data does not support" asks for a *row*, not for two
    /// kinds it had to know were related.
    #[must_use]
    pub const fn source(self) -> DerivationSource {
        match self {
            Self::Invented | Self::Omitted => DerivationSource::Data,
            Self::Chosen | Self::Discarded => DerivationSource::Request,
        }
    }

    /// Whether the picture has something the source does not (`true`), or the
    /// source has something the picture does not (`false`).
    ///
    /// The other axis. Together with [`source`](Self::source) it reconstructs
    /// the kind, which is what
    /// [`from_axes`](Self::from_axes) is for and what the round-trip test
    /// holds the table to.
    #[must_use]
    pub const fn picture_has_more(self) -> bool {
        matches!(self, Self::Invented | Self::Chosen)
    }

    /// The kind at a cell of the 2×2.
    ///
    /// Its existence is the claim that the table is total: every combination
    /// of the two axes names a kind, and no kind is unreachable.
    #[must_use]
    pub const fn from_axes(source: DerivationSource, picture_has_more: bool) -> Self {
        match (source, picture_has_more) {
            (DerivationSource::Data, true) => Self::Invented,
            (DerivationSource::Data, false) => Self::Omitted,
            (DerivationSource::Request, true) => Self::Chosen,
            (DerivationSource::Request, false) => Self::Discarded,
        }
    }
}

impl fmt::Display for DerivationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name())
    }
}

/// Which of a picture's two sources a [`DerivationKind`] compares it against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DerivationSource {
    /// The values the drawing was given.
    Data,
    /// The settings the caller asked for.
    Request,
}

impl DerivationSource {
    /// Both sources — the axis, for a consumer covering the vocabulary.
    pub const ALL: [Self; 2] = [Self::Data, Self::Request];

    /// The wire spelling.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Request => "request",
        }
    }
}

impl fmt::Display for DerivationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name())
    }
}

/// R1629 — the measurement behind one derivation, typed rather than
/// stringified.
///
/// A client that has to parse `"0.081"` out of a sentence cannot compare it to
/// a threshold, and one that gets a bare number cannot tell a count from a
/// fraction. Four arms, because these are the four shapes the framework's own
/// reports actually take; a fifth would be a new *shape* of answer, not a new
/// subject, which is why this one **is** `#[non_exhaustive]` where
/// [`DerivationKind`] is not.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Evidence {
    /// A name a client matches, not a number: the kernel that was used, the
    /// mark that made a setting meaningless, the source a distribution came
    /// from.
    Name(Cow<'static, str>),
    /// A real quantity, in the derivation's [`unit`](Derivation::unit).
    Real(f64),
    /// A whole count of things — samples, points, categories.
    Count(usize),
    /// Yes or no: whether the estimate was bounded, whether a control was on.
    Flag(bool),
}

impl Evidence {
    /// The wire discriminator for this shape of answer.
    #[must_use]
    pub const fn wire_name(&self) -> &'static str {
        match self {
            Self::Name(_) => "name",
            Self::Real(_) => "real",
            Self::Count(_) => "count",
            Self::Flag(_) => "flag",
        }
    }

    /// The quantity when this is a number, or `None`.
    ///
    /// [`Count`](Self::Count) answers here too: a count *is* a quantity, and a
    /// client comparing "how much" against a threshold should not have to
    /// branch on which of the two spellings a particular report chose.
    #[must_use]
    pub fn quantity(&self) -> Option<f64> {
        match self {
            Self::Real(v) => Some(*v),
            #[expect(
                clippy::cast_precision_loss,
                reason = "a count of marks in one drawing; the exact range is far \
                          below 2^53 and a lossy answer is still the right answer \
                          for the comparison this exists to serve"
            )]
            Self::Count(v) => Some(*v as f64),
            Self::Name(_) | Self::Flag(_) => None,
        }
    }
}

impl fmt::Display for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(n) => f.write_str(n),
            Self::Real(v) => write!(f, "{v}"),
            Self::Count(v) => write!(f, "{v}"),
            Self::Flag(v) => write!(f, "{v}"),
        }
    }
}

/// R1629 §2 #7 — one statement a drawing makes about how it was produced.
///
/// Built with [`new`](Self::new) and narrowed with the three `with_` methods,
/// none of which is required: a derivation with no subject is about the whole
/// drawing, one with no unit measures something dimensionless, and one with no
/// span is not localized.
#[derive(Debug, Clone, PartialEq)]
pub struct Derivation {
    kind: DerivationKind,
    name: Cow<'static, str>,
    subject: Option<Cow<'static, str>>,
    evidence: Evidence,
    unit: Option<Cow<'static, str>>,
    span: Option<(usize, usize)>,
}

impl Derivation {
    /// A derivation of `kind` about `name`, evidenced by `evidence`.
    ///
    /// `name` is what the statement is *about* — `"bandwidth"`, `"overshoot"`,
    /// `"caps"` — and it is the handle a client filters on, so it is a stable
    /// identifier rather than a sentence.
    #[must_use]
    pub fn new(
        kind: DerivationKind,
        name: impl Into<Cow<'static, str>>,
        evidence: Evidence,
    ) -> Self {
        Self {
            kind,
            name: name.into(),
            subject: None,
            evidence,
            unit: None,
            span: None,
        }
    }

    /// Which part of the drawing this is about — a series, a category, a
    /// session. Absent means the whole drawing.
    ///
    /// The spelling is the consumer's: a chart names its series the way its
    /// own tags do, so an answer here addresses the same thing
    /// `scene/snapshot` shows.
    #[must_use]
    pub fn about(mut self, subject: impl Into<Cow<'static, str>>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// The units of a [`Evidence::Real`], so `0.081` is readable.
    ///
    /// Absent when the quantity is dimensionless or the evidence is not a
    /// number at all.
    #[must_use]
    pub fn in_units(mut self, unit: impl Into<Cow<'static, str>>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Where in the owning set's [`domain`](DerivationSet::domain) this
    /// applies, as `[start, end)`.
    ///
    /// An inverted range is stored in order, so a caller cannot publish a span
    /// that reports a negative width.
    #[must_use]
    pub fn spanning(mut self, start: usize, end: usize) -> Self {
        self.span = Some(if start <= end {
            (start, end)
        } else {
            (end, start)
        });
        self
    }

    /// Which of the four disagreements this is.
    #[must_use]
    pub const fn kind(&self) -> DerivationKind {
        self.kind
    }

    /// What the statement is about.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Which part of the drawing, or `None` for the whole of it.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// The measurement.
    #[must_use]
    pub const fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    /// The units of the measurement, or `None`.
    #[must_use]
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    /// Where it applies, as `[start, end)` in the set's domain, or `None` when
    /// it is not localized.
    #[must_use]
    pub const fn span(&self) -> Option<(usize, usize)> {
        self.span
    }
}

/// R1629 — the derivations one node published, with the index space their
/// spans count in.
///
/// Order is declaration order, like [`MarkSet`](crate::marks::MarkSet)'s: a
/// consumer reading the list front to back sees the statements in the order
/// the builder made them, which is the order a caption would read them out.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivationSet {
    domain: Cow<'static, str>,
    entries: Vec<Derivation>,
}

impl DerivationSet {
    /// An empty set whose spans count in `domain`.
    ///
    /// The domain is stated even by a set whose entries are all unlocalized,
    /// because "no entry has a span *yet*" is not a promise about the next
    /// one, and a client should never have to guess what an index counts.
    #[must_use]
    pub fn over(domain: impl Into<Cow<'static, str>>) -> Self {
        Self {
            domain: domain.into(),
            entries: Vec::new(),
        }
    }

    /// Add one derivation, at the end.
    #[must_use]
    pub fn stating(mut self, derivation: Derivation) -> Self {
        self.entries.push(derivation);
        self
    }

    /// Add every derivation of `iter`, in its order.
    #[must_use]
    pub fn stating_all(mut self, iter: impl IntoIterator<Item = Derivation>) -> Self {
        self.entries.extend(iter);
        self
    }

    /// What a [`span`](Derivation::span) indexes — the same vocabulary
    /// [`crate::marks::domain`] names.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// Every derivation, in declaration order.
    #[must_use]
    pub fn entries(&self) -> &[Derivation] {
        &self.entries
    }

    /// How many.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the node stated nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every derivation of `kind`, in declaration order.
    pub fn of_kind(&self, kind: DerivationKind) -> impl Iterator<Item = &Derivation> {
        self.entries.iter().filter(move |d| d.kind == kind)
    }

    /// Every derivation comparing the picture against `source` — a whole row
    /// of the 2×2.
    pub fn against(&self, source: DerivationSource) -> impl Iterator<Item = &Derivation> {
        self.entries
            .iter()
            .filter(move |d| d.kind.source() == source)
    }

    /// Every derivation named `name`, in declaration order.
    ///
    /// A name repeats across subjects — every series has its own `overshoot` —
    /// so this answers with all of them rather than with the first.
    pub fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Derivation> {
        self.entries.iter().filter(move |d| d.name == name)
    }
}

/// R1629 §2 #7 — whether a [`Scene`](crate::scene::Scene) node kind can state
/// how its drawing was produced.
///
/// Exhaustive on [`SceneNodeKind`](crate::scene::SceneNodeKind), for the
/// reason [`MarksChannel`](crate::marks::MarksChannel) is: a kind added later
/// has to decide, rather than inheriting "no derivations" from a `_ =>` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DerivesChannel {
    /// The kind assembles a drawing out of data, so there is a production step
    /// for it to describe. A chart's root is one of these.
    Composes,
    /// The kind paints one thing out of values it was handed. Whatever derived
    /// those values did so outside the node, and the composition that owns it
    /// is where the statement belongs — attributing a bandwidth to the single
    /// path that happens to carry the outline would put one fact in as many
    /// places as the outline has strokes.
    Painted,
    /// The kind shows a subtree through a viewport. It decides *where* a
    /// drawing appears, never how it was produced, so the statement belongs to
    /// what it shows.
    Deferred,
    /// A §3 escape hatch. What it draws is opaque to the framework, so the
    /// framework cannot say what produced it; only the escape's own
    /// introspection can.
    Opaque,
}

impl DerivesChannel {
    /// Every channel, for a consumer covering the vocabulary.
    pub const ALL: [Self; 4] = [Self::Composes, Self::Painted, Self::Deferred, Self::Opaque];

    /// The wire spelling, and the word a client matches on.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Composes => "composes",
            Self::Painted => "painted",
            Self::Deferred => "deferred",
            Self::Opaque => "opaque",
        }
    }
}

impl fmt::Display for DerivesChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name())
    }
}

/// What a tagged node answers when asked how its drawing was produced.
///
/// Four outcomes, the same shape
/// [`MarksLookup`](crate::marks::MarksLookup) has and for the same reason: a
/// client that cannot tell "no such node" from "that node stated nothing" from
/// "that kind of node has nothing to state" cannot tell a bug from a design.
#[derive(Debug, Clone, PartialEq)]
pub enum DerivationLookup<'a> {
    /// The node composes a drawing and stated how it was produced.
    Published(&'a DerivationSet),
    /// The node composes a drawing and stated nothing — a real composition,
    /// honestly silent. Either it derived nothing, or it has not been taught
    /// to say what it derived.
    Silent,
    /// The node's kind has no derivations channel, and this is why.
    NoChannel(DerivesChannel),
    /// No node in the scene carries that tag.
    NoSuchTag,
}

impl<'a> DerivationLookup<'a> {
    /// The published set, or `None` for every other outcome.
    #[must_use]
    pub const fn published(&self) -> Option<&'a DerivationSet> {
        match self {
            Self::Published(set) => Some(set),
            _ => None,
        }
    }

    /// Every published derivation of `kind`, or nothing.
    #[must_use]
    pub fn of_kind(&self, kind: DerivationKind) -> Vec<&'a Derivation> {
        self.published()
            .map_or_else(Vec::new, |set| set.of_kind(kind).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1629_the_four_kinds_are_the_cells_of_the_two_by_two() {
        // The claim the module doc makes is that the taxonomy is a product,
        // not a list. That is testable in both directions: every kind lands in
        // a cell, and every cell names a kind — so there is no kind outside
        // the table and no cell without one.
        let mut seen = Vec::new();
        for source in DerivationSource::ALL {
            for picture_has_more in [true, false] {
                let kind = DerivationKind::from_axes(source, picture_has_more);
                assert_eq!(kind.source(), source);
                assert_eq!(kind.picture_has_more(), picture_has_more);
                seen.push(kind);
            }
        }
        assert_eq!(seen.len(), DerivationKind::ALL.len());
        for kind in DerivationKind::ALL {
            assert!(seen.contains(&kind), "{kind} is not a cell of the table");
        }
    }

    #[test]
    fn r1629_a_wire_name_round_trips_and_the_names_are_distinct() {
        // The parser and the writer are one table, so a filter a client sends
        // back is the filter it was told about.
        for kind in DerivationKind::ALL {
            assert_eq!(DerivationKind::from_wire_name(kind.wire_name()), Some(kind));
        }
        let names: Vec<&str> = DerivationKind::ALL.iter().map(|k| k.wire_name()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "two kinds share a wire name");
        assert_eq!(DerivationKind::from_wire_name("nonsense"), None);
    }

    #[test]
    fn r1629_a_set_answers_by_kind_by_source_and_by_name() {
        let set = DerivationSet::over("sample")
            .stating(
                Derivation::new(DerivationKind::Invented, "overshoot", Evidence::Real(-3.5))
                    .about("series.0")
                    .in_units("value")
                    .spanning(1, 3),
            )
            .stating(
                Derivation::new(DerivationKind::Invented, "overshoot", Evidence::Real(2.0))
                    .about("series.1"),
            )
            .stating(Derivation::new(
                DerivationKind::Chosen,
                "kernel",
                Evidence::Name("gaussian".into()),
            ))
            .stating(Derivation::new(
                DerivationKind::Discarded,
                "caps",
                Evidence::Name("ohlc".into()),
            ));

        assert_eq!(set.domain(), "sample");
        assert_eq!(set.len(), 4);
        assert!(!set.is_empty());
        assert_eq!(set.of_kind(DerivationKind::Invented).count(), 2);
        assert_eq!(set.of_kind(DerivationKind::Omitted).count(), 0);
        // A whole row of the table: everything the DATA does not support.
        assert_eq!(set.against(DerivationSource::Data).count(), 2);
        assert_eq!(set.against(DerivationSource::Request).count(), 2);
        // A name repeats across subjects and every one is answered.
        let shoots: Vec<&Derivation> = set.named("overshoot").collect();
        assert_eq!(shoots.len(), 2);
        assert_eq!(shoots[0].subject(), Some("series.0"));
        assert_eq!(shoots[1].subject(), Some("series.1"));
        assert_eq!(shoots[0].span(), Some((1, 3)));
        assert_eq!(shoots[1].span(), None, "the second one is not localized");
        assert_eq!(shoots[0].unit(), Some("value"));
    }

    #[test]
    fn r1629_an_inverted_span_is_stored_in_order() {
        // A published span is read as a width by whoever draws a caption from
        // it, so it can never be allowed to report a negative one.
        let d = Derivation::new(DerivationKind::Omitted, "off_scale", Evidence::Count(2))
            .spanning(9, 4);
        assert_eq!(d.span(), Some((4, 9)));
    }

    #[test]
    fn r1629_evidence_answers_a_quantity_only_where_there_is_one() {
        assert_eq!(Evidence::Real(0.5).quantity(), Some(0.5));
        assert_eq!(Evidence::Count(3).quantity(), Some(3.0));
        assert_eq!(Evidence::Flag(true).quantity(), None);
        assert_eq!(Evidence::Name("gaussian".into()).quantity(), None);

        let names: Vec<&str> = [
            Evidence::Name("x".into()),
            Evidence::Real(1.0),
            Evidence::Count(1),
            Evidence::Flag(false),
        ]
        .iter()
        .map(Evidence::wire_name)
        .collect();
        assert_eq!(names, vec!["name", "real", "count", "flag"]);
    }

    #[test]
    fn r1629_the_lookup_separates_the_three_ways_of_having_no_answer() {
        let set = DerivationSet::over("sample").stating(Derivation::new(
            DerivationKind::Chosen,
            "bandwidth",
            Evidence::Real(1.5),
        ));
        let published = DerivationLookup::Published(&set);
        assert!(published.published().is_some());
        assert_eq!(published.of_kind(DerivationKind::Chosen).len(), 1);
        assert_eq!(published.of_kind(DerivationKind::Invented).len(), 0);

        for absent in [
            DerivationLookup::Silent,
            DerivationLookup::NoChannel(DerivesChannel::Painted),
            DerivationLookup::NoSuchTag,
        ] {
            assert!(absent.published().is_none());
            assert!(absent.of_kind(DerivationKind::Chosen).is_empty());
        }
        // And they are three answers, not one.
        assert_ne!(
            DerivationLookup::<'_>::Silent,
            DerivationLookup::NoChannel(DerivesChannel::Painted)
        );
        assert_ne!(DerivationLookup::<'_>::Silent, DerivationLookup::NoSuchTag);
    }

    #[test]
    fn r1629_stating_all_keeps_declaration_order() {
        let set = DerivationSet::over("sample").stating_all([
            Derivation::new(DerivationKind::Omitted, "a", Evidence::Count(1)),
            Derivation::new(DerivationKind::Omitted, "b", Evidence::Count(2)),
        ]);
        let names: Vec<&str> = set.entries().iter().map(Derivation::name).collect();
        assert_eq!(names, vec!["a", "b"]);
    }
}
