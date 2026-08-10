//! R1629 §5.11 §5.28 §2 #7 — the vocabulary this crate's charts publish on
//! [`pinion_core::derivation`], and the mechanical parts of building it.
//!
//! # Why the reports move onto the scene
//!
//! Every chart here already answers "what did you do that the picture cannot
//! give back": `LineChart::overshoot`, `off_scale` on five charts,
//! `BoxPlotChart::without_density`, and everything a [`Density`](crate::Density)
//! knows about its own estimate. All of it was **in-process Rust**, and the
//! §2 #2 primary client holds a `Scene` and no chart. So the strongest reports
//! in the crate were invisible to the reader they were written for.
//!
//! # Two rules that keep the report honest
//!
//! **A `Chosen` entry is always published; an `Invented`, `Omitted` or
//! `Discarded` one exists only when there is something to report.** A client
//! filtering for `invented` must get a non-empty answer *exactly* when the
//! picture shows something the data does not — publishing `spill = 0` would
//! make "did this chart invent anything" answer yes for every violin ever
//! drawn. The choices go the other way: a bandwidth that was defaulted is
//! still a decision the reader did not make, and it is unreachable from the
//! picture whether or not it was explicit.
//!
//! **One entry per (name, subject), not per datum.** A scatter with 10,000
//! off-scale points publishes one count per series, not 10,000 statements. A
//! report that grows with the data is a report nobody can read and a scene
//! nobody can serialize; the in-process accessors still hand back every
//! element for a caller that wants them.
//!
//! # What is deliberately NOT here
//!
//! The channel publishes **disagreements between the picture and its sources**
//! — see [`pinion_core::derivation::DerivationKind`]'s 2×2.
//! Plain provenance that is neither ("this chart has four series") is not a
//! derivation and is not smuggled in as one; the scene already carries the
//! series as nodes. Keeping the taxonomy closed is what lets a client write an
//! exhaustive match over it.

use std::collections::BTreeMap;

use pinion_core::Scene;
use pinion_core::derivation::{Derivation, DerivationKind, DerivationSet, Evidence};
use pinion_core::scene::ContainerNode;

/// The stable names a chart's derivations carry. A client matches these, so
/// they are constants rather than literals repeated at each emit site.
pub(crate) mod name {
    /// The join between consecutive samples — a choice, always published.
    pub const INTERPOLATION: &str = "interpolation";
    /// A curve that left the range its own samples span.
    pub const OVERSHOOT: &str = "overshoot";
    /// How many segments of a series overshot.
    pub const OVERSHOOT_SEGMENTS: &str = "overshoot_segments";
    /// Data the axis cannot place, so the picture does not show it.
    pub const OFF_SCALE: &str = "off_scale";
    /// A bearing a periodic axis carried by wrapping it, so the mark sits at
    /// an angle the datum never took.
    pub const WRAPPED: &str = "wrapped";
    /// The mark a series or session is drawn as.
    pub const MARK: &str = "mark";
    /// A candlestick option with no meaning under the mark that was drawn.
    pub const CAPS: &str = "caps";
    /// The kernel a density estimate smooths with.
    pub const KERNEL: &str = "kernel";
    /// The resolved bandwidth of a density estimate.
    pub const BANDWIDTH: &str = "bandwidth";
    /// The rule that resolved the bandwidth.
    pub const BANDWIDTH_RULE: &str = "bandwidth_rule";
    /// Whether the kernel was reflected at the observed extremes.
    pub const BOUNDED: &str = "bounded";
    /// The share of estimated mass outside the range the samples spanned.
    pub const SPILL: &str = "spill";
    /// The individual measurements an outline replaced.
    pub const SAMPLES: &str = "samples";
    /// A density that was asked for and could not be estimated.
    pub const DENSITY: &str = "density";
    /// How violin widths are scaled against one another.
    pub const VIOLIN_SCALE: &str = "violin_scale";
}

/// The units a [`Evidence::Real`] is measured in.
pub(crate) mod unit {
    /// The value axis' own units — whatever the data is in.
    pub const VALUE: &str = "value";
    /// A share of a whole, in `0.0..=1.0`.
    pub const FRACTION: &str = "fraction";
}

