//! R1824 — **every chart kind that carries marks can dim the ones a
//! cross-filter selection does not cover**, proved per kind against the scene
//! each one produces.
//!
//! # What this file is answering
//!
//! Measured before it existed, by building every kind this crate ships and
//! reading the fill alphas of its marks: three of ten kinds dimmed anything
//! (`BarChart`, `LineChart`, `ScatterChart`, each with its own hand-written
//! selection field), and seven had no way to be told about a selection at all.
//! A board could declare a ring chart as a participating view, publish a
//! selection, receive a `Reach` naming it as reached, and paint it entirely
//! unchanged.
//!
//! # Why the assertions are what they are
//!
//! A test that only checked the setter would pass against a chart that stores
//! the selection and ignores it — which is the exact defect. So every case
//! here **builds the scene twice**, once with no selection and once with one,
//! and compares the *painted* marks by tag: the mark the selection excludes
//! must come out fainter, and the mark it covers must come out byte-identical
//! to its unselected self. Both directions matter — a change that dimmed
//! everything would pass the first assertion alone.
//!
//! This is an integration test on purpose: it may use only the crate's public
//! API, which is the standing rule for a verdict's `proven_by` (R1602). If a
//! kind could only be muted through something `pub(crate)`, it could not be
//! muted by a consumer, and this file would not compile.

use std::collections::BTreeSet;

use pinion_chart::{
    AngularScale, Bar, BarChart, BoxPlotChart, Candle, CandlestickChart, ChartStyle, DataPoint,
    Distribution, Domain, DonutChart, Lane, LineChart, Mute, PolarChart, QuantileMethod,
    ScatterChart, Selection, Series, Slice, Span, Sparkline, Tile, Timeline, Treemap,
};
use pinion_core::Scene;
use pinion_core::scene::Rect;

const RECT: Rect = Rect::new(0, 0, 640, 400);

// --- reading the produced scene ---------------------------------------------

/// The alpha the node tagged `tag` paints its mark with: its box fill, or —
/// for a path — the **strongest** of its fill and its stroke.
///
/// The strongest, not the first present, and the reason is a real chart. A
/// rising candlestick body is drawn HOLLOW: a fill at alpha zero with a
/// stroked outline, so the ink a reader sees is entirely the stroke. Reading
/// the fill because it happens to exist would report `0` before and `0` after
/// any dimming, and the assertion would have been vacuous rather than false —
/// the failure mode that is worst to have in a proof.
///
/// `None` when nothing carries the tag, which every caller here treats as a
/// failure: a mark that vanished under a selection has not been *muted*, and
/// the difference is the whole point of dimming rather than hiding.
fn mark_alpha(scene: &Scene, tag: &str) -> Option<u8> {
    fn walk(scene: &Scene, tag: &str) -> Option<u8> {
        if scene.tag() == Some(tag) {
            return match scene {
                Scene::Box(b) => Some(b.style.fill.a),
                Scene::Path(p) => {
                    let fill = p.style.fill.map(|c| c.a);
                    let stroke = p.style.stroke.as_ref().map(|s| s.color.a);
                    match (fill, stroke) {
                        (Some(f), Some(s)) => Some(f.max(s)),
                        (some, None) | (None, some) => some,
                    }
                }
                _ => None,
            };
        }
        match scene {
            Scene::Container(c) => c.children.iter().find_map(|ch| walk(ch, tag)),
            _ => None,
        }
    }
    walk(scene, tag)
}

fn alpha(scene: &Scene, tag: &str) -> u8 {
    mark_alpha(scene, tag).unwrap_or_else(|| panic!("`{tag}` is a painted mark"))
}

/// The core assertion, applied identically to every kind.
///
/// `dim` names marks the selection excludes and `lit` marks it covers. Both
/// lists are required to be non-empty by the caller's own construction — a case
/// with nothing lit proves only that a chart can be greyed out wholesale.
fn assert_muted(kind: &str, full: &Scene, filtered: &Scene, dim: &[&str], lit: &[&str]) {
    assert!(
        !dim.is_empty() && !lit.is_empty(),
        "{kind}: a proof needs both a mark that dims and one that does not"
    );
    for tag in dim {
        let (before, after) = (alpha(full, tag), alpha(filtered, tag));
        assert!(
            after < before,
            "{kind}: `{tag}` is outside the selection and must draw fainter, \
             but its alpha went {before} -> {after}"
        );
    }
    for tag in lit {
        let (before, after) = (alpha(full, tag), alpha(filtered, tag));
        assert_eq!(
            after, before,
            "{kind}: `{tag}` is inside the selection and must be untouched"
        );
    }
}

