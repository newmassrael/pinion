//! ★★★★★ R1767 — **a walk reproduces a specification no single frame can**,
//! driven.
//!
//! The fixture application here is the shape that made the per-frame verdict
//! unreachable, reduced to its two causes and nothing else:
//!
//! * three open sections, so **one frame paints one of them** and the other two
//!   are away — and an away surface reconciles nothing (R1742);
//! * one of those sections specifies **two surfaces that exclude each other** —
//!   a row with its roster shut, and the roster standing over it — so even
//!   standing inside it, no instant has both on the frame. That is not invented
//!   for a test: it is the node lab's own shape, written down in prose in that
//!   screen's judge at R1742 (*this document cannot be fully judged at any one
//!   instant*) and measured on the running analysis tool at the head of this
//!   round as `lab 1/15 reconciles=false` **while the reader was standing in
//!   it**.
//!
//! Every fixture surface answers from marks it really recorded
//! ([`record_painted_regions`]), so the evidence is `paint` for the reason a
//! real screen's is, and a test cannot pass by asserting its own tables.

use std::cell::Cell;

use pinion_a11y::WidgetA11y;
use pinion_core::availability::Unavailable;
use pinion_core::conformance::{
    Built, DocumentReport, Part, SpecDocument, parts_titled, titles_from,
};
use pinion_core::external::{External, StubExternal};
use pinion_core::painted::{PaintedRegions, record_painted_regions};
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::destination::{Destination, Destinations, Journey};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_screen::{
    JourneyConformance, JourneySection, JourneyStanding, Mount, Screen, ScreenRoster, SectionJudge,
    SectionStanding, Showing, SurfaceVisit,
};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{SizeStrategy, WidgetView};

const INSPECTOR_TAG: &str = "fixture_inspector";
const STREAM_TAG: &str = "fixture_stream";
/// The host's own paint root, which is what a page it draws itself is read from.
const BOARD_TAG: &str = "fixture_board";

thread_local! {
    /// Whether the inspector's enumeration roster is open — the session fact
    /// that decides WHICH of its two specified surfaces is on the frame.
    static ROSTER_OPEN: Cell<bool> = const { Cell::new(false) };
    /// Whether the board judge answers from its own tables instead of a frame.
    static BOARD_DECLARES: Cell<bool> = const { Cell::new(false) };
}

// --- The inspector: two surfaces that exclude each other ----------------------

struct InspectorFixture;

impl WidgetCore for InspectorFixture {
    type State = u32;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn tag() -> &'static str {
        INSPECTOR_TAG
    }

    fn read_state(_scene: &Scene) -> u32 {
        u32::from(ROSTER_OPEN.with(Cell::get))
    }

    fn view(_state: u32, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode::new(Vec::new()).with_tag(INSPECTOR_TAG))
    }

    fn event_name((): ()) -> &'static str {
        "inspector_event"
    }

    fn title() -> &'static str {
        "the inspector"
    }
}

impl WidgetA11y for InspectorFixture {}

impl WidgetView for InspectorFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: 800,
            height: 600,
        }
    }

    /// The verdict, read back out of the marks the fixture recorded.
    ///
    /// The two surfaces are **alternatives**: the roster standing over the row
    /// takes the row off the frame, and a shut roster has no options on it.
    /// Neither state is a defect, and neither state can be a whole verdict.
    fn conformance() -> Option<DocumentReport> {
        Some(
            inspector_specification().report_from_paint(INSPECTOR_TAG, &|regions, surface| {
                // ⚠ Which surface is away is decided by **the sibling being on
                // the frame**, never by this fixture's own flag. That is the
                // real node lab's rule, and its reason survives the reduction:
                // a verdict read from the paint store and an away-ness read
                // from the model are two accounts of one frame, and they come
                // apart on exactly the frame after a state change — measured
                // here as the first draft of this fixture, which reported a
                // roster of zero parts because the flag had moved and the
                // marks had not.
                let roster_on_frame = !regions.parts_under("fixture_inspector.roster.").is_empty();
                match surface {
                    "row" if roster_on_frame => Built::away(
                        "the roster is standing over this row, so the row this surface is \
                         specified for is not on the frame",
                    ),
                    "row" => Built::Standing(parts_titled(
                        regions,
                        "fixture_inspector.row.",
                        &titles_from(vec![("label", "Label"), ("value", "Value")]),
                    )),
                    "roster" if !roster_on_frame => {
                        Built::away("the roster is shut, so it has no options on the frame")
                    }
                    "roster" => Built::Standing(parts_titled(
                        regions,
                        "fixture_inspector.roster.",
                        &titles_from(vec![
                            ("first", "First"),
                            ("second", "Second"),
                            ("third", "Third"),
                        ]),
                    )),
                    other => panic!("the fixture specification does not name {other}"),
                }
            }),
        )
    }
}