/// The index spaces a chart's spans count in. Shares
/// [`pinion_core::marks::domain`]'s role: one vocabulary of index spaces, so
/// `sample` means the same thing to a mark and to a derivation.
pub(crate) mod domain {
    /// Positions in a series' own point order.
    pub const SAMPLE: &str = "sample";
    /// Slots along a categorical axis — one distribution, one session, one
    /// bar.
    pub const SLOT: &str = "slot";
}

/// A series' subject spelling: its index in the chart's own order, which is
/// the index every in-process report keys on.
pub(crate) fn series_subject(index: usize) -> String {
    format!("series.{index}")
}

/// A categorical slot's subject spelling — a distribution or a session.
pub(crate) fn slot_subject(index: usize) -> String {
    format!("slot.{index}")
}

/// Tally `subjects` and emit one `kind` derivation named `name` per distinct
/// subject, in **first-seen** order, or nothing when the iterator is empty.
///
/// The mechanical half of every per-datum report in this crate. Five charts
/// report off-scale data through four different element types, a sixth
/// reports wrapped bearings, and all of them want the same thing said about it
/// — "this subject has N of these" — so the counting lives here rather than
/// six times over (the R1553 rule this crate lifted `scene_probe` under).
pub(crate) fn counts_by_subject(
    kind: DerivationKind,
    name: &'static str,
    subjects: impl IntoIterator<Item = String>,
) -> Vec<Derivation> {
    // Ordered by first appearance, not by subject spelling: `slot.10` sorts
    // before `slot.2` and a reader following the chart left to right would
    // see the report jump.
    let mut order: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for subject in subjects {
        let seen = counts.entry(subject.clone()).or_insert(0);
        if *seen == 0 {
            order.push(subject);
        }
        *seen += 1;
    }
    order
        .into_iter()
        .map(|subject| {
            let count = counts[&subject];
            Derivation::new(kind, name, Evidence::Count(count)).about(subject)
        })
        .collect()
}

/// [`counts_by_subject`] at the kind every axis report uses: data the picture
/// does not show.
pub(crate) fn omitted_counts(
    name: &'static str,
    subjects: impl IntoIterator<Item = String>,
) -> Vec<Derivation> {
    counts_by_subject(DerivationKind::Omitted, name, subjects)
}

/// The root node **every** chart in this crate returns from `build_body`.
///
/// It takes the derivation set rather than defaulting it, which is the point:
/// a chart added later cannot reach the scene without deciding what its
/// drawing derived. The alternative — a hand-written census of chart types,
/// asserted in a test — is the shape that goes stale silently, and R1619
/// recorded the general form: an idempotent writer removes the census.
///
/// A chart with nothing to state passes an **empty** set, which is a different
/// answer from not passing one: `Published` with no entries says "I ran my
/// reports and the picture hides nothing", while
/// [`Silent`](pinion_core::derivation::DerivationLookup::Silent) says "this
/// composition does not answer".
pub(crate) fn chart_root(
    children: Vec<Scene>,
    tag: String,
    derivations: DerivationSet,
) -> ContainerNode {
    ContainerNode::new(children)
        .with_tag(tag)
        .with_derivations(derivations)
}

