//! A **link group** — the declared set of views one cross-filter reaches, and
//! the report of what it actually reached.
//!
//! # The defect this exists to remove
//!
//! Before this module a cross-filter was an *imperative call, written once per
//! view*. A consumer read a [`Brush`](crate::Brush), mapped it to a window, and
//! then wrote `.select_x_range(Some(window))` by hand on each chart it happened
//! to hold. Measured across this tree at R1806, every cross-filter call site in
//! `crates/` and `examples/` is exactly that — four demos holding two, two, one
//! and one call each. **Nothing anywhere declared a set**, and the analysis
//! tool's own dashboard held none of them at all.
//!
//! That arrangement has one failure mode and it is silent. Add a sixth view to
//! a board and forget the call, and the view renders *unfiltered* while the
//! five beside it narrow. No type is violated, no assertion fails, no gate
//! fires — the view simply keeps showing everything, which looks exactly like a
//! view that legitimately had nothing to hide. This is the class this
//! repository has now measured on three unrelated axes in three consecutive
//! rounds: **a declared constraint silently loses to an imperative call.**
//!
//! # What replaces it
//!
//! A [`LinkGroup`] is a *value*: the views that participate, each one saying
//! which [`Domain`]s of selection it can accept. Publishing a [`Selection`]
//! into the group returns a [`Reach`], and a `Reach`
//! **accounts for every declared view** — each is either in
//! [`reached`](Reach::reached) or in [`refused`](Reach::refused) with a
//! [`Refusal`] that says why. There is no third outcome and no way to fall out
//! of the set, because both halves are built from one pass over the same
//! declaration.
//!
//! Three things follow that a hand-written call cannot give:
//!
//! * **"every linked view" becomes a set you can name**, so a test asserts a
//!   set rather than a count. A count of two is equally true of the right two
//!   views and the wrong two.
//! * **A view that cannot participate says so.** A distribution over latency
//!   cannot answer a question about message *kind*; today that view would
//!   simply not narrow, and a reader could not tell "no matching data" from
//!   "nobody wired it". [`Refusal`] distinguishes them.
//! * **A view drawn but never declared is catchable.** [`LinkGroup::audit`]
//!   takes the set of views actually painted and names the ones missing from
//!   the declaration — the forgotten-call failure, turned from silence into a
//!   sentence.
//!
//! # Applying a reach
//!
//! This module owns the *selection and its accounting only*. What a view does
//! with the selection it was handed stays in the view, exactly as it does for
//! [`Brush`](crate::Brush).
//!
//! For a view this crate draws, that half is [`crate::mute`] (R1824): every
//! chart kind implements [`Mute`](crate::Mute), a
//! [`Reach`] is applied with
//! [`muted_by_reach`](crate::Mute::muted_by_reach), and the marks a selection
//! does not cover are dimmed. **Until that module existed only three of the ten
//! kinds could be told about a selection at all**, so a board could declare a
//! ring chart, publish, be told it was reached, and paint it unchanged — the
//! declaration was checkable and the drawing was not. A chart's own [`Link`] is
//! now derived from its marks ([`Mute::link`](crate::Mute::link)), which is what
//! closes that gap rather than papering over it.
//!
//! A view that is not a chart at all — a table of rows, a tree of decoded
//! fields — participates on the same terms: it takes a `Selection` and decides
//! what to mute. Nothing here requires the view to be drawn by this crate.
//!
//! # Why the domain is part of the selection
//!
//! [`Brush`](crate::Brush) can say one thing: an interval on one numeric axis.
//! Most chart geometries do not select that way — a ring chart selects a
//! *category*, a polar geometry an angular sector and a radial band, a timeline
//! a lane and a time window. Had this module let a selection be a bare
//! `(f64, f64)` and left each geometry to reinterpret it, the second geometry
//! added would have had to re-decide what the pair meant, differently. Carrying
//! the [`Domain`] in the value settles it once: a view declares what it speaks,
//! and a mismatch is a refusal with both sides named rather than a coincidence
//! of arity.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The kind of thing a [`Selection`] selects — and therefore the vocabulary a
/// view must speak to be reached by one.
///
/// A view declares the domains it accepts ([`Link::new`]); a selection carries
/// exactly one ([`Selection::domain`]). The pair is what makes a refusal
/// mechanical and explicable instead of a silent no-op.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, pinion_derive::VariantCensus,
)]
#[variant_census(all)]
pub enum Domain {
    /// A window on one numeric axis, in the data's own units — what a
    /// [`Brush`](crate::Brush) produces and what `select_x_range` consumes.
    #[default]
    XRange,
    /// One named category out of a nominal vocabulary — what a ring chart
    /// slice, a legend entry or a saved-filter chip selects.
    Category,
    /// An angular sector together with a radial band: two axes, one of them
    /// cyclic. The polar geometries' domain.
    Sector,
    /// One lane together with a time window — two dimensions that are not
    /// interchangeable, which is why this is not two `XRange`s.
    LaneWindow,
}