// --- the fixtures, one per kind ---------------------------------------------

fn bars() -> Vec<Bar> {
    vec![
        Bar::new("alpha", 10.0),
        Bar::new("beta", 20.0),
        Bar::new("gamma", 30.0),
    ]
}

fn two_series() -> Vec<Series> {
    vec![
        Series::new(
            "a",
            (0..6)
                .map(|i| DataPoint::new(f64::from(i), f64::from(i % 4)))
                .collect(),
        ),
        Series::new(
            "b",
            (0..6)
                .map(|i| DataPoint::new(f64::from(i), f64::from((i + 2) % 5)))
                .collect(),
        ),
    ]
}

fn slices() -> Vec<Slice> {
    vec![
        Slice::new("a", 30.0),
        Slice::new("b", 20.0),
        Slice::new("c", 50.0),
    ]
}

fn tiles() -> Vec<Tile> {
    vec![
        Tile::new("a", 40.0),
        Tile::new("b", 30.0),
        Tile::new("c", 20.0),
    ]
}

fn lanes() -> Vec<Lane> {
    vec![
        Lane::new(
            "l0",
            vec![Span::new(0.0, 10.0, "x"), Span::new(10.0, 20.0, "y")],
        ),
        Lane::new("l1", vec![Span::new(2.0, 18.0, "z")]),
    ]
}

fn distribution(label: &str, base: f64) -> Distribution {
    let samples: Vec<f64> = (0..40).map(|i| base + f64::from(i % 11)).collect();
    Distribution::from_samples(label, &samples, QuantileMethod::default())
        .expect("the fixture holds finite samples")
}

fn candles() -> Vec<Candle> {
    (0..5)
        .map(|i| {
            let base = f64::from(i);
            Candle::new(base, base + 1.0, base + 3.0, base, base + 2.0)
                .expect("the fixture's prices are ordered")
        })
        .collect()
}

fn rose() -> PolarChart {
    PolarChart::new(
        vec![Series::new(
            "w",
            vec![
                DataPoint::new(0.0, 4.0),
                DataPoint::new(90.0, 9.0),
                DataPoint::new(180.0, 2.0),
                DataPoint::new(270.0, 6.0),
            ],
        )],
        AngularScale::new((0.0, 360.0)),
    )
}

/// Apply a selection through the ONE API, insisting the kind accepts it —
/// generic over the kind, because that is the whole claim: the same call
/// reaches all ten.
fn muted_by<T: Mute>(chart: T, selection: &Selection) -> T {
    chart
        .try_muted_by(selection)
        .unwrap_or_else(|why| panic!("the fixture's marks answer this domain: {why}"))
}

// --- the ten proofs ---------------------------------------------------------

#[test]
fn bar_chart_dims_the_bars_the_selection_excludes() {
    let style = ChartStyle::default();
    let full = BarChart::new(bars()).build(RECT, &style);
    let chart = muted_by(BarChart::new(bars()), &Selection::Category("beta".into()));
    assert_eq!(chart.muted().dimmed(), 2, "two of three bars are excluded");
    let filtered = chart.build(RECT, &style);
    assert_muted(
        "BarChart",
        &full,
        &filtered,
        &["chart.bar.0", "chart.bar.2"],
        &["chart.bar.1"],
    );
}

#[test]
fn line_chart_dims_the_series_the_selection_excludes() {
    let style = ChartStyle::default();
    let full = LineChart::new(two_series()).build(RECT, &style);
    let chart = muted_by(
        LineChart::new(two_series()),
        &Selection::Category("b".into()),
    );
    assert_eq!(chart.muted().dimmed(), 1);
    let filtered = chart.build(RECT, &style);
    assert_muted(
        "LineChart",
        &full,
        &filtered,
        &["chart.series.0"],
        &["chart.series.1"],
    );
}

#[test]
fn scatter_chart_dims_the_series_the_selection_excludes() {
    let style = ChartStyle::default();
    let full = ScatterChart::new(two_series()).build(RECT, &style);
    let chart = muted_by(
        ScatterChart::new(two_series()),
        &Selection::Category("b".into()),
    );
    assert_eq!(chart.muted().dimmed(), 1);
    let filtered = chart.build(RECT, &style);
    assert_muted(
        "ScatterChart",
        &full,
        &filtered,
        &["chart.point.0.0", "chart.point.0.3"],
        &["chart.point.1.0", "chart.point.1.3"],
    );
}