fn inspector_specification() -> SpecDocument {
    SpecDocument::parse(
        r#"{ "row": {
              "canon": [
                { "key": "label", "title": "Label" },
                { "key": "value", "title": "Value" }
              ],
              "owed": []
           },
           "roster": {
              "canon": [
                { "key": "first", "title": "First" },
                { "key": "second", "title": "Second" },
                { "key": "third", "title": "Third" }
              ],
              "owed": []
           } }"#,
    )
    .expect("the fixture specification is a document")
}

/// Record the marks the inspector's frame would have drawn, for the state it is
/// in.
fn paint_inspector() {
    let marks: Vec<(String, Rect)> = if ROSTER_OPEN.with(Cell::get) {
        ["first", "second", "third"]
            .iter()
            .enumerate()
            .map(|(row, key)| {
                (
                    format!("fixture_inspector.roster.{key}"),
                    Rect::new(10, 40 + 20 * u32::try_from(row).unwrap_or(0), 120, 18),
                )
            })
            .collect()
    } else {
        ["label", "value"]
            .iter()
            .enumerate()
            .map(|(column, key)| {
                (
                    format!("fixture_inspector.row.{key}"),
                    Rect::new(10 + 130 * u32::try_from(column).unwrap_or(0), 20, 120, 18),
                )
            })
            .collect()
    };
    record_painted_regions(INSPECTOR_TAG, PaintedRegions::from_marks(marks));
}

// --- The stream: one surface, always whole while it is on the frame -----------

struct StreamFixture;

impl WidgetCore for StreamFixture {
    type State = u32;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        Vec::new()
    }

    fn tag() -> &'static str {
        STREAM_TAG
    }

    fn read_state(_scene: &Scene) -> u32 {
        0
    }

    fn view(_state: u32, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode::new(Vec::new()).with_tag(STREAM_TAG))
    }

    fn event_name((): ()) -> &'static str {
        "stream_event"
    }

    fn title() -> &'static str {
        "the stream"
    }
}

impl WidgetA11y for StreamFixture {}

impl WidgetView for StreamFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: 800,
            height: 600,
        }
    }

    fn conformance() -> Option<DocumentReport> {
        Some(stream_specification().report_from_paint(
            STREAM_TAG,
            &|regions, surface| match surface {
                "columns" => Built::Standing(parts_titled(
                    regions,
                    "fixture_stream.columns.",
                    &titles_from(vec![("time", "Time"), ("size", "Size")]),
                )),
                other => panic!("the fixture specification does not name {other}"),
            },
        ))
    }
}

fn stream_specification() -> SpecDocument {
    SpecDocument::parse(
        r#"{ "columns": {
              "canon": [
                { "key": "time", "title": "Time" },
                { "key": "size", "title": "Size" }
              ],
              "owed": []
           } }"#,
    )
    .expect("the fixture specification is a document")
}

fn paint_stream() {
    record_painted_regions(
        STREAM_TAG,
        PaintedRegions::from_marks(vec![
            (
                "fixture_stream.columns.time".to_owned(),
                Rect::new(0, 0, 80, 18),
            ),
            (
                "fixture_stream.columns.size".to_owned(),
                Rect::new(80, 0, 80, 18),
            ),
        ]),
    );
}

// --- The dashboard: a page the host paints, judged by a `SectionJudge` --------

struct BoardJudge;