impl Domain {
    /// Every domain, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 4] = [Self::XRange, Self::Category, Self::Sector, Self::LaneWindow];

    /// Stable name, for a wire form, a caption or a refusal sentence.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::XRange => "x-range",
            Self::Category => "category",
            Self::Sector => "sector",
            Self::LaneWindow => "lane-window",
        }
    }
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What a cross-filter selects, in the domain it selects it in.
///
/// The value a driver publishes into a [`LinkGroup`]. Its
/// [`domain`](Self::domain) is what decides which declared views can be reached
/// at all.
#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    /// The half-open window `[lo, hi)` on one numeric axis, in data units.
    XRange {
        /// The window's lower edge, included.
        lo: f64,
        /// The window's upper edge, excluded.
        hi: f64,
    },
    /// One category out of a nominal vocabulary, by name.
    Category(String),
    /// An angular sector `angle` (in the chart's own angular units) together
    /// with the radial band `radius`.
    Sector {
        /// The angular interval selected.
        angle: (f64, f64),
        /// The radial interval selected.
        radius: (f64, f64),
    },
    /// One lane, together with the time window selected inside it.
    LaneWindow {
        /// The lane's name.
        lane: String,
        /// The half-open time window inside that lane.
        window: (f64, f64),
    },
}

impl Selection {
    /// The domain this selection speaks in.
    #[must_use]
    pub const fn domain(&self) -> Domain {
        match self {
            Self::XRange { .. } => Domain::XRange,
            Self::Category(_) => Domain::Category,
            Self::Sector { .. } => Domain::Sector,
            Self::LaneWindow { .. } => Domain::LaneWindow,
        }
    }

    /// The numeric window, when this is one — ready to hand to a chart's
    /// `select_x_range`. `None` in every other domain, so a view that only
    /// knows how to narrow numerically cannot silently misread a category.
    #[must_use]
    pub const fn x_range(&self) -> Option<(f64, f64)> {
        match self {
            Self::XRange { lo, hi } => Some((*lo, *hi)),
            _ => None,
        }
    }

    /// The selected category, when this is one.
    #[must_use]
    pub fn category(&self) -> Option<&str> {
        match self {
            Self::Category(name) => Some(name.as_str()),
            _ => None,
        }
    }

    /// The selected angular sector and radial band, when this is one — the
    /// pair a polar geometry tests its marks against.
    ///
    /// R1824. Until then this variant could be *constructed* and never read,
    /// which is what "a declared vocabulary with no implementation" looks like
    /// from the outside: [`Domain::Sector`] was a value nothing consumed. See
    /// [`crate::mute`].
    #[must_use]
    pub const fn sector(&self) -> Option<((f64, f64), (f64, f64))> {
        match self {
            Self::Sector { angle, radius } => Some((*angle, *radius)),
            _ => None,
        }
    }

    /// The selected lane and the time window inside it, when this is one.
    ///
    /// R1824, and the same story as [`sector`](Self::sector): the twin of
    /// [`x_range`](Self::x_range) for the two-dimensional domain, so a track
    /// view can narrow to one lane without a caller having to reach into the
    /// enum.
    #[must_use]
    pub fn lane_window(&self) -> Option<(&str, (f64, f64))> {
        match self {
            Self::LaneWindow { lane, window } => Some((lane.as_str(), *window)),
            _ => None,
        }
    }
}