/// The first of the two `Sector` proofs. Slices of 30 / 20 / 50 sweep
/// `0..0.6pi`, `0.6pi..1.0pi` and `1.0pi..2pi`, so a sector one radian wide
/// from zero falls inside the first alone.
#[test]
fn donut_chart_dims_the_slices_outside_the_sector() {
    let style = ChartStyle::default();
    let full = DonutChart::new(slices()).build(RECT, &style);
    let chart = muted_by(
        DonutChart::new(slices()),
        &Selection::Sector {
            angle: (0.0, 1.0),
            radius: (0.0, 1.0),
        },
    );
    assert_eq!(chart.muted().dimmed(), 2);
    let filtered = chart.build(RECT, &style);
    assert_muted(
        "DonutChart",
        &full,
        &filtered,
        &["chart.slice.1", "chart.slice.2"],
        &["chart.slice.0"],
    );
}

#[test]
fn donut_chart_also_answers_a_category() {
    let style = ChartStyle::default();
    let full = DonutChart::new(slices()).build(RECT, &style);
    let filtered =
        muted_by(DonutChart::new(slices()), &Selection::Category("b".into())).build(RECT, &style);
    assert_muted(
        "DonutChart/category",
        &full,
        &filtered,
        &["chart.slice.0", "chart.slice.2"],
        &["chart.slice.1"],
    );
}

#[test]
fn treemap_dims_the_tiles_the_selection_excludes() {
    let style = ChartStyle::default();
    let full = Treemap::new(tiles()).build(RECT, &style);
    let chart = muted_by(Treemap::new(tiles()), &Selection::Category("b".into()));
    assert_eq!(chart.muted().dimmed(), 2);
    let filtered = chart.build(RECT, &style);
    // Tiles are tagged in DRAW order (descending value), so `b` (30) is tile 1.
    assert_muted(
        "Treemap",
        &full,
        &filtered,
        &["chart.tile.0", "chart.tile.2"],
        &["chart.tile.1"],
    );
}

/// The sparkline proves BOTH of its units in one case: a reference dot is one
/// sample and dims on its own verdict, while the trend stroke spans every
/// sample and stays lit because the window keeps two of them.
#[test]
fn sparkline_dims_the_dot_outside_the_window_and_keeps_the_run_that_reaches_it() {
    let style = ChartStyle::default();
    let values = vec![1.0, 5.0, 3.0, 9.0, 4.0];
    let full = Sparkline::new(values.clone())
        .with_markers(true)
        .build(RECT, &style);
    let chart = muted_by(
        Sparkline::new(values).with_markers(true),
        &Selection::XRange { lo: 0.0, hi: 2.0 },
    );
    assert_eq!(chart.muted().dimmed(), 3, "indices 2, 3 and 4 are outside");
    let filtered = chart.build(RECT, &style);
    // `spark.end` is the last sample (index 4, outside); `spark.max` is index 3
    // (outside); `spark.min` is index 0 (inside).
    assert_muted(
        "Sparkline",
        &full,
        &filtered,
        &["spark.end", "spark.max"],
        &["spark.min", "spark.line"],
    );
}

/// A named sparkline answers the domain a saved filter publishes: when the
/// trend on screen is not the one selected, the whole run reads as context.
#[test]
fn a_named_sparkline_dims_whole_when_the_selection_names_another_trend() {
    let style = ChartStyle::default();
    let values = vec![1.0, 5.0, 3.0, 9.0, 4.0];
    let named = || Sparkline::new(values.clone()).labelled("matched");
    assert!(
        named().mute_accepts(Domain::Category),
        "a named trend can be asked whether it is the selected one"
    );
    assert!(
        !Sparkline::new(values.clone()).mute_accepts(Domain::Category),
        "and an unnamed one cannot, rather than claiming an identity it lacks"
    );
    let full = named().build(RECT, &style);
    let filtered =
        muted_by(named(), &Selection::Category("shared memory".into())).build(RECT, &style);
    assert!(
        alpha(&filtered, "spark.line") < alpha(&full, "spark.line"),
        "a trend of the whole population is not a trend of the selection"
    );
    let kept = muted_by(named(), &Selection::Category("matched".into())).build(RECT, &style);
    assert_eq!(
        alpha(&kept, "spark.line"),
        alpha(&full, "spark.line"),
        "and the trend that IS the selection is untouched"
    );
}