impl SectionJudge for BoardJudge {
    fn conformance(&self, showing: Showing) -> DocumentReport {
        // ★ The declaration arm exists so one test can prove the walk refuses
        // evidence that is not a painted frame, exactly as the per-frame report
        // does (R1758). It is a fixture switch, not a route a screen has.
        if BOARD_DECLARES.with(Cell::get) {
            return board_specification().report(&|surface| match surface {
                "bar" => {
                    Built::Standing(vec![Part::new("add", "Add"), Part::new("preset", "Preset")])
                }
                other => panic!("the fixture specification does not name {other}"),
            });
        }
        // ⚠ A judge READS. It does not paint, and it does not forget: the host
        // owns that store and repaints it every frame. A first draft recorded
        // and forgot marks here, and it made a walk that had not begun report a
        // surface standing — the escape hatch R1742 refused, arriving from the
        // one direction nobody was watching.
        board_specification().report_from_paint(BOARD_TAG, &|regions, surface| match surface {
            "bar" if !showing.is_on_screen() => {
                Built::away("the reader is on another page, so this bar is not on the frame")
            }
            "bar" => Built::Standing(parts_titled(
                regions,
                "fixture_board.bar.",
                &titles_from(vec![("add", "Add"), ("preset", "Preset")]),
            )),
            other => panic!("the fixture specification does not name {other}"),
        })
    }
}

/// Record the marks the host's own dashboard page draws.
fn paint_board() {
    record_painted_regions(
        BOARD_TAG,
        PaintedRegions::from_marks(vec![
            ("fixture_board.bar.add".to_owned(), Rect::new(0, 0, 60, 18)),
            (
                "fixture_board.bar.preset".to_owned(),
                Rect::new(60, 0, 60, 18),
            ),
        ]),
    );
}

fn board_specification() -> SpecDocument {
    SpecDocument::parse(
        r#"{ "bar": {
              "canon": [
                { "key": "add", "title": "Add" },
                { "key": "preset", "title": "Preset" }
              ],
              "owed": []
           } }"#,
    )
    .expect("the fixture specification is a document")
}

// --- The fixture application --------------------------------------------------

fn destinations() -> Destinations {
    Destinations::new(vec![
        Destination::open("dashboard", "Dashboard"),
        Destination::open("inspector", "Inspector"),
        Destination::open("stream", "Stream"),
        Destination::closed(
            "topology",
            "Topology",
            Unavailable::reserved("requirement 12"),
        ),
    ])
    .expect("the fixture rail is navigable")
}

fn roster() -> ScreenRoster {
    ScreenRoster::new(
        destinations(),
        vec![
            (
                "inspector",
                Box::new(Mount::<InspectorFixture>::new()) as Box<dyn Screen>,
            ),
            ("stream", Box::new(Mount::<StreamFixture>::new())),
        ],
    )
    .expect("the fixture screens are mounted at open destinations")
    .judging("dashboard", Box::new(BoardJudge))
    .expect("the host's own page is open and unmounted")
}

fn at(key: &str) -> Journey {
    Journey::begin(&destinations(), key).expect("the fixture opens at a real destination")
}

/// Reset every fixture's session state, so a test says what it drove rather
/// than what the test before it left behind.
fn fresh() {
    ROSTER_OPEN.with(|open| open.set(false));
    BOARD_DECLARES.with(|declares| declares.set(false));
    pinion_core::painted::forget_painted_regions(INSPECTOR_TAG);
    pinion_core::painted::forget_painted_regions(STREAM_TAG);
    pinion_core::painted::forget_painted_regions(BOARD_TAG);
}

/// One frame: the host latches, then the section on screen paints.
///
/// The order is the framework's own — [`ScreenRoster::latch`] runs at
/// `read_state`, before the frame it belongs to is drawn — and getting it
/// backwards here would hide the very defect the recorder is shaped around.
fn frame(roster: &ScreenRoster, journey: &Journey) {
    let empty = Scene::Container(ContainerNode::new(Vec::new()));
    let _ = roster.latch(journey, &empty);
    match journey.at() {
        "inspector" => paint_inspector(),
        "stream" => paint_stream(),
        "dashboard" => paint_board(),
        _ => {}
    }
}

/// Walk to a destination and give it one frame.
fn walk_to(roster: &ScreenRoster, key: &str) -> Journey {
    let journey = at(key);
    frame(roster, &journey);
    journey
}