impl fmt::Display for Selection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::XRange { lo, hi } => write!(f, "x-range {lo}..{hi}"),
            Self::Category(name) => write!(f, "category {name}"),
            Self::Sector { angle, radius } => write!(
                f,
                "sector angle {}..{} radius {}..{}",
                angle.0, angle.1, radius.0, radius.1
            ),
            Self::LaneWindow { lane, window } => {
                write!(f, "lane {lane} window {}..{}", window.0, window.1)
            }
        }
    }
}

/// One view's declaration that it participates in a [`LinkGroup`].
///
/// A view is named by the same string the rest of the application knows it by
/// (a card id, a tag, a panel name) — [`Reach::selection_for`] is looked up with
/// it and [`LinkGroup::audit`] compares it against what was painted, so the two
/// must be the same vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    name: String,
    accepts: BTreeSet<Domain>,
    /// `Some` exactly when the view accepts nothing *deliberately*: the reason
    /// its author gave. An empty `accepts` with no reason is refused by
    /// [`LinkGroup::new`].
    inert: Option<String>,
}

impl Link {
    /// A view that accepts the given domains.
    ///
    /// Passing no domains produces a view that can never be reached and cannot
    /// say why, so use [`inert`](Self::inert) for that case instead — it takes
    /// the reason. A `Link::new` with an empty domain set is reported by
    /// [`LinkGroup::new`] as [`LinkFault::MuteWithoutReason`].
    #[must_use]
    pub fn new(name: impl Into<String>, accepts: impl IntoIterator<Item = Domain>) -> Self {
        Self {
            name: name.into(),
            accepts: accepts.into_iter().collect(),
            inert: None,
        }
    }

    /// A view that belongs to the group but accepts **nothing**, together with
    /// the reason it cannot.
    ///
    /// This is the case a hand-written cross-filter cannot express. A board may
    /// hold a view whose data is simply not drawn from the filtered population
    /// — a key legend beside four views over a capture. Leaving it out of the
    /// group would make [`audit`](LinkGroup::audit) call it undeclared; giving
    /// it an empty domain set would make its refusal indistinguishable from a
    /// mismatch. Declaring it inert says the true thing: it is part of the
    /// board, it will never narrow, and here is why.
    #[must_use]
    pub fn inert(name: impl Into<String>, why: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            accepts: BTreeSet::new(),
            inert: Some(why.into()),
        }
    }

    /// The view's name — its address in [`Reach`] and in [`LinkGroup::audit`].
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether this view accepts selections in `domain`.
    #[must_use]
    pub fn accepts(&self, domain: Domain) -> bool {
        self.accepts.contains(&domain)
    }

    /// The domains this view accepts, in [`Domain`]'s own order.
    pub fn domains(&self) -> impl Iterator<Item = Domain> + '_ {
        self.accepts.iter().copied()
    }

    /// The stated reason this view accepts nothing, when it was declared
    /// [`inert`](Self::inert).
    #[must_use]
    pub fn inert_reason(&self) -> Option<&str> {
        self.inert.as_deref()
    }
}

/// Why a declared view was **not** reached by a published selection.
///
/// Every view the group declares gets either a place in [`Reach::reached`] or
/// one of these. Silence is not among the outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The view speaks other domains than the one selected. Both sides are
    /// named, because "it did not narrow" is not a diagnosis.
    Domain {
        /// The domain the published selection spoke in.
        selected: Domain,
        /// The domains this view declared it accepts, in [`Domain`] order.
        accepts: Vec<Domain>,
    },
    /// The view declared itself outside the filtered population, and said why
    /// ([`Link::inert`]).
    Inert {
        /// The reason its author gave.
        why: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain { selected, accepts } if accepts.is_empty() => {
                write!(f, "accepts no selection at all, and this one is {selected}")
            }
            Self::Domain { selected, accepts } => {
                let names: Vec<&str> = accepts.iter().map(|d| d.name()).collect();
                write!(
                    f,
                    "selects by {}, and this selection is {selected}",
                    names.join(" or ")
                )
            }
            Self::Inert { why } => f.write_str(why),
        }
    }
}