/// The [`Chosen`](DerivationKind::Chosen) derivation naming a setting whose
/// value is a stable name — an interpolation, a kernel, a mark.
pub(crate) fn chosen_name(name: &'static str, value: &'static str) -> Derivation {
    Derivation::new(DerivationKind::Chosen, name, Evidence::Name(value.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1629_counts_are_per_subject_and_in_first_seen_order() {
        // ★ The subjects are chosen so first-seen order and the tally map's
        // own key order DISAGREE: `"slot.10" < "slot.2"` lexicographically, so
        // a fixture that met slot.10 first would pass under either rule and
        // prove nothing. A counterfactual iterating the map found exactly
        // that hole in this test's first draft.
        let found = omitted_counts(
            name::OFF_SCALE,
            [
                slot_subject(2),
                slot_subject(10),
                slot_subject(2),
                slot_subject(2),
            ],
        );
        assert_eq!(found.len(), 2, "one entry per subject, not per datum");
        assert_eq!(found[0].subject(), Some("slot.2"), "first seen is first");
        assert_eq!(found[0].evidence(), &Evidence::Count(3));
        assert_eq!(found[1].subject(), Some("slot.10"));
        assert_eq!(found[1].evidence(), &Evidence::Count(1));
        // ...and the map's order is the other one, so a reader following the
        // chart left to right would see the report jump.
        let mut by_key: Vec<&str> = found.iter().filter_map(Derivation::subject).collect();
        by_key.sort_unstable();
        assert_ne!(
            by_key,
            found
                .iter()
                .filter_map(Derivation::subject)
                .collect::<Vec<_>>(),
            "the fixture must distinguish the two orders"
        );
    }

    #[test]
    fn r1629_nothing_omitted_publishes_nothing() {
        assert!(omitted_counts(name::OFF_SCALE, []).is_empty());
    }
}

/// R1629 — the derivations every chart in this crate publishes, asserted on
/// the **built scene** rather than on the builder.
///
/// The scene is the layer that matters: a report that answers in-process and
/// does not reach `Scene` is exactly the state this round found, so a test
/// that called `chart.derivations()` directly would pass over the defect it
/// exists to prevent.
#[cfg(test)]
mod chart_tests {
    use super::{domain, name, unit};
    use pinion_core::Scene;
    use pinion_core::derivation::{DerivationKind, DerivationLookup, DerivationSet, Evidence};
    use pinion_core::scene::Rect;

    use crate::candle::Candle;
    use crate::density::DensitySpec;
    use crate::distribution::{Distribution, QuantileMethod};
    use crate::interpolate::Interpolation;
    use crate::polar::AngularScale;
    use crate::series::{DataPoint, Series};
    use crate::style::ChartStyle;
    use crate::{
        Bar, BarChart, BoxPlotChart, CandlestickChart, DistributionMark, DonutChart, Kernel, Lane,
        LineChart, PolarChart, ScatterChart, SessionMark, Slice, Span, Sparkline, Tile, Timeline,
        Treemap,
    };

    const RECT: Rect = Rect::new(0, 0, 640, 360);

    fn published(scene: &Scene) -> DerivationSet {
        match scene.derivations_for_tag("chart") {
            DerivationLookup::Published(set) => set.clone(),
            other => panic!("the chart root did not publish: {other:?}"),
        }
    }

    /// Every entry of `kind` named `name`, as `(subject, evidence)`.
    fn entries(
        set: &DerivationSet,
        kind: DerivationKind,
        name: &str,
    ) -> Vec<(Option<String>, Evidence)> {
        set.of_kind(kind)
            .filter(|d| d.name() == name)
            .map(|d| (d.subject().map(ToOwned::to_owned), d.evidence().clone()))
            .collect()
    }

    /// A plateau and then a jump — the case every spline overshoots, and the
    /// one R1625's own report was written against.
    fn plateau() -> Vec<Series> {
        vec![Series::new(
            "s",
            [0.0, 0.0, 0.0, 0.0, 10.0, 10.0]
                .into_iter()
                .enumerate()
                .map(|(i, y)| DataPoint::new(f64::from(u8::try_from(i).expect("six points")), y))
                .collect(),
        )]
    }

    #[test]
    fn r1629_a_spline_names_on_the_scene_where_it_left_the_data() {
        let style = ChartStyle::default();
        let smooth = LineChart::new(plateau())
            .interpolation(Interpolation::CatmullRom)
            .build(RECT, &style);
        let set = published(&smooth);

        // The choice, always.
        assert_eq!(
            entries(&set, DerivationKind::Chosen, name::INTERPOLATION),
            vec![(None, Evidence::Name("catmull-rom".into()))]
        );
        // The excursion: one entry for the series, localized to the gap that
        // made it, measured in the value axis' own units.
        let shoots = set
            .of_kind(DerivationKind::Invented)
            .filter(|d| d.name() == name::OVERSHOOT)
            .collect::<Vec<_>>();
        assert_eq!(shoots.len(), 1, "one entry per series, not per segment");
        assert_eq!(shoots[0].subject(), Some("series.0"));
        assert_eq!(shoots[0].unit(), Some(unit::VALUE));
        let (start, end) = shoots[0].span().expect("localized to its gap");
        assert_eq!(end - start, 2, "a gap covers both of its samples");
        assert!(
            shoots[0]
                .evidence()
                .quantity()
                .is_some_and(|beyond| beyond > 0.0),
            "an excursion has a size"
        );
        assert_eq!(set.domain(), domain::SAMPLE, "the span's index space");
        // ...and how many gaps did it, which the localized entry cannot say.
        let counted = entries(&set, DerivationKind::Invented, name::OVERSHOOT_SEGMENTS);
        assert_eq!(counted.len(), 1);
        assert!(matches!(counted[0].1, Evidence::Count(n) if n >= 1));

        // ★ The counterfactual: the monotone interpolant draws the same data
        // and invents nothing, so the picture-vs-data row is EMPTY while the
        // choice is still published. A `Chosen` entry that only appeared when
        // something went wrong would be unreadable as a control.
        let safe = LineChart::new(plateau())
            .interpolation(Interpolation::Monotone)
            .build(RECT, &style);
        let safe_set = published(&safe);
        assert_eq!(
            safe_set.of_kind(DerivationKind::Invented).count(),
            0,
            "a monotone curve stays inside its samples"
        );
        assert_eq!(
            entries(&safe_set, DerivationKind::Chosen, name::INTERPOLATION),
            vec![(None, Evidence::Name("monotone".into()))]
        );
    }

    #[test]
    fn r1629_the_reported_excursion_is_the_worst_one() {
        // ★ Found by a counterfactual: inverting the max to a min left the
        // whole suite green. One entry per series is only defensible if the
        // entry is the excursion a reader must be warned about, and nothing
        // held it to that — `beyond > 0.0` is true of the smallest one too.
        //
        // The oracle is the in-process report, which is an independent
        // statement (it enumerates every excursion and this publishes one), so
        // the test is not a restatement of the code under it.
        let style = ChartStyle::default();
        // Two plateau-then-rise features of very different heights, so the
        // series overshoots twice by different amounts.
        let ys = [0.0_f64, 0.0, 0.0, 30.0, 30.0, 30.0, 31.0, 31.0, 31.0];
        let chart = LineChart::new(vec![Series::new(
            "s",
            ys.iter()
                .enumerate()
                .map(|(i, &y)| DataPoint::new(f64::from(u8::try_from(i).expect("nine points")), y))
                .collect(),
        )])
        .interpolation(Interpolation::CatmullRom);

        let reported: Vec<f64> = chart
            .overshoot()
            .into_iter()
            .filter(|(series, _)| *series == 0)
            .map(|(_, shoot)| f64::from(shoot.beyond))
            .collect();
        assert!(
            reported.len() >= 2,
            "the fixture must overshoot more than once: {reported:?}"
        );
        let worst = reported.iter().copied().fold(f64::MIN, f64::max);
        let least = reported.iter().copied().fold(f64::MAX, f64::min);
        assert!(
            worst > least,
            "and by different amounts, or the assertion below is vacuous: {reported:?}"
        );

        let set = published(&chart.build(RECT, &style));
        let published_beyond = set
            .named(name::OVERSHOOT)
            .next()
            .and_then(|d| d.evidence().quantity())
            .expect("the series published its excursion");
        assert!(
            (published_beyond - worst).abs() < 1e-6,
            "published {published_beyond}, worst {worst}, all {reported:?}"
        );
        // ...and the tally still counts every one of them, which is the fact
        // the single localized entry gives up.
        assert_eq!(
            entries(&set, DerivationKind::Invented, name::OVERSHOOT_SEGMENTS),
            vec![(Some("series.0".into()), Evidence::Count(reported.len()))]
        );
    }

    #[test]
    fn r1629_a_log_axis_counts_per_series_what_it_could_not_place() {
        let style = ChartStyle::default();
        let chart = LineChart::new(vec![
            Series::new(
                "positive",
                vec![DataPoint::new(1.0, 1.0), DataPoint::new(2.0, 10.0)],
            ),
            Series::new(
                "mixed",
                vec![
                    DataPoint::new(1.0, -1.0),
                    DataPoint::new(2.0, 0.0),
                    DataPoint::new(3.0, 5.0),
                ],
            ),
        ])
        .y_log();
        let set = published(&chart.build(RECT, &style));
        let off = entries(&set, DerivationKind::Omitted, name::OFF_SCALE);
        assert_eq!(off.len(), 1, "only the series with unplaceable data");
        assert_eq!(off[0].0.as_deref(), Some("series.1"));
        assert_eq!(
            off[0].1,
            Evidence::Count(2),
            "the non-positive samples, counted rather than enumerated"
        );
        // The count agrees with the in-process report it was built from, so
        // the wire cannot drift from the accessor.
        assert_eq!(chart.off_scale().len(), 2);
    }

    #[test]
    fn r1629_a_violin_publishes_the_four_choices_behind_its_outline() {
        let style = ChartStyle::default();
        let samples: Vec<f64> = (0..40).map(|i| f64::from(i) * 0.5).collect();
        let chart = BoxPlotChart::new(vec![
            Distribution::from_samples_with_density(
                "a",
                &samples,
                QuantileMethod::Tukey,
                DensitySpec::default(),
            )
            .expect("forty finite samples"),
        ])
        .with_mark(DistributionMark::Violin);
        let set = published(&chart.build(RECT, &style));

        assert_eq!(
            entries(&set, DerivationKind::Chosen, name::KERNEL),
            vec![(Some("slot.0".into()), Evidence::Name("gaussian".into()))]
        );
        let bandwidth = entries(&set, DerivationKind::Chosen, name::BANDWIDTH);
        assert_eq!(bandwidth.len(), 1);
        assert!(bandwidth[0].1.quantity().is_some_and(|b| b > 0.0));
        assert_eq!(
            set.named(name::BANDWIDTH)
                .next()
                .and_then(|d| d.unit().map(ToOwned::to_owned)),
            Some(unit::VALUE.to_owned()),
            "a bandwidth without units is a number nobody can read"
        );
        assert_eq!(
            entries(&set, DerivationKind::Chosen, name::BOUNDED),
            vec![(Some("slot.0".into()), Evidence::Flag(false))]
        );
        assert_eq!(
            entries(&set, DerivationKind::Chosen, name::BANDWIDTH_RULE).len(),
            1
        );
        // The measurements the outline replaced.
        assert_eq!(
            entries(&set, DerivationKind::Omitted, name::SAMPLES),
            vec![(Some("slot.0".into()), Evidence::Count(40))]
        );
        // An unbounded Gaussian always reaches past the data, and the share is
        // published as an invented quantity in fractions.
        let spill = set
            .named(name::SPILL)
            .next()
            .expect("an unbounded estimate spills");
        assert_eq!(spill.kind(), DerivationKind::Invented);
        assert_eq!(spill.unit(), Some(unit::FRACTION));
        assert!(spill.evidence().quantity().is_some_and(|s| s > 0.0));
    }

    #[test]
    fn r1629_a_bounded_estimate_invents_nothing_and_the_report_is_absent() {
        // ★ The rule this round rests on: an `Invented` entry exists exactly
        // when the picture shows something the data does not. Publishing
        // `spill = 0` would make "did this chart invent anything?" answer yes
        // for every violin ever drawn, which is the answer that cannot be
        // acted on.
        let style = ChartStyle::default();
        let samples: Vec<f64> = (0..40).map(|i| f64::from(i) * 0.5).collect();
        let chart = BoxPlotChart::new(vec![
            Distribution::from_samples_with_density(
                "a",
                &samples,
                QuantileMethod::Tukey,
                DensitySpec::new(Kernel::Gaussian, crate::Bandwidth::default()).bounded(),
            )
            .expect("forty finite samples"),
        ])
        .with_mark(DistributionMark::Violin);
        let set = published(&chart.build(RECT, &style));
        assert_eq!(set.named(name::SPILL).count(), 0, "nothing was invented");
        assert_eq!(
            set.of_kind(DerivationKind::Invented).count(),
            0,
            "and the whole row is empty"
        );
        // ...and the reason is still on the wire, as a choice.
        assert_eq!(
            entries(&set, DerivationKind::Chosen, name::BOUNDED),
            vec![(Some("slot.0".into()), Evidence::Flag(true))]
        );
    }

    #[test]
    fn r1629_a_violin_that_cannot_be_estimated_is_discarded_only_when_asked_for() {
        let style = ChartStyle::default();
        let summary = || {
            Distribution::from_summary("s", 1.0, 2.0, 3.0, 4.0, 5.0).expect("ordered five numbers")
        };
        let asked = BoxPlotChart::new(vec![summary()]).with_mark(DistributionMark::Violin);
        let set = published(&asked.build(RECT, &style));
        assert_eq!(
            entries(&set, DerivationKind::Discarded, name::DENSITY),
            vec![(Some("slot.0".into()), Evidence::Name("summary".into()))],
            "a violin was asked for and the data could not support it"
        );

        // ★ The counterfactual: under a box mark nothing was requested, so
        // nothing was discarded. A report keyed off the DATA alone — which is
        // what `without_density` is — would fire here and tell a caller its
        // code was wrong when it was not.
        let unasked = BoxPlotChart::new(vec![summary()]).with_mark(DistributionMark::Box);
        let unasked_set = published(&unasked.build(RECT, &style));
        assert_eq!(unasked_set.of_kind(DerivationKind::Discarded).count(), 0);
        assert_eq!(
            unasked.without_density(),
            vec![0],
            "while the data-only report still names it"
        );
    }

    fn week() -> Vec<Candle> {
        (0..4)
            .map(|i| {
                let base = f64::from(i);
                Candle::new(base, 10.0 + base, 12.0 + base, 9.0 + base, 11.0 + base)
                    .expect("ordered prices")
            })
            .collect()
    }

    #[test]
    fn r1629_caps_asked_for_under_a_bar_are_reported_rather_than_dropped() {
        // The debt this closes: `with_caps(true).with_mark(Ohlc)` added no
        // node and said nothing, so the option read as broken. The bar's own
        // open and close ticks already play the caps' role, so the answer is
        // not to draw something — it is to say the setting was discarded, and
        // to name the mark that made it meaningless.
        let style = ChartStyle::default();
        let set = published(
            &CandlestickChart::new(week())
                .with_caps(true)
                .with_mark(SessionMark::Ohlc)
                .build(RECT, &style),
        );
        assert_eq!(
            entries(&set, DerivationKind::Discarded, name::CAPS),
            vec![(Some(name::MARK.into()), Evidence::Name("ohlc".into()))]
        );
        assert_eq!(
            entries(&set, DerivationKind::Chosen, name::MARK),
            vec![(None, Evidence::Name("ohlc".into()))]
        );

        // ★ Three counterfactuals, because three things could make this fire
        // wrongly: caps under the mark that HAS caps, no caps under the bar,
        // and the default chart.
        for (caps, mark) in [
            (true, SessionMark::Candle),
            (false, SessionMark::Ohlc),
            (false, SessionMark::Candle),
        ] {
            let other = published(
                &CandlestickChart::new(week())
                    .with_caps(caps)
                    .with_mark(mark)
                    .build(RECT, &style),
            );
            assert_eq!(
                other.of_kind(DerivationKind::Discarded).count(),
                0,
                "caps = {caps} under {mark:?} discards nothing"
            );
        }
    }

    #[test]
    fn r1629_a_wrapped_bearing_is_a_position_the_datum_never_took() {
        let style = ChartStyle::default();
        let chart = PolarChart::new(
            vec![Series::new(
                "s",
                vec![DataPoint::new(370.0, 1.0), DataPoint::new(10.0, 2.0)],
            )],
            AngularScale::new((0.0, 360.0)),
        );
        let set = published(&chart.build(RECT, &style));
        assert_eq!(
            entries(&set, DerivationKind::Invented, name::WRAPPED),
            vec![(Some("series.0".into()), Evidence::Count(1))],
            "the mark is real and its bearing is not the datum's"
        );
    }

    /// R1629 — every chart in this crate answers, and the answer is
    /// `Published`. A chart that fell through to
    /// [`Silent`](DerivationLookup::Silent) would be indistinguishable from
    /// one that has nothing to say, which is the distinction the channel
    /// exists for.
    ///
    /// The list is what `crate::derivations::chart_root` makes unnecessary to
    /// keep in sync by hand — every builder here reaches the scene through it,
    /// so a chart added later cannot compile without deciding. This test is
    /// the behavioural half of that claim.
    #[test]
    fn r1629_every_chart_publishes_rather_than_falling_silent() {
        let style = ChartStyle::default();
        let series = || vec![Series::new("s", vec![DataPoint::new(1.0, 1.0)])];
        let built: Vec<(&str, Scene)> = vec![
            ("line", LineChart::new(series()).build(RECT, &style)),
            ("scatter", ScatterChart::new(series()).build(RECT, &style)),
            (
                "polar",
                PolarChart::new(series(), AngularScale::new((0.0, 360.0))).build(RECT, &style),
            ),
            (
                "candlestick",
                CandlestickChart::new(week()).build(RECT, &style),
            ),
            (
                "boxplot",
                BoxPlotChart::new(vec![
                    Distribution::from_summary("s", 1.0, 2.0, 3.0, 4.0, 5.0).expect("ordered"),
                ])
                .build(RECT, &style),
            ),
            (
                "bar",
                BarChart::new(vec![Bar::new("a", 1.0)]).build(RECT, &style),
            ),
            (
                "donut",
                DonutChart::new(vec![Slice::new("a", 1.0)]).build(RECT, &style),
            ),
            (
                "sparkline",
                // Its own default prefix is `spark`; normalized so the loop
                // asks every chart at one address.
                Sparkline::new(vec![1.0, 2.0])
                    .with_tag_prefix("chart")
                    .build(RECT, &style),
            ),
            (
                "timeline",
                // Its own default prefix is `timeline`, normalized for the
                // same reason the sparkline's is.
                Timeline::new(vec![Lane::new("l", vec![Span::new(0.0, 1.0, "a")])])
                    .with_tag_prefix("chart")
                    .build(RECT, &style),
            ),
            (
                "treemap",
                Treemap::new(vec![Tile::new("a", 1.0)]).build(RECT, &style),
            ),
        ];
        assert_eq!(built.len(), 10, "every chart builder this crate exports");
        for (who, scene) in built {
            match scene.derivations_for_tag("chart") {
                DerivationLookup::Published(set) => {
                    assert!(
                        !set.domain().is_empty(),
                        "{who} publishes without saying what a span would index"
                    );
                }
                other => panic!("{who} answered {other:?} instead of publishing"),
            }
        }
    }

    #[test]
    fn r1629_the_unmeasured_sentinel_has_composed_nothing_yet() {
        // `build_fill(0, 0)` is the bootstrap frame: the slot has not been
        // measured, so no drawing exists to make a statement about. Pinned by
        // a test rather than left to fall out of the code, because a client
        // polling during bootstrap sees this answer.
        let style = ChartStyle::default();
        let sentinel = LineChart::new(plateau()).build_fill((0, 0), &style);
        assert_eq!(
            sentinel.derivations_for_tag("chart"),
            DerivationLookup::Silent
        );
    }

    #[test]
    fn r1629_a_chart_is_the_only_node_of_its_tree_that_answers() {
        // The channel is on the composition, so the paths and boxes a chart
        // emits say `NoChannel` rather than `Silent` — a client that walked
        // into the tree looking for the report is told it is looking at the
        // wrong kind of node, not that the chart forgot.
        use pinion_core::derivation::DerivesChannel;
        let style = ChartStyle::default();
        let scene = LineChart::new(plateau())
            .interpolation(Interpolation::CatmullRom)
            .build(RECT, &style);
        assert!(matches!(
            scene.derivations_for_tag("chart.series.0"),
            DerivationLookup::NoChannel(DerivesChannel::Painted)
        ));
        assert_eq!(
            scene.derivations_for_tag("chart.nobody"),
            DerivationLookup::NoSuchTag
        );
    }
}