fn row<'a>(walk: &'a JourneyConformance, key: &str) -> &'a JourneySection {
    walk.rows()
        .iter()
        .find(|row| row.key == key)
        .unwrap_or_else(|| panic!("the fixture roster has no `{key}`"))
}

fn visit<'a>(walk: &'a JourneyConformance, key: &str, surface: &str) -> &'a SurfaceVisit {
    row(walk, key)
        .standing
        .surfaces()
        .iter()
        .find(|visit| visit.surface() == surface)
        .unwrap_or_else(|| panic!("`{key}` does not specify a `{surface}`"))
}

// --- The rows -----------------------------------------------------------------

/// ★★★★★ The measurement that forced the module: **no frame of this
/// application can reconcile**, and the walk is what can.
#[test]
fn no_single_frame_can_reconcile_and_the_walk_can() {
    fresh();
    let roster = roster();

    // Every frame of a full walk, asked the per-frame question. Not one of them
    // says the application reproduces its specification, and each says so for
    // an honest reason: the other sections are away.
    let mut per_frame = Vec::new();
    for key in ["dashboard", "inspector", "stream"] {
        let journey = walk_to(&roster, key);
        let frame_report = roster.conformance(&journey);
        per_frame.push(frame_report.conforms());
    }
    assert_eq!(
        per_frame,
        vec![false, false, false],
        "no frame of a three-section application can report conformance -- one \
         frame paints one section and an away surface reconciles nothing"
    );

    // The inspector's own alternatives, standing in it: whichever state it is
    // in, one of its two specified surfaces is off the frame.
    let inspector = walk_to(&roster, "inspector");
    let shut = roster.conformance(&inspector);
    let shut_row = shut
        .rows()
        .iter()
        .find(|row| row.key == "inspector")
        .expect("the fixture roster has an inspector");
    let SectionStanding::Judged(shut_report) = &shut_row.standing else {
        panic!("the inspector answers a specification");
    };
    assert_eq!(
        shut_report.away(),
        1,
        "with the roster shut, the roster surface is off the frame"
    );
    ROSTER_OPEN.with(|open| open.set(true));
    frame(&roster, &inspector);
    let open = roster.conformance(&inspector);
    let open_row = open
        .rows()
        .iter()
        .find(|row| row.key == "inspector")
        .expect("the fixture roster has an inspector");
    let SectionStanding::Judged(open_report) = &open_row.standing else {
        panic!("the inspector answers a specification");
    };
    assert_eq!(
        open_report.away(),
        1,
        "★★★★★ and with it open the ROW is off the frame -- the two surfaces \
         are alternatives, so this section cannot be fully judged at any one \
         instant"
    );

    // The walk, however, saw both.
    let walk = roster.journey_conformance(&inspector);
    assert!(
        visit(&walk, "inspector", "row").stood(),
        "the walk saw the row, at the step the roster was shut"
    );
    assert!(
        visit(&walk, "inspector", "roster").stood(),
        "★★★★★ and it saw the roster, at a later step -- two one-frame verdicts \
         about frames that cannot coexist, held by one report"
    );
    assert_ne!(
        visit(&walk, "inspector", "row").step(),
        visit(&walk, "inspector", "roster").step(),
        "★★ and they are credited to DIFFERENT steps, which is what stops this \
         being a claim about an instant"
    );
}

/// A walk that stood in every open section, with each section's surfaces all
/// seen somewhere, reports conformance — and says how long the walk was.
#[test]
fn a_full_walk_reproduces_the_specification() {
    fresh();
    let roster = roster();
    walk_to(&roster, "dashboard");
    walk_to(&roster, "inspector");
    ROSTER_OPEN.with(|open| open.set(true));
    let inspector = at("inspector");
    frame(&roster, &inspector);
    let stream = walk_to(&roster, "stream");

    let walk = roster.journey_conformance(&stream);
    assert_eq!(walk.unvisited(), 0, "every open section was stood in");
    assert_eq!(walk.unanswered(), 0, "something answered for each of them");
    assert_eq!(walk.declared(), 0, "and each answered from a painted frame");
    assert_eq!(
        walk.stood(),
        walk.surfaces(),
        "every specified surface was on some frame of the walk"
    );
    assert_eq!(
        walk.unreconciled(),
        0,
        "and reconciled on the frame it was on"
    );
    assert!(
        walk.conforms(),
        "★★★★★ so the application reproduces its specification OVER THIS WALK, \
         which is the sentence no frame could ever say"
    );
    assert_eq!(
        walk.stops(),
        3,
        "★ and the claim carries how far the reader walked to earn it"
    );
    assert_eq!(
        walk.steps(),
        4,
        "★★ which is NOT how many readings it took: the roster was opened at a \
         stop the walk had already made"
    );
    assert_eq!(
        walk.specified(),
        walk.reproduced(),
        "the totals agree because every credited frame was whole"
    );
}