#[test]
fn sparkline_dims_its_whole_run_when_the_window_reaches_no_sample() {
    let style = ChartStyle::default();
    let values = vec![1.0, 5.0, 3.0, 9.0, 4.0];
    let full = Sparkline::new(values.clone()).build(RECT, &style);
    let filtered = muted_by(
        Sparkline::new(values),
        &Selection::XRange { lo: 90.0, hi: 99.0 },
    )
    .build(RECT, &style);
    assert!(
        alpha(&filtered, "spark.line") < alpha(&full, "spark.line"),
        "a run with no sample inside the window is context, and draws as context"
    );
}

/// The `LaneWindow` proof — the domain's only consumer, and the reason it is
/// one domain rather than two.
#[test]
fn timeline_dims_the_spans_outside_the_selected_lane() {
    let style = ChartStyle::default();
    let full = Timeline::new(lanes()).build(RECT, &style);
    let chart = muted_by(
        Timeline::new(lanes()),
        &Selection::LaneWindow {
            lane: "l1".into(),
            window: (0.0, 100.0),
        },
    );
    assert_eq!(
        chart.muted().dimmed(),
        2,
        "the window covers every span, and the lane covers one"
    );
    let filtered = chart.build(RECT, &style);
    assert_muted(
        "Timeline",
        &full,
        &filtered,
        &["timeline.lane.0.span.0", "timeline.lane.0.span.1"],
        &["timeline.lane.1.span.0"],
    );
}

#[test]
fn box_plot_dims_the_distributions_the_selection_excludes() {
    let style = ChartStyle::default();
    let dists = || {
        vec![
            distribution("a", 0.0),
            distribution("b", 5.0),
            distribution("c", 9.0),
        ]
    };
    let full = BoxPlotChart::new(dists()).build(RECT, &style);
    let chart = muted_by(BoxPlotChart::new(dists()), &Selection::Category("b".into()));
    assert_eq!(chart.muted().dimmed(), 2);
    let filtered = chart.build(RECT, &style);
    assert_muted(
        "BoxPlotChart",
        &full,
        &filtered,
        &["chart.box.0", "chart.box.2"],
        &["chart.box.1"],
    );
}

#[test]
fn candlestick_dims_the_sessions_outside_the_time_window() {
    let style = ChartStyle::default();
    let full = CandlestickChart::new(candles()).build(RECT, &style);
    let chart = muted_by(
        CandlestickChart::new(candles()),
        &Selection::XRange { lo: 2.0, hi: 10.0 },
    );
    assert_eq!(chart.muted().dimmed(), 2, "the sessions at 0 and 1");
    let filtered = chart.build(RECT, &style);
    assert_muted(
        "CandlestickChart",
        &full,
        &filtered,
        &["chart.candle.0", "chart.candle.1"],
        &["chart.candle.2", "chart.candle.4"],
    );
}

/// The second `Sector` proof, and the one that shows why the mark had to be the
/// SAMPLE: a rose's series spans the whole turn, so a per-series test would
/// find it inside every sector and dim nothing.
#[test]
fn polar_chart_dims_the_samples_outside_the_sector() {
    let style = ChartStyle::default();
    let full = rose().build(RECT, &style);
    let chart = muted_by(
        rose(),
        &Selection::Sector {
            angle: (80.0, 100.0),
            radius: (0.0, 100.0),
        },
    );
    assert_eq!(
        chart.muted().dimmed(),
        3,
        "only the bearing at 90 is inside"
    );
    let filtered = chart.build(RECT, &style);
    assert_muted(
        "PolarChart",
        &full,
        &filtered,
        &["chart.point.0.0", "chart.point.0.2", "chart.point.0.3"],
        &["chart.point.0.1", "chart.series.0"],
    );
}

// --- the properties that hold across every kind -----------------------------

/// Every kind this crate ships that carries marks, as trait objects.
///
/// The list is what makes the assertions below a statement about the CRATE
/// rather than about ten separate charts, and adding an eleventh kind without
/// adding it here is the omission the module was written to make impossible to
/// leave silent — so `every_domain_of_the_vocabulary_has_a_consumer` reads the
/// domains off this list rather than off a written-down set.
fn every_kind() -> Vec<(&'static str, Box<dyn Mute>)> {
    vec![
        ("BarChart", Box::new(BarChart::new(bars()))),
        ("LineChart", Box::new(LineChart::new(two_series()))),
        ("ScatterChart", Box::new(ScatterChart::new(two_series()))),
        ("DonutChart", Box::new(DonutChart::new(slices()))),
        ("Treemap", Box::new(Treemap::new(tiles()))),
        (
            "Sparkline",
            Box::new(Sparkline::new(vec![1.0, 5.0, 3.0, 9.0, 4.0])),
        ),
        ("Timeline", Box::new(Timeline::new(lanes()))),
        (
            "BoxPlotChart",
            Box::new(BoxPlotChart::new(vec![
                distribution("a", 0.0),
                distribution("b", 5.0),
            ])),
        ),
        (
            "CandlestickChart",
            Box::new(CandlestickChart::new(candles())),
        ),
        ("PolarChart", Box::new(rose())),
    ]
}