/// An arrangement a [`LinkGroup`] refuses to be built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkFault {
    /// Two links carry the same name. One would shadow the other in
    /// [`Reach::selection_for`] and the group would silently hold fewer views
    /// than its author wrote — the exact failure this module exists to end.
    DuplicateName(String),
    /// A [`Link::new`] with no domains: a view that can never be reached and
    /// has no reason to give. [`Link::inert`] is how to say that on purpose.
    MuteWithoutReason(String),
}

impl fmt::Display for LinkFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateName(name) => {
                write!(f, "two linked views are both named `{name}`")
            }
            Self::MuteWithoutReason(name) => write!(
                f,
                "linked view `{name}` accepts no domain and gives no reason; \
                 declare it inert with the reason instead"
            ),
        }
    }
}

impl std::error::Error for LinkFault {}

/// The declared set of views one cross-filter reaches.
///
/// Built once from the views a board holds, then asked
/// ([`publish`](Self::publish)) what a given selection reaches. See the module
/// documentation for why this is a value rather than a sequence of calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkGroup {
    links: Vec<Link>,
}

impl LinkGroup {
    /// Declare a group from its views.
    ///
    /// # Errors
    ///
    /// [`LinkFault::DuplicateName`] when two views share a name, and
    /// [`LinkFault::MuteWithoutReason`] when a view accepts no domain without
    /// being declared [`inert`](Link::inert). Both are authoring mistakes whose
    /// runtime symptom would be a view that quietly never narrows, so they are
    /// refused at construction rather than diagnosed later.
    pub fn new(links: impl IntoIterator<Item = Link>) -> Result<Self, LinkFault> {
        let links: Vec<Link> = links.into_iter().collect();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for link in &links {
            if !seen.insert(link.name()) {
                return Err(LinkFault::DuplicateName(link.name().to_string()));
            }
            if link.accepts.is_empty() && link.inert.is_none() {
                return Err(LinkFault::MuteWithoutReason(link.name().to_string()));
            }
        }
        Ok(Self { links })
    }

    /// The views this group declares, by name.
    #[must_use]
    pub fn declared(&self) -> BTreeSet<&str> {
        self.links.iter().map(Link::name).collect()
    }

    /// The declared views, in declaration order.
    pub fn links(&self) -> impl Iterator<Item = &Link> {
        self.links.iter()
    }

    /// One declared view, by name.
    #[must_use]
    pub fn link(&self, name: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.name() == name)
    }

    /// Publish `selection` into the group: the set it reaches, and for every
    /// view it does not, the reason.
    ///
    /// The two halves are built in one pass over the declaration, so
    /// [`Reach::accounted`] equals [`declared`](Self::declared) by construction
    /// — a view cannot fall out of the accounting.
    #[must_use]
    pub fn publish(&self, selection: &Selection) -> Reach {
        let domain = selection.domain();
        let mut reached = BTreeSet::new();
        let mut refused = BTreeMap::new();
        for link in &self.links {
            if link.accepts(domain) {
                reached.insert(link.name().to_string());
            } else if let Some(why) = link.inert_reason() {
                refused.insert(
                    link.name().to_string(),
                    Refusal::Inert {
                        why: why.to_string(),
                    },
                );
            } else {
                refused.insert(
                    link.name().to_string(),
                    Refusal::Domain {
                        selected: domain,
                        accepts: link.domains().collect(),
                    },
                );
            }
        }
        Reach {
            selection: selection.clone(),
            reached,
            refused,
        }
    }

    /// Compare the declaration against the views actually **painted**.
    ///
    /// This is the half a per-view call can never have. A board paints what it
    /// paints; the group declares what it declares; nothing else in a running
    /// application compares the two, so a view added to the board and forgotten
    /// here would render unfiltered for as long as nobody looked. Give this the
    /// names the board drew and it reports both directions.
    #[must_use]
    pub fn audit(&self, painted: &BTreeSet<String>) -> Audit {
        let declared: BTreeSet<String> = self.links.iter().map(|l| l.name().to_string()).collect();
        Audit {
            undeclared: painted.difference(&declared).cloned().collect(),
            undrawn: declared.difference(painted).cloned().collect(),
        }
    }
}