/// An open section the walk never stood in refuses the verdict, and is still in
/// the denominator.
#[test]
fn a_section_the_walk_never_stood_in_refuses_it() {
    fresh();
    let roster = roster();
    walk_to(&roster, "dashboard");
    let inspector = walk_to(&roster, "inspector");
    ROSTER_OPEN.with(|open| open.set(true));
    frame(&roster, &inspector);

    let walk = roster.journey_conformance(&inspector);
    assert_eq!(walk.unvisited(), 1, "the stream was never stood in");
    assert!(
        !walk.conforms(),
        "★★★★★ so the application cannot claim to reproduce its specification \
         -- conformance earned by the sections somebody happened to visit is \
         R1738's defect at journey scale"
    );
    assert!(
        !row(&walk, "stream").is_visited(),
        "and the row names which one"
    );
    assert_eq!(
        visit(&walk, "stream", "columns").specified(),
        2,
        "★★ while its SPECIFICATION is still counted -- a section missing from \
         the denominator is the same defect wearing a different hat"
    );
    assert_eq!(
        visit(&walk, "stream", "columns").reproduced(),
        0,
        "and none of it is credited, because no frame of this walk had it"
    );
}

/// A closed destination owes a walk nothing.
#[test]
fn a_closed_destination_owes_the_walk_nothing() {
    fresh();
    let roster = roster();
    let walk = roster.journey_conformance(&at("dashboard"));
    assert_eq!(walk.closed(), 1);
    assert_eq!(walk.open(), 3);
    assert_eq!(walk.sections(), 4);
    let topology = row(&walk, "topology");
    assert!(!topology.is_open());
    assert!(
        topology.reproduced_over_the_walk(),
        "a destination nobody can arrive at cannot be walked to, so it is not \
         what stops the verdict"
    );
    assert_eq!(
        topology.standing.why().as_deref(),
        Some("reserved for requirement 12"),
        "and it says why, in the destination's own words"
    );
}

/// A verdict answered from a screen's own tables is refused, over a walk as
/// over a frame.
#[test]
fn a_declared_verdict_is_refused_over_a_walk() {
    fresh();
    BOARD_DECLARES.with(|declares| declares.set(true));
    let roster = roster();
    walk_to(&roster, "dashboard");
    walk_to(&roster, "inspector");
    ROSTER_OPEN.with(|open| open.set(true));
    let inspector = at("inspector");
    frame(&roster, &inspector);
    let stream = walk_to(&roster, "stream");

    let walk = roster.journey_conformance(&stream);
    assert_eq!(walk.unvisited(), 0, "the walk stood everywhere");
    assert_eq!(
        walk.declared(),
        1,
        "★★★★★ but one section answered from its own tables"
    );
    assert!(
        !walk.conforms(),
        "so the walk refuses it -- a verdict that could not fail is not made \
         truer by being taken three times"
    );
}