#[test]
fn every_kind_carries_marks_and_answers_at_least_one_domain() {
    for (name, kind) in every_kind() {
        assert!(
            !kind.mark_keys().is_empty(),
            "{name} draws marks, so it must be able to name them"
        );
        assert!(
            !kind.mute_domains().is_empty(),
            "{name} must be reachable by some selection; a kind that answers \
             nothing is a view that silently never narrows"
        );
    }
}

/// The half of the debt that was about the vocabulary rather than the marks:
/// `Domain::Sector` and `Domain::LaneWindow` existed as values with no
/// consumer at all.
#[test]
fn every_domain_of_the_vocabulary_has_a_consumer() {
    let mut answered: BTreeSet<Domain> = BTreeSet::new();
    for (_, kind) in every_kind() {
        answered.extend(kind.mute_domains());
    }
    for domain in Domain::ALL {
        assert!(
            answered.contains(&domain),
            "`{domain}` is in the selection vocabulary and no chart kind can be \
             tested by it — a declared domain with no implementation"
        );
    }
}

#[test]
fn a_kind_refuses_a_domain_its_marks_cannot_answer_instead_of_muting_nothing() {
    // A treemap's marks carry a name and no numeric position: an area has no
    // axis a window could be stated on.
    let mut treemap = Treemap::new(tiles());
    let why = treemap
        .mute(Some(&Selection::XRange { lo: 0.0, hi: 1.0 }))
        .expect_err("a treemap has no numeric axis");
    let sentence = why.to_string();
    assert!(
        sentence.contains("category") && sentence.contains("x-range"),
        "a refusal names both sides, so `it did not narrow` is not the diagnosis: {sentence}"
    );
    assert!(
        treemap.muted().is_clear(),
        "and nothing was dimmed by the attempt"
    );
}

/// The two halves finally joined: a chart's own `Link` is derived from its
/// marks, so a board's declaration cannot claim a domain the drawing cannot
/// answer.
#[test]
fn a_charts_link_declaration_comes_from_its_marks() {
    let timeline = Timeline::new(lanes());
    let link = timeline.link("track");
    assert!(link.accepts(Domain::LaneWindow), "a timeline has lanes");
    assert!(!link.accepts(Domain::Sector), "and no angular geometry");

    let empty = Timeline::new(Vec::new());
    assert!(
        empty.link("track").inert_reason().is_some(),
        "a chart with nothing drawn declares itself inert with a reason, \
         rather than as a view that accepts an empty set of domains"
    );
}

/// The board-level entry point: a view applies what the group's `Reach` says it
/// gets, and a view the group refused is left showing everything.
#[test]
fn a_reach_drives_the_kinds_it_named_and_leaves_the_others_full() {
    let style = ChartStyle::default();
    let group = pinion_chart::LinkGroup::new([
        DonutChart::new(slices()).link("share"),
        pinion_chart::Link::inert("notes", "a caption, not capture data"),
    ])
    .expect("the declaration is well formed");

    let reach = group.publish(&Selection::Category("b".into()));
    assert!(reach.reaches("share"));
    assert!(!reach.reaches("notes"));

    let full = DonutChart::new(slices()).build(RECT, &style);
    let narrowed = DonutChart::new(slices())
        .muted_by_reach(Some(&reach), "share")
        .build(RECT, &style);
    let untouched = DonutChart::new(slices())
        .muted_by_reach(Some(&reach), "notes")
        .build(RECT, &style);
    let unpublished = DonutChart::new(slices())
        .muted_by_reach(None, "share")
        .build(RECT, &style);

    assert!(alpha(&narrowed, "chart.slice.0") < alpha(&full, "chart.slice.0"));
    assert_eq!(
        alpha(&untouched, "chart.slice.0"),
        alpha(&full, "chart.slice.0"),
        "a view the reach did not name narrows to nothing, silently to the eye \
         but explicably: the reach carries the reason"
    );
    assert_eq!(
        alpha(&unpublished, "chart.slice.0"),
        alpha(&full, "chart.slice.0"),
        "and nothing published is not an empty selection: every mark draws full"
    );
}