/// What publishing a [`Selection`] into a [`LinkGroup`] reached — and, for
/// every view it did not, why not.
#[derive(Debug, Clone, PartialEq)]
pub struct Reach {
    selection: Selection,
    reached: BTreeSet<String>,
    refused: BTreeMap<String, Refusal>,
}

impl Reach {
    /// The selection that was published.
    #[must_use]
    pub const fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The views this selection reached — **the set** the census sentence
    /// "every linked view" is about.
    #[must_use]
    pub const fn reached(&self) -> &BTreeSet<String> {
        &self.reached
    }

    /// The views it did not reach, each with its [`Refusal`].
    #[must_use]
    pub const fn refused(&self) -> &BTreeMap<String, Refusal> {
        &self.refused
    }

    /// Whether `view` was reached.
    #[must_use]
    pub fn reaches(&self, view: &str) -> bool {
        self.reached.contains(view)
    }

    /// The selection `view` must apply, or `None` if it was refused (or is not
    /// in the group at all).
    ///
    /// A view asks this instead of being handed a window, so "was I part of
    /// this?" and "what do I narrow to?" are one question with one answer.
    #[must_use]
    pub fn selection_for(&self, view: &str) -> Option<&Selection> {
        self.reaches(view).then_some(&self.selection)
    }

    /// Why `view` was not reached, as a sentence. `None` when it was reached,
    /// or is not declared.
    #[must_use]
    pub fn reason(&self, view: &str) -> Option<String> {
        self.refused.get(view).map(ToString::to_string)
    }

    /// Every view the group declared — reached and refused together.
    ///
    /// Equal to [`LinkGroup::declared`] for the group that produced it. That
    /// identity is what "accounts for every declared view" means, and the
    /// crate's tests pin it.
    #[must_use]
    pub fn accounted(&self) -> BTreeSet<&str> {
        self.reached
            .iter()
            .map(String::as_str)
            .chain(self.refused.keys().map(String::as_str))
            .collect()
    }
}

/// The result of comparing a [`LinkGroup`]'s declaration against the views a
/// board actually painted ([`LinkGroup::audit`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Audit {
    undeclared: BTreeSet<String>,
    undrawn: BTreeSet<String>,
}

impl Audit {
    /// Views that were painted but are **not** in the link group — each one a
    /// view that will never narrow and never say so.
    #[must_use]
    pub const fn undeclared(&self) -> &BTreeSet<String> {
        &self.undeclared
    }

    /// Views the group declares that were not painted. Not necessarily wrong —
    /// a board may legitimately hide a card — but it is the other direction of
    /// the same drift, so it is reported rather than dropped.
    #[must_use]
    pub const fn undrawn(&self) -> &BTreeSet<String> {
        &self.undrawn
    }