/// A surface that stood and did not reconcile is refused, and the step it stood
/// at is still published.
#[test]
fn a_surface_that_stood_and_diverged_is_refused() {
    fresh();
    let roster = roster();
    walk_to(&roster, "dashboard");
    // The inspector paints its row with a part missing: a real divergence on a
    // frame that really happened.
    let inspector = at("inspector");
    let empty = Scene::Container(ContainerNode::new(Vec::new()));
    let _ = roster.latch(&inspector, &empty);
    record_painted_regions(
        INSPECTOR_TAG,
        PaintedRegions::from_marks(vec![(
            "fixture_inspector.row.label".to_owned(),
            Rect::new(10, 20, 120, 18),
        )]),
    );
    ROSTER_OPEN.with(|open| open.set(true));
    frame(&roster, &inspector);
    let stream = walk_to(&roster, "stream");

    let walk = roster.journey_conformance(&stream);
    let row_visit = visit(&walk, "inspector", "row");
    assert!(row_visit.stood(), "the row was on a frame of this walk");
    assert!(
        !row_visit.reconciles(),
        "★★★★★ and what it drew there is not what it is specified to draw"
    );
    assert_eq!(walk.unreconciled(), 1, "one surface stood and diverged");
    assert!(!walk.conforms(), "so the walk refuses the application");
    assert_eq!(
        row_visit.step(),
        Some(2),
        "★ and the refusal names the step it was read at, which is the whole of \
         how a walk keeps `a verdict is about one frame`"
    );
}

/// Leaving a section keeps what the walk saw of it, while the per-frame report
/// still forgets — the two reports are different claims and stay so.
#[test]
fn leaving_keeps_the_walk_and_still_empties_the_frame() {
    fresh();
    let roster = roster();
    walk_to(&roster, "dashboard");
    walk_to(&roster, "inspector");
    ROSTER_OPEN.with(|open| open.set(true));
    frame(&roster, &at("inspector"));
    let stream = walk_to(&roster, "stream");

    let frame_report = roster.conformance(&stream);
    let inspector_row = frame_report
        .rows()
        .iter()
        .find(|row| row.key == "inspector")
        .expect("the fixture roster has an inspector");
    let SectionStanding::Judged(report) = &inspector_row.standing else {
        panic!("the inspector answers a specification");
    };
    assert_eq!(
        report.reproduced(),
        0,
        "R1763 stands: the section the reader left reproduces nothing on THIS \
         frame, because it painted none of it"
    );

    let walk = roster.journey_conformance(&stream);
    assert_eq!(
        visit(&walk, "inspector", "row").reproduced(),
        2,
        "★★★★★ and the walk still holds what it saw there, which is a different \
         claim rather than the one R1763 removed -- it names the step"
    );
    assert_eq!(
        visit(&walk, "inspector", "row").step(),
        Some(2),
        "at the second reading of this walk, which is what `about one frame` \
         survives as"
    );
    assert!(
        !row(&walk, "inspector").showing,
        "and the row says the reader is not there now"
    );
}

/// The section a reader is standing on is in the report on the frame it
/// arrives, not one frame later.
#[test]
fn the_frame_in_front_of_the_reader_is_folded_in_live() {
    fresh();
    let roster = roster();
    let stream = walk_to(&roster, "stream");
    // Exactly one latch has happened at `stream`, and the recorder attributes a
    // latch to the position the PREVIOUS one left — so nothing is recorded for
    // it yet, and only the live fold can see it.
    let walk = roster.journey_conformance(&stream);
    assert!(
        row(&walk, "stream").is_visited(),
        "★★★★★ the section a reader arrived at is in the report on that frame"
    );
    assert!(
        visit(&walk, "stream", "columns").stood(),
        "with the verdict its own marks give"
    );
    assert_eq!(
        visit(&walk, "stream", "columns").reproduced(),
        2,
        "read from the frame, not from a table"
    );
}

/// Reading the report does not change it.
#[test]
fn reading_the_walk_twice_answers_the_same_thing() {
    fresh();
    let roster = roster();
    walk_to(&roster, "dashboard");
    let inspector = walk_to(&roster, "inspector");
    let once = roster.journey_conformance(&inspector);
    let twice = roster.journey_conformance(&inspector);
    assert_eq!(
        once, twice,
        "the live fold is a derivation and not a write -- a report that \
         accumulated by being read would credit a section for being asked about"
    );
}

/// The recorder attributes a frame's marks to the section that painted them,
/// not to the one the journey has just moved to.
#[test]
fn a_frame_is_credited_to_the_section_that_painted_it() {
    fresh();
    let roster = roster();
    // Stand in the inspector with the roster shut, and paint.
    let inspector = walk_to(&roster, "inspector");
    let _ = &inspector;
    // Now move to the stream WITHOUT painting: the store still holds the
    // inspector's marks, and this is the frame the naive recorder mis-files.
    let stream = at("stream");
    let empty = Scene::Container(ContainerNode::new(Vec::new()));
    let _ = roster.latch(&stream, &empty);

    let walk = roster.journey_conformance(&stream);
    assert!(
        visit(&walk, "inspector", "row").stood(),
        "★★★★★ the inspector's marks are credited to the inspector"
    );
    assert!(
        !visit(&walk, "stream", "columns").stood(),
        "★★★★★ and the stream, which has not painted, is credited with nothing \
         -- attributing one section's marks to another is the defect this \
         ordering exists to prevent"
    );
}

/// Nothing is credited before a frame exists.
#[test]
fn a_walk_that_has_not_begun_credits_nothing() {
    fresh();
    let roster = roster();
    let walk = roster.journey_conformance(&at("dashboard"));
    assert_eq!(
        walk.stops(),
        1,
        "a walk that never moved still stood somewhere"
    );
    assert_eq!(walk.steps(), 1, "and this is its first reading");
    assert_eq!(walk.stood(), 0, "★★★★★ and no surface has been on a frame");
    assert!(!walk.conforms());
    assert_eq!(
        walk.specified(),
        9,
        "while every specification is already counted: 2 bar + 2 row + 3 roster \
         + 2 columns"
    );
    assert_eq!(walk.reproduced(), 0);
}

/// A section nothing answers for refuses the walk, and says so.
#[test]
fn a_section_nothing_answers_for_refuses_the_walk() {
    fresh();
    let bare_roster =
        || Destinations::new(vec![Destination::open("home", "Home")]).expect("a roster");
    let bare = ScreenRoster::new(bare_roster(), Vec::new())
        .expect("nothing is mounted, so nothing can be mounted wrongly");
    let home = Journey::begin(&bare_roster(), "home").expect("`home` is open");
    let empty = Scene::Container(ContainerNode::new(Vec::new()));
    let _ = bare.latch(&home, &empty);

    let walk = bare.journey_conformance(&home);
    assert_eq!(walk.unanswered(), 1);
    assert!(!walk.conforms());
    assert!(matches!(
        walk.rows()[0].standing,
        JourneyStanding::Unanswered(_)
    ));
    assert_eq!(walk.rows()[0].standing.word(), "unanswered");
}

/// The published value carries every fact the typed report does.
#[test]
fn the_wire_carries_the_walk() {
    fresh();
    let roster = roster();
    walk_to(&roster, "dashboard");
    walk_to(&roster, "inspector");
    ROSTER_OPEN.with(|open| open.set(true));
    let inspector = at("inspector");
    frame(&roster, &inspector);
    let stream = walk_to(&roster, "stream");

    let walk = roster.journey_conformance(&stream);
    let wire = walk.to_json();
    assert_eq!(wire["conforms"], serde_json::json!(true));
    assert_eq!(wire["steps"], serde_json::json!(4));
    assert_eq!(wire["stops"], serde_json::json!(3));
    assert_eq!(wire["sections"], serde_json::json!(4));
    assert_eq!(wire["unvisited"], serde_json::json!(0));
    assert_eq!(wire["stood"], wire["surfaces"]);
    let rows = wire["rows"].as_array().expect("rows is an array");
    let inspector_row = rows
        .iter()
        .find(|row| row["key"] == serde_json::json!("inspector"))
        .expect("the inspector is a row");
    assert_eq!(
        inspector_row["surfaces"]["row"]["step"],
        serde_json::json!(2),
        "★★ every credited verdict names the step it was read at, on the wire too"
    );
    assert_eq!(
        inspector_row["surfaces"]["roster"]["step"],
        serde_json::json!(3),
        "★★★★★ and the sibling names a DIFFERENT one, which is the wire saying \
         these two were never on screen together"
    );
    assert_eq!(
        inspector_row["surfaces"]["roster"]["stood"],
        serde_json::json!(true)
    );
    assert!(
        inspector_row["surfaces"]["row"]["why"].is_string(),
        "★ and a surface credited at an earlier step still says why it is not \
         on the frame now"
    );
}