    /// Whether the declaration and the painting agree exactly.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.undeclared.is_empty() && self.undrawn.is_empty()
    }

    /// The disagreement as a sentence, or `None` when there is none.
    #[must_use]
    pub fn fault(&self) -> Option<String> {
        if self.agrees() {
            return None;
        }
        let mut parts = Vec::new();
        if !self.undeclared.is_empty() {
            let names: Vec<&str> = self.undeclared.iter().map(String::as_str).collect();
            parts.push(format!(
                "painted but not linked: {} (each will never narrow)",
                names.join(", ")
            ));
        }
        if !self.undrawn.is_empty() {
            let names: Vec<&str> = self.undrawn.iter().map(String::as_str).collect();
            parts.push(format!("linked but not painted: {}", names.join(", ")));
        }
        Some(parts.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The board this module was built for: the analysis tool's dashboard, in
    /// miniature. Three views over the capture, one legend that is not.
    fn board() -> LinkGroup {
        LinkGroup::new([
            Link::new("packet", [Domain::Category, Domain::XRange]),
            Link::new("decode", [Domain::Category]),
            Link::new("latency", [Domain::XRange]),
            Link::inert("keymap", "a key legend, not capture data"),
        ])
        .expect("the miniature board is well formed")
    }

    // ── the accounting identity ──────────────────────────────────────────

    #[test]
    fn every_declared_view_is_accounted_for_in_every_domain() {
        let group = board();
        for domain in Domain::ALL {
            let selection = match domain {
                Domain::XRange => Selection::XRange { lo: 0.0, hi: 1.0 },
                Domain::Category => Selection::Category("Data".into()),
                Domain::Sector => Selection::Sector {
                    angle: (0.0, 1.0),
                    radius: (0.0, 1.0),
                },
                Domain::LaneWindow => Selection::LaneWindow {
                    lane: "l".into(),
                    window: (0.0, 1.0),
                },
            };
            let reach = group.publish(&selection);
            assert_eq!(
                reach.accounted(),
                group.declared(),
                "publishing a {domain} selection left a declared view unaccounted for"
            );
            // And the two halves never overlap: a view is reached or refused,
            // never both, so the identity above cannot be met by double-counting.
            for name in reach.reached() {
                assert!(
                    !reach.refused().contains_key(name),
                    "{name} is both reached and refused"
                );
            }
        }
    }

    // ── the reach is a SET, and it is the right set ──────────────────────

    #[test]
    fn a_category_selection_reaches_the_views_that_select_by_category() {
        let reach = board().publish(&Selection::Category("Data".into()));
        assert_eq!(
            reach
                .reached()
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["decode", "packet"]),
            "a category selection reaches exactly the category-speaking views"
        );
        assert_eq!(
            reach
                .refused()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["keymap", "latency"])
        );
    }

    #[test]
    fn an_x_range_selection_reaches_a_different_set_of_the_same_board() {
        let reach = board().publish(&Selection::XRange { lo: 8.0, hi: 16.0 });
        assert_eq!(
            reach
                .reached()
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["latency", "packet"]),
            "the numeric window reaches the numeric views, not the same two"
        );
    }

    #[test]
    fn a_domain_no_view_speaks_reaches_nothing_and_still_accounts_for_all() {
        let group = board();
        let reach = group.publish(&Selection::Sector {
            angle: (0.0, 90.0),
            radius: (0.0, 1.0),
        });
        assert!(reach.reached().is_empty());
        assert_eq!(reach.accounted(), group.declared());
    }

    // ── a refusal says WHY, and the two reasons are distinguishable ──────

    #[test]
    fn a_domain_refusal_names_both_sides() {
        let reach = board().publish(&Selection::Category("Data".into()));
        assert_eq!(
            reach.refused().get("latency"),
            Some(&Refusal::Domain {
                selected: Domain::Category,
                accepts: vec![Domain::XRange],
            })
        );
        let said = reach
            .reason("latency")
            .expect("a refused view has a reason");
        assert!(
            said.contains("x-range") && said.contains("category"),
            "the sentence names what the view speaks and what was published: {said}"
        );
    }

    #[test]
    fn an_inert_refusal_carries_the_authors_reason_not_a_domain_list() {
        let reach = board().publish(&Selection::Category("Data".into()));
        assert_eq!(
            reach.reason("keymap").as_deref(),
            Some("a key legend, not capture data"),
            "an abstaining view says its own reason"
        );
        // The distinction the module exists to keep: "cannot answer THIS
        // question" and "is not part of this population" are different facts.
        assert!(matches!(
            reach.refused().get("keymap"),
            Some(Refusal::Inert { .. })
        ));
        assert!(matches!(
            reach.refused().get("latency"),
            Some(Refusal::Domain { .. })
        ));
    }

    #[test]
    fn a_reached_view_has_no_reason_and_a_refused_view_has_no_selection() {
        let reach = board().publish(&Selection::Category("Data".into()));
        assert!(reach.reason("packet").is_none());
        assert_eq!(
            reach.selection_for("packet"),
            Some(&Selection::Category("Data".into()))
        );
        assert!(
            reach.selection_for("latency").is_none(),
            "a refused view is handed nothing, so it cannot half-apply a filter"
        );
        assert!(
            reach.selection_for("nobody").is_none(),
            "a name the group never declared is not reached either"
        );
    }

    // ── the authoring mistakes are refused at construction ───────────────

    #[test]
    fn two_views_of_the_same_name_are_refused() {
        let fault = LinkGroup::new([
            Link::new("packet", [Domain::Category]),
            Link::new("packet", [Domain::XRange]),
        ])
        .expect_err("a duplicate name must not build");
        assert_eq!(fault, LinkFault::DuplicateName("packet".into()));
        assert!(fault.to_string().contains("packet"));
    }

    #[test]
    fn a_view_that_accepts_nothing_must_say_why() {
        let fault = LinkGroup::new([Link::new("mystery", [])])
            .expect_err("an unexplained mute must not build");
        assert_eq!(fault, LinkFault::MuteWithoutReason("mystery".into()));
        // And the explained form of the same thing builds.
        assert!(LinkGroup::new([Link::inert("mystery", "not capture data")]).is_ok());
    }

    // ── the audit: a view painted but never declared ─────────────────────

    #[test]
    fn the_audit_names_a_painted_view_that_nobody_linked() {
        let painted: BTreeSet<String> = ["packet", "decode", "latency", "keymap", "throughput"]
            .into_iter()
            .map(String::from)
            .collect();
        let audit = board().audit(&painted);
        assert!(!audit.agrees());
        assert_eq!(
            audit.undeclared(),
            &BTreeSet::from(["throughput".to_string()]),
            "the sixth card, added to the board and forgotten here"
        );
        assert!(audit.undrawn().is_empty());
        let fault = audit.fault().expect("a disagreement has a sentence");
        assert!(fault.contains("throughput") && fault.contains("never narrow"));
    }

    #[test]
    fn the_audit_names_a_linked_view_that_was_not_painted() {
        let painted: BTreeSet<String> = ["packet", "decode", "latency"]
            .into_iter()
            .map(String::from)
            .collect();
        let audit = board().audit(&painted);
        assert_eq!(audit.undrawn(), &BTreeSet::from(["keymap".to_string()]));
        assert!(audit.undeclared().is_empty());
    }

    #[test]
    fn an_agreeing_board_has_no_fault_sentence() {
        let painted: BTreeSet<String> = board().declared().into_iter().map(String::from).collect();
        let audit = board().audit(&painted);
        assert!(audit.agrees());
        assert!(audit.fault().is_none());
    }

    // ── the domain vocabulary ────────────────────────────────────────────

    #[test]
    fn every_domain_has_a_distinct_name_and_a_selection_that_reports_it() {
        let names: BTreeSet<&str> = Domain::ALL.iter().map(|d| d.name()).collect();
        assert_eq!(names.len(), Domain::ALL.len(), "the names are distinct");
        assert_eq!(
            Selection::XRange { lo: 0.0, hi: 1.0 }.domain(),
            Domain::XRange
        );
        assert_eq!(Selection::Category("k".into()).domain(), Domain::Category);
        assert_eq!(
            Selection::Sector {
                angle: (0.0, 1.0),
                radius: (0.0, 1.0)
            }
            .domain(),
            Domain::Sector
        );
        assert_eq!(
            Selection::LaneWindow {
                lane: "l".into(),
                window: (0.0, 1.0)
            }
            .domain(),
            Domain::LaneWindow
        );
    }

    #[test]
    fn an_accessor_answers_only_in_its_own_domain() {
        let category = Selection::Category("Data".into());
        assert_eq!(category.category(), Some("Data"));
        assert!(
            category.x_range().is_none(),
            "a category must not read as a numeric window"
        );
        let window = Selection::XRange { lo: 8.0, hi: 16.0 };
        assert_eq!(window.x_range(), Some((8.0, 16.0)));
        assert!(window.category().is_none());
    }
}