/// ★★★★★ R1770 — **a credited verdict is dropped when the surface is next read
/// at a different extent.**
///
/// Found by running, not by reading. The analysis tool this module exists for
/// was walked maximised, where it conforms; the window was shrunk, where one
/// section is given less width than it declares it lays out at and therefore
/// declines to be judged; and the walk **still reported `conforms=true`**, on
/// the strength of frames painted at a size that no longer existed.
///
/// The asymmetry this module is built on — a standing frame replaces the
/// credit, an away frame does not — is unchanged and right: *the reader closed
/// it again* leaves the frame that verdict came from intact. *The reader
/// resized the window* does not, and until R1770 a verdict did not carry the
/// size it was read at, so the difference could not be written down.
#[test]
fn a_credit_is_dropped_when_the_surface_is_next_read_at_another_extent() {
    use pinion_core::painted::Extent;

    /// The inspector's frame for the state it is in, at a stated extent.
    ///
    /// The inspector is the fixture with an away path — its roster is on the
    /// frame or it is not — and an away is what the case this test is about
    /// produces: a section given less room than it declares it lays out at
    /// declines to be judged rather than drawing badly.
    fn inspector_at(extent: Extent, roster_open: bool) {
        ROSTER_OPEN.with(|open| open.set(roster_open));
        let marks: Vec<(String, Rect)> = if roster_open {
            ["first", "second", "third"]
                .iter()
                .enumerate()
                .map(|(row, key)| {
                    (
                        format!("fixture_inspector.roster.{key}"),
                        Rect::new(10, 40 + 20 * u32::try_from(row).unwrap_or(0), 120, 18),
                    )
                })
                .collect()
        } else {
            ["label", "value"]
                .iter()
                .enumerate()
                .map(|(column, key)| {
                    (
                        format!("fixture_inspector.row.{key}"),
                        Rect::new(10 + 130 * u32::try_from(column).unwrap_or(0), 20, 120, 18),
                    )
                })
                .collect()
        };
        record_painted_regions(
            INSPECTOR_TAG,
            PaintedRegions::from_marks(marks).with_extent(extent),
        );
    }

    let big = Extent::new(1200, 800);
    let small = Extent::new(600, 800);
    let empty = Scene::Container(ContainerNode::new(Vec::new()));

    fresh();
    let first = roster();
    walk_to(&first, "dashboard");

    // One frame with the roster open, at a known extent: `roster` is credited
    // there and `row` is away, which is this fixture's pair of alternatives.
    let inspector = at("inspector");
    let _ = first.latch(&inspector, &empty);
    inspector_at(big, true);
    let walked = first.journey_conformance(&inspector);
    let credited = visit(&walked, "inspector", "roster");
    assert!(credited.stood(), "the roster was on a frame of this walk");
    assert!(credited.reconciles(), "and it reproduced its specification");
    assert_eq!(
        credited.at(),
        Some(big),
        "★ and the credit names the extent it was read at",
    );

    // ⚠ First, the asymmetry this rule must NOT weaken: at the SAME extent, an
    // away frame leaves the credit alone. That is R1767's rule and the only
    // thing that lets two mutually exclusive surfaces of one section both be
    // credited over one walk.
    let _ = first.latch(&inspector, &empty);
    inspector_at(big, false);
    let same_size = first.journey_conformance(&inspector);
    assert!(
        visit(&same_size, "inspector", "roster").stood(),
        "★★ shutting the roster does not take away the frame it was open on -- \
         the new rule is about the SIZE changing, not about a surface going \
         quiet",
    );

    // Now the window changes, and the away arrives at a different extent.
    let _ = first.latch(&inspector, &empty);
    inspector_at(small, false);
    let after = first.journey_conformance(&inspector);
    let now = visit(&after, "inspector", "roster");
    assert!(
        !now.stood(),
        "★★★★★ the earlier credit is gone: it was read at {big} and this surface \
         is {small} now, so the frame it came from is not a frame of this \
         application any more",
    );
    assert_eq!(now.reproduced(), 0, "and it credits nothing");
    assert!(
        !after.conforms(),
        "★★★★★ so the walk stops claiming conformance on the strength of a size \
         that no longer exists -- which it did, measured, before this rule",
    );
}
