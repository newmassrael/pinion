//! ★★★★★ R1724 — **what mounting guarantees, driven.**
//!
//! Each test here is one row of the table in the crate's own documentation,
//! and the rows were measured against the reference toolkit at 6.11.1 by
//! building a probe and running it. Where a row says the reference does the
//! wrong thing, the assertion message says what it did.

use std::cell::Cell;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::chrome::{HostChrome, Part as ChromePart, host_chrome};
use pinion_core::conformance::{Built, DocumentReport, Part, SpecDocument};
use pinion_core::external::{External, StubExternal, layout_size};
use pinion_core::reactive::{Owner, VIEWPORT_SIZE};
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::destination::{Destination, Destinations, Journey};
use pinion_core::{Frame, Modifiers, Scene, WidgetCore};
use pinion_screen::{Mount, Screen, ScreenRoster, SectionStanding};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec};

// --- Two fixture screens, each observable ------------------------------------
//
// Real bindings, not stubs of `Screen`: the whole claim of this crate is that
// an existing `WidgetView` becomes a page without being edited, so the tests
// must go through `Mount<V>` rather than around it.

thread_local! {
    /// What the lab's projection currently reads, so a test can move it.
    static LAB_VALUE: Cell<u32> = const { Cell::new(0) };
    /// The extent the lab last saw while painting.
    static LAB_PAINTED_AT: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
    /// The extent the lab last saw while handling a key.
    static LAB_KEYED_AT: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
    /// Whether the viewer took the last file drop.
    static VIEWER_DROPS: Cell<u32> = const { Cell::new(0) };
}

const LAB_SHRINK: ShrinkPolicy = ShrinkPolicy::panning(LAB_COMFORTABLE, (600, 300));

const LAB_TAG: &str = "fixture_lab";
const LAB_FIELD_TAG: &str = "fixture_lab.field";
/// The navigation the lab fixture draws for itself when nothing else does.
const LAB_RAIL_TAG: &str = "fixture_lab.rail";
const VIEWER_TAG: &str = "fixture_viewer";

struct LabFixture;

impl WidgetCore for LabFixture {
    type State = u32;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            LAB_FIELD_TAG,
            Box::new(StubExternal::new()),
        )]
    }

    fn tag() -> &'static str {
        LAB_TAG
    }

    fn read_state(_scene: &Scene) -> u32 {
        LAB_VALUE.with(Cell::get)
    }

    fn view(state: u32, _frame: &Frame) -> Scene {
        // A screen that hit-tests its own rectangles asks the framework how
        // big it is. That question is the one `with_surface_extent` answers.
        LAB_PAINTED_AT.with(|at| at.set(layout_size(LAB_TAG, (10, 10), (1440, 900))));
        Scene::Container(
            ContainerNode::new(vec![Scene::Container(
                ContainerNode::new(Vec::new()).with_tag(format!("lab.value.{state}")),
            )])
            .with_tag(LAB_TAG),
        )
    }

    fn event_name((): ()) -> &'static str {
        "lab_event"
    }

    fn title() -> &'static str {
        "the node graph lab"
    }

    fn keybinding(key: &str) -> Option<()> {
        (key == "F5").then_some(())
    }

    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        key: &str,
        _modifiers: Modifiers,
    ) -> bool {
        LAB_KEYED_AT.with(|at| at.set(layout_size(LAB_TAG, (10, 10), (1440, 900))));
        key == "Space"
    }

    fn fmt_state_log(state: &u32) -> String {
        format!("lab at {state}")
    }
}

impl WidgetA11y for LabFixture {
    /// ★★★★★ R1725 — the fixture behaves like the real guest: it contributes a
    /// navigation of its own **only where it is the one providing it**. The
    /// accessibility half is the one the reference toolkit cannot get right —
    /// measured at 6.11.1, a placed application window's menu bar, tool bar and
    /// status bar all stay in the tree beside the host's, so a reader is told
    /// the application has two of each.
    fn access_node(state: &u32, _focused: Option<&str>) -> Vec<AccessNode> {
        let mut nodes =
            vec![AccessNode::new(LAB_TAG, AriaRole::Group).with_name(format!("lab {state}"))];
        if !host_chrome().provides(ChromePart::Navigation) {
            nodes.push(AccessNode::new(LAB_RAIL_TAG, AriaRole::Navigation).with_name("sections"));
        }
        nodes
    }
}

/// The width below which the lab fixture's layout stops reflowing — the fact a
/// region has to respect or pan over.
const LAB_COMFORTABLE: (u32, u32) = (1625, 400);

impl WidgetView for LabFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::shrinking(LAB_SHRINK, LAB_COMFORTABLE)
    }

    /// Like the real node lab's: a layout that stops reflowing well before the
    /// window does, and pans over the difference.
    fn shrink_policy() -> Option<ShrinkPolicy> {
        Some(LAB_SHRINK)
    }

    /// ★★★★★ R1738 — this fixture answers a written specification, and the
    /// viewer beside it deliberately does not. One roster then holds all four
    /// standings a section can have, which is what the report is tested
    /// against.
    fn conformance() -> Option<DocumentReport> {
        Some(lab_specification().report(&|surface| match surface {
            // The build has the two parts in the specified order and is missing
            // the third — a real difference rather than a passing fixture,
            // because a report tested only against agreement cannot be shown to
            // report anything.
            "rows" => Built::Standing(vec![Part::new("id", "ID"), Part::new("name", "Name")]),
            other => panic!("the fixture specification does not name {other}"),
        }))
    }

    fn focus_ring_style(_focused_tag: &str) -> Option<pinion_overlay::FocusRingStyle> {
        // A content surface that owns its own indicator — the one hook whose
        // `None` is a decision rather than an absence.
        None
    }
}

struct ViewerFixture;

impl WidgetCore for ViewerFixture {
    type State = u32;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn tag() -> &'static str {
        VIEWER_TAG
    }

    fn read_state(_scene: &Scene) -> u32 {
        0
    }

    fn view(_state: u32, _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode::new(Vec::new()).with_tag(VIEWER_TAG))
    }

    fn event_name((): ()) -> &'static str {
        "viewer_event"
    }

    fn title() -> &'static str {
        "the capture viewer"
    }
}

impl WidgetA11y for ViewerFixture {}

impl WidgetView for ViewerFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: 800,
            height: 600,
        }
    }

    /// A section that tears a panel off owns a window. The reference leaves it
    /// on screen when you navigate away; here it is part of what "the current
    /// screen" means.
    fn windows() -> Vec<WindowSpec> {
        vec![
            WindowSpec::main(Self::title(), Self::initial_size_strategy()),
            WindowSpec::new(
                "viewer.float",
                "torn-off byte pane",
                Self::initial_size_strategy(),
            ),
        ]
    }

    fn on_file_drop(_window_id: &str, _state: &u32, _path: &str) -> bool {
        VIEWER_DROPS.with(|n| n.set(n.get() + 1));
        true
    }
}

// --- The fixture application --------------------------------------------------

/// The specification the lab fixture answers to (R1738).
///
/// Three parts, of which the fixture builds two, so every count the report adds
/// up has a value other than "all of it" and a test cannot pass by the numbers
/// happening to agree.
fn lab_specification() -> SpecDocument {
    SpecDocument::parse(
        r#"{ "rows": {
              "canon": [
                { "key": "id", "title": "ID" },
                { "key": "name", "title": "Name" },
                { "key": "state", "title": "State" }
              ],
              "owed": [
                {
                  "key": "state",
                  "sentence": "part 2 `state` (State) is specified and the surface has no such part",
                  "since": "R1738",
                  "why": "The fixture builds two of the three parts on purpose, so every count this report adds up has a value other than all-of-it and a test cannot pass by the numbers happening to agree."
                }
              ]
           } }"#,
    )
    .expect("the fixture specification is a document")
}

fn destinations() -> Destinations {
    Destinations::new(vec![
        Destination::open("dashboard", "Dashboard"),
        Destination::open("catalog", "Catalog"),
        Destination::open("stream", "Stream"),
        Destination::closed(
            "topology",
            "Topology",
            pinion_core::availability::Unavailable::reserved("requirement 12"),
        ),
    ])
    .expect("the fixture rail is navigable")
}

fn roster() -> ScreenRoster {
    ScreenRoster::new(
        destinations(),
        vec![
            (
                "catalog",
                Box::new(Mount::<LabFixture>::new()) as Box<dyn Screen>,
            ),
            ("stream", Box::new(Mount::<ViewerFixture>::new())),
        ],
    )
    .expect("the fixture screens are mounted at open destinations")
}

fn at(key: &str) -> Journey {
    Journey::begin(&destinations(), key).expect("the fixture opens at a real destination")
}

// --- The rows -----------------------------------------------------------------

/// ★★★★★ The row the reference gets most wrong. Measured at 6.11.1: a page
/// that is not current, sent a press, a key and a wheel, **counted all three**.
#[test]
fn r1724_only_the_screen_you_are_at_has_externals() {
    let roster = roster();

    let here = roster.externals(&at("catalog"));
    let tags: Vec<&str> = here.iter().map(|e| e.tag.as_ref()).collect();
    assert_eq!(
        tags,
        vec![LAB_TAG, LAB_FIELD_TAG],
        "the current screen's primary surface first, then its extras, in the \
         order the binding declared them"
    );

    let elsewhere = roster.externals(&at("stream"));
    let tags: Vec<&str> = elsewhere.iter().map(|e| e.tag.as_ref()).collect();
    assert_eq!(
        tags,
        vec![VIEWER_TAG],
        "and standing at the other destination, the lab's two surfaces are not \
         in the set at all -- there is nothing for the router to resolve a \
         press to and no slot for the wire to address"
    );
    assert!(
        !tags.contains(&LAB_TAG) && !tags.contains(&LAB_FIELD_TAG),
        "stated the other way round, because this is the guarantee"
    );
}

/// A destination the host paints itself is not a screen, and every accessor
/// says so rather than guessing.
/// ★★★★★ R1725 — **a screen is told what the place it was put in already
/// provides, and it is told at every hook.**
///
/// Both halves are load-bearing and they fail differently. Told only at
/// `view`, a guest would omit its navigation from the picture and keep it in
/// the accessibility tree — a landmark a screen reader walks to and a pointer
/// cannot reach, which is worse than drawing two. Told nothing at all, it draws
/// its own beside its host's, which is what the first mount did and what the
/// reference toolkit does by construction (measured at 6.11.1: the placed
/// window's menu bar, tool bar and status bar all keep drawing, its tree
/// carries 2 of each, and there is no property, method or event by which the
/// guest could have asked).
#[test]
fn r1725_a_screen_is_told_what_its_place_already_provides() {
    let bare = roster();
    let hosted = roster().providing(HostChrome::NONE.with(ChromePart::Navigation));

    assert!(
        bare.chrome().is_empty(),
        "a host that declares nothing must provide nothing, so every existing \
         host keeps the behaviour it had"
    );

    // Outside the roster nothing is declared, which is the standalone reading.
    assert!(!host_chrome().provides(ChromePart::Navigation));

    let rails = |roster: &ScreenRoster| -> usize {
        roster
            .with_current(&at("catalog"), |screen| screen.access_node(None))
            .expect("catalog is a mounted screen")
            .iter()
            .filter(|n| n.tag == LAB_RAIL_TAG)
            .count()
    };
    assert_eq!(
        rails(&bare),
        1,
        "a screen whose host provides nothing keeps its own navigation -- it \
         may be the only one there is"
    );
    assert_eq!(
        rails(&hosted),
        0,
        "and it leaves it out where the place already has one. This is the \
         ACCESSIBILITY hook, not the view: it is reached through the same \
         `with_current`, so a declaration that only wrapped the paint would \
         pass a picture test and fail here"
    );

    // …and the declaration does not outlive the call.
    assert!(
        !host_chrome().provides(ChromePart::Navigation),
        "a declaration left standing would make the NEXT screen omit a \
         navigation nothing is drawing"
    );
}

/// ★★ R1725 — the extent grant and the chrome declaration are two facts about
/// one place, and a screen that reads both must get both from one call.
///
/// They were added a round apart and are wrapped at the same site for that
/// reason; this pins it, because the cheap way to add the second was to wrap
/// only the branch that already had a grant — which would silently tell a
/// screen nothing on the first frame, before anything has been placed.
#[test]
fn r1725_the_place_is_stated_before_anything_has_been_placed() {
    let hosted = roster().providing(HostChrome::NONE.with(ChromePart::Navigation));
    // No `page_scene` yet, so no extent has ever been recorded.
    let told = hosted
        .with_current(&at("catalog"), |_| {
            host_chrome().provides(ChromePart::Navigation)
        })
        .expect("catalog is a mounted screen");
    assert!(
        told,
        "the chrome declaration must not be conditional on a region having \
         been measured: a host lays a rail seat out before it paints a page"
    );
}

#[test]
fn r1724_an_unmounted_destination_is_the_hosts_own_page() {
    let roster = roster();
    let dashboard = at("dashboard");
    assert!(!roster.is_mounted("dashboard"));
    assert_eq!(roster.current_tag(&dashboard), None);
    assert_eq!(roster.current_title(&dashboard), None);
    assert!(roster.externals(&dashboard).is_empty());
    assert!(
        roster
            .page_scene(&dashboard, (100, 100), &Frame::default())
            .is_none()
    );
    assert_eq!(
        roster.mounted_keys().collect::<Vec<_>>(),
        vec!["catalog", "stream"],
        "and the mounted keys come back in rail order, not in map order"
    );
}

/// ★★★★★ The row the reference gets right, and which this must not regress:
/// leaving and returning is a return, not a restart.
#[test]
fn r1724_a_screen_you_left_keeps_what_it_had() {
    let roster = roster();
    let catalog = at("catalog");

    LAB_VALUE.with(|v| v.set(7));
    let state = roster.latch(&catalog, &Scene::Container(ContainerNode::new(Vec::new())));
    assert_eq!(state.at, 1, "catalog is the second destination on the rail");

    // Go somewhere else, and let the world move on while we are away.
    let stream = at("stream");
    LAB_VALUE.with(|v| v.set(99));
    let away = roster.latch(&stream, &Scene::Container(ContainerNode::new(Vec::new())));
    assert_eq!(away.at, 2, "and stream is the third");

    // Come back. Nothing has latched the lab since, so what it holds is what
    // it held -- the reference toolkit's one good row, kept.
    assert_eq!(
        roster
            .with_current(&catalog, |screen| screen.fmt_state_log())
            .as_deref(),
        Some("lab at 7"),
        "the projection parked before leaving is still there on return"
    );
}

/// The revision is a change detector, and both halves of it move.
#[test]
fn r1724_the_hosts_state_notices_navigation_and_the_screen_moving() {
    let roster = roster();
    let catalog = at("catalog");
    let empty = Scene::Container(ContainerNode::new(Vec::new()));

    LAB_VALUE.with(|v| v.set(1));
    let first = roster.latch(&catalog, &empty);
    let again = roster.latch(&catalog, &empty);
    assert_eq!(
        first, again,
        "a projection that has not moved does not move the revision, so a host \
         with constant state is not repainted for nothing"
    );

    LAB_VALUE.with(|v| v.set(2));
    let moved = roster.latch(&catalog, &empty);
    assert_ne!(
        again.revision, moved.revision,
        "and when the screen inside a constant host moves, the host's state \
         does too -- without this a mounted text field paints its first frame \
         and no other"
    );

    let elsewhere = roster.latch(&at("dashboard"), &empty);
    assert_ne!(
        moved.at, elsewhere.at,
        "arriving somewhere is the other half of the detector"
    );
}

/// ★★★★★ The paint half and the gesture half of a mounted screen read ONE
/// rectangle. Before R1724 the in-view branch of `layout_size` answered the
/// window — which is the shape of the R1700 defect, where a screen's paint
/// reflowed and its hit test did not.
#[test]
fn r1724_a_mounted_screen_lays_out_in_its_region_not_the_window() {
    let roster = roster();
    let catalog = at("catalog");
    let owner = Owner::new();
    VIEWPORT_SIZE.resolve(&owner).set((1920, 1080));

    let region = (1000, 780);
    owner.run(|| {
        let _ = roster.page_scene(&catalog, region, &Frame::default());
    });
    assert_eq!(
        LAB_PAINTED_AT.with(Cell::get),
        region,
        "the screen painted in the region the host placed it in, not the \
         1920x1080 window it is inside"
    );

    // And the hooks the shell wraps in an owner scope read the same rectangle,
    // which is what stops a press landing where nothing is drawn.
    let mut scene = Scene::Container(ContainerNode::new(Vec::new()));
    owner.run(|| {
        roster.with_current(&catalog, |screen| {
            screen.apply_key(&mut scene, None, "Space", Modifiers::default())
        });
    });
    assert_eq!(
        LAB_KEYED_AT.with(Cell::get),
        region,
        "a key handler inside an owner scope reads the region too -- this is \
         the half a grant only around `view` would have missed"
    );
}

/// ★★★★★ The row that changes what an application can be. Measured at 6.11.1:
/// a page's own floating tool window is **still visible after you leave its
/// page**.
#[test]
fn r1724_a_screens_windows_leave_with_it() {
    let roster = roster();

    let viewer_windows = roster
        .with_current(&at("stream"), |screen| screen.windows())
        .expect("the viewer is mounted");
    let ids: Vec<String> = viewer_windows
        .iter()
        .map(|w| w.id.as_ref().to_owned())
        .collect();
    assert!(
        ids.iter().any(|id| id == "viewer.float"),
        "while its section is showing, the torn-off pane is one of the \
         application's windows: {ids:?}"
    );

    let lab_windows = roster
        .with_current(&at("catalog"), |screen| screen.windows())
        .expect("the lab is mounted");
    let ids: Vec<String> = lab_windows
        .iter()
        .map(|w| w.id.as_ref().to_owned())
        .collect();
    assert!(
        !ids.iter().any(|id| id == "viewer.float"),
        "and standing in the lab it is not, so leaving a section takes its \
         floating panels with it: {ids:?}"
    );
}

/// ★★★★★ §2 #2 — which destinations are whole screens is on the wire.
///
/// `Destinations::wire` cannot answer it: a destination's page being another
/// binding is a fact about the PAIRING, not about the destination. Left
/// unpublished, a client infers it from the tag prefixes it happens to see,
/// which is a rule nobody wrote down.
#[test]
fn r1724_the_roster_publishes_which_destinations_are_screens() {
    let roster = roster();
    let wire = roster.wire(&at("catalog"));
    assert_eq!(
        wire["at"], "catalog",
        "the journey, as `Destinations::wire`"
    );

    let rows = wire["destinations"]
        .as_array()
        .expect("the roster publishes its destinations")
        .clone();
    assert_eq!(rows.len(), 4, "every destination, mounted or not");

    let by_key = |key: &str| {
        rows.iter()
            .find(|row| row["key"] == key)
            .expect("the key is on the rail")
            .clone()
    };
    assert_eq!(by_key("catalog")["mounted"], true);
    assert_eq!(by_key("catalog")["screen"]["tag"], LAB_TAG);
    assert_eq!(by_key("catalog")["screen"]["title"], "the node graph lab");
    assert_eq!(
        by_key("dashboard")["mounted"],
        false,
        "a page the host paints itself is not a screen, and says so",
    );
    assert!(
        by_key("dashboard")["screen"].is_null(),
        "so there is nothing to address"
    );
    assert_eq!(
        by_key("dashboard")["open"],
        true,
        "and the fields `Destinations::wire` already published are unchanged",
    );
    assert_eq!(
        by_key("topology")["kind"],
        "reserved",
        "including the closure vocabulary on a seat nothing is mounted at",
    );
}

/// The title the host publishes is the screen's. The reference keeps a mounted
/// window's title and shows it nowhere.
#[test]
fn r1724_the_window_title_is_the_current_screens() {
    let roster = roster();
    assert_eq!(
        roster.current_title(&at("catalog")),
        Some("the node graph lab")
    );
    assert_eq!(
        roster.current_title(&at("stream")),
        Some("the capture viewer")
    );
}

/// A pairing that cannot be run is refused where it is written, not where it
/// paints.
#[test]
fn r1724_a_roster_refuses_a_mount_it_cannot_honour() {
    use pinion_screen::MountDefect;

    let missing = ScreenRoster::new(
        destinations(),
        vec![(
            "sessions",
            Box::new(Mount::<LabFixture>::new()) as Box<dyn Screen>,
        )],
    );
    assert_eq!(
        missing.err(),
        Some(MountDefect::NoSuchDestination {
            key: "sessions".to_owned()
        })
    );

    let shut = ScreenRoster::new(
        destinations(),
        vec![(
            "topology",
            Box::new(Mount::<LabFixture>::new()) as Box<dyn Screen>,
        )],
    );
    assert_eq!(
        shut.err(),
        Some(MountDefect::DestinationIsClosed {
            key: "topology".to_owned()
        }),
        "a seat that tells a reader the destination is unavailable, with the \
         screen mounted right there, is the application contradicting itself"
    );

    let twice = ScreenRoster::new(
        destinations(),
        vec![
            (
                "catalog",
                Box::new(Mount::<LabFixture>::new()) as Box<dyn Screen>,
            ),
            ("catalog", Box::new(Mount::<ViewerFixture>::new())),
        ],
    );
    assert_eq!(
        twice.err(),
        Some(MountDefect::DuplicateMount {
            key: "catalog".to_owned()
        })
    );
}

/// The hooks a binding overrode are the hooks the mounted screen answers. The
/// census in `coverage` proves every hook is *mirrored*; these prove the
/// mirroring *dispatches*, on the hooks whose default is a different answer.
#[test]
fn r1724_a_mounted_binding_keeps_the_hooks_it_overrode() {
    let roster = roster();
    let catalog = at("catalog");
    let stream = at("stream");
    let empty = Scene::Container(ContainerNode::new(Vec::new()));

    LAB_VALUE.with(|v| v.set(4));
    let _ = roster.latch(&catalog, &empty);

    assert_eq!(
        roster.with_current(&catalog, |screen| screen.keybinding("F5")),
        Some(Some("lab_event")),
        "a typed event crossed the roster as the name it would have become"
    );
    assert_eq!(
        roster.with_current(&catalog, |screen| screen.keybinding("F6")),
        Some(None),
        "and a chord the binding does not bind is still not bound"
    );

    let nodes = roster
        .with_current(&catalog, |screen| screen.access_node(None))
        .expect("the lab is mounted");
    assert_eq!(
        nodes.iter().map(|n| n.name.clone()).collect::<Vec<_>>(),
        // ★ R1725 — the second node is the fixture's OWN navigation, and it is
        // here because this roster declares no chrome: nothing else is
        // providing one, so the screen keeps its. The exact list is kept exact
        // rather than relaxed to a `contains`, because what this assertion is
        // for is that a node nobody expected cannot appear unnoticed.
        vec![Some("lab 4".to_owned()), Some("sections".to_owned())],
        "the accessibility tree is built from the latched projection"
    );

    assert_eq!(
        roster.with_current(&catalog, |screen| screen.focus_ring_style("anything")),
        Some(None),
        "a content surface that suppresses the framework ring still does when \
         it is a page -- the hook whose `None` is a decision"
    );
    assert!(
        roster
            .with_current(&stream, |screen| screen.focus_ring_style("anything"))
            .expect("the viewer is mounted")
            .is_some(),
        "and a screen that did not suppress it still gets one"
    );

    let before = VIEWER_DROPS.with(Cell::get);
    assert_eq!(
        roster.with_current(&stream, |screen| screen.on_file_drop("main", "/tmp/a.pcap")),
        Some(true),
        "a file dropped on the application reaches the screen that is showing"
    );
    assert_eq!(VIEWER_DROPS.with(Cell::get), before + 1);
    assert_eq!(
        roster.with_current(&catalog, |screen| screen
            .on_file_drop("main", "/tmp/a.pcap")),
        Some(false),
        "and the screen that is not showing is not offered it"
    );
    assert_eq!(
        VIEWER_DROPS.with(Cell::get),
        before + 1,
        "measured rather than inferred: the viewer's counter did not move"
    );

    // A hook neither fixture overrides still answers, so a mount is total
    // rather than only forwarding what somebody remembered.
    assert_eq!(
        roster.with_current(&catalog, |screen| screen
            .dock_drop_preview("panel", "target", Rect::new(0, 0, 10, 10), 0.5, 0.5)
            .is_none()),
        Some(true)
    );
    assert_eq!(
        roster.with_current(&catalog, |screen| screen.windows_signal().is_some()),
        Some(false)
    );
}

/// A mount that has never been latched answers from an empty scene rather than
/// panicking: a host laying out a rail seat may ask a page what it is before
/// anybody has gone there.
#[test]
fn r1724_an_unlatched_mount_reads_an_empty_scene() {
    LAB_VALUE.with(|v| v.set(0));
    let mount: Mount<LabFixture> = Mount::new();
    assert_eq!(mount.fmt_state_log(), "lab at 0");
    assert_eq!(mount.tag(), LAB_TAG);
    assert_eq!(mount.title(), "the node graph lab");
}

/// ★★★★★ R1724 — **a region owes a screen the recourse it declared.**
///
/// The lab fixture declares `Recourse::Pan` with a comfortable size wider than
/// the region it is placed in, so its scene comes back wrapped in a viewport of
/// the region's size. Measured on the real mount before this existed: 51 of the
/// node lab's regions painted outside the rectangle it was placed in, its
/// inspector running past the window's right edge.
#[test]
fn r1724_a_screen_too_big_for_its_region_pans_inside_it() {
    let roster = roster();
    let owner = Owner::new();
    VIEWPORT_SIZE.resolve(&owner).set((1920, 1080));

    // Wide enough for the lab's comfortable size: nothing to pan over, and the
    // scene comes back exactly as the screen painted it.
    let roomy = owner
        .run(|| {
            roster.page_scene(
                &at("catalog"),
                (LAB_COMFORTABLE.0 + 40, 600),
                &Frame::default(),
            )
        })
        .expect("the lab is mounted");
    assert!(
        matches!(roomy, Scene::Container(_)),
        "a screen that fits is handed back untouched, not wrapped for nothing"
    );

    // Narrower than it: the region becomes the viewport and the screen the
    // content, which is what `Recourse::Pan` means.
    let tight = owner
        .run(|| {
            roster.page_scene(
                &at("catalog"),
                (LAB_COMFORTABLE.0 - 400, 600),
                &Frame::default(),
            )
        })
        .expect("the lab is mounted");
    let Scene::Scroll(node) = &tight else {
        panic!("a screen that does not fit its region pans inside it; got {tight:?}");
    };
    assert_eq!(
        (node.viewport.w, node.viewport.h),
        (LAB_COMFORTABLE.0 - 400, 600),
        "the viewport is the region, so nothing is painted outside it",
    );

    // A screen that declares nothing gets nothing done to it.
    let plain = owner
        .run(|| roster.page_scene(&at("stream"), (10, 10), &Frame::default()))
        .expect("the viewer is mounted");
    assert!(
        matches!(plain, Scene::Container(_)),
        "no declaration, no recourse -- `pan` is the identity"
    );
}

/// The extent grant does not leak past the host's scene: after `page_scene`
/// returns, a read of the same tag is the ordinary one again.
#[test]
fn r1724_the_grant_is_scoped_to_the_hosts_scene() {
    let roster = roster();
    let owner = Owner::new();
    VIEWPORT_SIZE.resolve(&owner).set((1920, 1080));
    owner.run(|| {
        let _ = roster.page_scene(&at("catalog"), (640, 480), &Frame::default());
    });
    assert_eq!(
        pinion_core::external::granted_surface_extent(LAB_TAG),
        None,
        "no grant stands once the page is painted"
    );
    assert_eq!(
        owner.run(|| layout_size(LAB_TAG, (10, 10), (1440, 900))),
        (1920, 1080),
        "so an un-granted read is what it always was"
    );
}

/// The screens a roster holds are reachable only through a journey — there is
/// no expression in this crate that hands out one the application is not
/// showing. `Rc` is unused elsewhere; this keeps the import honest by pinning
/// that the roster is shareable the way a host's owner cache holds it.
#[test]
fn r1724_a_roster_is_held_the_way_a_host_holds_it() {
    let roster = Rc::new(roster());
    let shared = Rc::clone(&roster);
    assert_eq!(shared.destinations().len(), 4);
    assert_eq!(
        shared.mounted_keys().count(),
        2,
        "two of the four destinations are screens; the other two are the \
         host's own page and a locked seat"
    );
}

// --- R1738: what the assembled application can say about itself ---------------

/// ★★★★★ The population is the ROSTER's, so a section is missing from the
/// report only by not being in the application.
///
/// The failing shape this replaces was measured on the real tool: its published
/// conformance was a verdict about the eight navigation seats, and four of its
/// six open sections had never been compared with anything — not failing a
/// check, absent from the population, with nothing anywhere saying so.
#[test]
fn r1738_every_destination_is_a_row_whether_or_not_anything_judged_it() {
    let said = roster().conformance(&at("dashboard"));
    assert_eq!(
        said.rows()
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        ["dashboard", "catalog", "stream", "topology"],
        "one row per destination, in the roster's own order"
    );
    assert_eq!(said.sections(), 4);
    assert_eq!(said.judged(), 1, "the lab fixture answers a specification");
    assert_eq!(
        said.unjudged(),
        2,
        "the viewer publishes no verdict and the dashboard has no screen at all"
    );
    assert_eq!(said.closed(), 1);
}

/// The four standings are four different facts and the roster tells them apart.
#[test]
fn r1738_a_section_that_answers_nothing_is_a_row_and_not_a_silence() {
    let said = roster().conformance(&at("dashboard"));
    let standing = |key: &str| {
        said.rows()
            .iter()
            .find(|row| row.key == key)
            .map(|row| row.standing.word())
            .expect("the fixture holds it")
    };
    assert_eq!(standing("catalog"), "judged");
    assert_eq!(
        standing("stream"),
        "unspecified",
        "a screen is mounted and it publishes no verdict"
    );
    assert_eq!(
        standing("dashboard"),
        "inline",
        "no screen is mounted, so this roster has nothing to ask — which is a \
         different fact from a screen that answers nothing, and the work that \
         closes it is different too"
    );
    assert_eq!(standing("topology"), "closed");
}

/// ★★★★★ The rule the type exists for: an application does not get to report
/// conformance on the strength of the sections somebody wrote a specification
/// for.
#[test]
fn r1738_an_application_with_unjudged_sections_does_not_conform() {
    let said = roster().conformance(&at("dashboard"));
    assert!(
        !said.conforms(),
        "two of its open sections were never judged, and that is part of the \
         verdict rather than a footnote under it"
    );
    let judged: Vec<_> = said
        .rows()
        .iter()
        .filter(|row| row.standing.is_judged())
        .collect();
    assert_eq!(judged.len(), 1);
    let SectionStanding::Judged(report) = &judged[0].standing else {
        panic!("it was just filtered for");
    };
    assert!(
        report.reconciles(),
        "the one section that IS judged has exactly the difference its ledger \
         declares — so the application failing to conform is about coverage, \
         not about that section"
    );
    assert_eq!((report.specified(), report.reproduced()), (3, 2));
    assert_eq!(
        (said.specified(), said.reproduced()),
        (3, 2),
        "the application's totals are its judged sections added up"
    );
}

/// ★★★★★ R1758 — **a verdict a screen read off its own tables is not
/// conformance**, and the application counts those separately from the ones it
/// never judged at all.
///
/// The rule R1742 settled and left as prose in one screen's header. Measured on
/// this tree's analysis tool at R1747 and again at R1758: two of its four judged
/// sections reported every part of their specification reproduced from a page
/// where they had not painted a frame, because the roster they answered with was
/// a copy of the specification. Nothing failed and no count told those two from
/// the two answering honestly.
///
/// The fixture beside this one answers with
/// [`SpecDocument::report`](pinion_core::conformance::SpecDocument::report),
/// which is the entry point that hands a screen nothing — so it is stamped
/// `declaration`, and this asserts what an application does about that.
///
/// ⚠ Sharpened into a roster where **nothing else is wrong**, because the
/// four-seat fixture already fails to conform for having unjudged pages: a test
/// on that roster would pass whether or not the evidence rule exists. Here the
/// only open destination holds the judged screen, whose difference is exactly
/// what its ledger declares — so `conforms()` turns on this rule alone.
#[test]
fn r1758_a_verdict_from_a_screens_own_tables_is_not_conformance() {
    let one_seat = Destinations::new(vec![Destination::open("catalog", "Catalog")])
        .expect("one open destination is a rail");
    let roster = ScreenRoster::new(
        one_seat,
        vec![(
            "catalog",
            Box::new(Mount::<LabFixture>::new()) as Box<dyn Screen>,
        )],
    )
    .expect("the fixture screen is mounted at an open destination");
    let said = roster.conformance(&at_one("catalog"));

    assert_eq!(said.unjudged(), 0, "nothing here is unjudged");
    let SectionStanding::Judged(report) = &said.rows()[0].standing else {
        panic!("the only seat holds the judged fixture");
    };
    assert!(
        report.reconciles(),
        "and its difference is exactly the one its ledger declares, so no other \
         rule can be what makes this application fail to conform"
    );

    assert_eq!(
        report.evidence(),
        pinion_core::conformance::Evidence::Declaration,
        "the fixture answers from its own tables, and the verdict says so"
    );
    assert_eq!(said.declared(), 1);
    assert!(
        !said.conforms(),
        "★★★★★ a screen agreeing with its own tables is not evidence that it \
         reproduces anything, so the application does not report conformance on \
         the strength of it"
    );

    let wire = said.to_json();
    assert_eq!(wire["declared"], 1, "and a client reads the count too");
    assert_eq!(wire["conforms"], false);
    assert_eq!(wire["rows"][0]["conformance"]["evidence"], "declaration");
}

/// A one-destination journey, for the roster above.
fn at_one(key: &str) -> Journey {
    let one_seat = Destinations::new(vec![Destination::open("catalog", "Catalog")])
        .expect("one open destination is a rail");
    Journey::begin(&one_seat, key).expect("the fixture opens at a real destination")
}

/// A row carries the tag its section is addressed by exactly when a screen is
/// mounted there — so a reader of the report can go and ask the section itself
/// without a mapping nobody published.
#[test]
fn r1738_a_row_says_how_to_reach_the_section_it_is_about() {
    let said = roster().conformance(&at("dashboard"));
    let tag = |key: &str| {
        said.rows()
            .iter()
            .find(|row| row.key == key)
            .and_then(|row| row.tag.clone())
    };
    assert_eq!(tag("catalog").as_deref(), Some(LAB_TAG));
    assert_eq!(tag("stream").as_deref(), Some(VIEWER_TAG));
    assert_eq!(tag("dashboard"), None, "the host paints it itself");
    assert_eq!(
        tag("topology"),
        None,
        "nothing can be mounted at a closed seat"
    );
}

/// A closed destination's row carries the destination's OWN reason, not a
/// second wording of the closure written beside it.
#[test]
fn r1738_a_closed_seat_reports_the_reason_the_destination_gives() {
    let said = roster().conformance(&at("dashboard"));
    let row = said
        .rows()
        .iter()
        .find(|row| row.key == "topology")
        .expect("the fixture holds it");
    let why = row.standing.why().expect("a closed seat says why");
    assert!(
        why.contains("requirement 12"),
        "the destination's own sentence, verbatim: {why}"
    );
}

/// The published value carries the counts rather than leaving a client to
/// recompute them, because a client that recomputes them can disagree with the
/// application about how much of it was judged.
#[test]
fn r1738_the_published_report_carries_its_own_counts() {
    let said = roster().conformance(&at("dashboard"));
    let wire = said.to_json();
    assert_eq!(wire["sections"], 4);
    assert_eq!(wire["judged"], 1);
    assert_eq!(wire["unjudged"], 2);
    assert_eq!(wire["closed"], 1);
    assert_eq!(wire["conforms"], false);
    let rows = wire["rows"]
        .as_array()
        .expect("the report publishes its rows");
    assert_eq!(
        rows.len(),
        said.sections(),
        "the wire cannot hold fewer rows"
    );
    assert!(
        rows.iter()
            .filter(|row| row["standing"] != "judged")
            .all(|row| row.get("why").is_some()),
        "every row that is not judged publishes its reason"
    );
}

/// ★★★★★ R1742 — **every row says which frame its verdict is about.**
///
/// Found by running the assembled tool, not by design. A section that derives
/// its verdict from its own paint answers about its LAST frame, and the paint
/// store keeps that frame after the section stops showing — so read from
/// another page, the node lab's row reported a surface *standing* while nothing
/// of it was on screen. Nothing said which frame the number was about.
///
/// The population is deliberately unchanged: every destination is still a row
/// wherever the reader stands, because withholding one would put the section
/// back outside the population, which is the defect R1738 repaired. What is
/// added is the fact a reader needs to interpret the number beside it.
#[test]
fn r1742_a_row_says_whether_its_section_was_the_one_showing() {
    let here = roster().conformance(&at("catalog"));
    let showing: Vec<(&str, bool)> = here
        .rows()
        .iter()
        .map(|row| (row.key.as_str(), row.showing))
        .collect();
    assert_eq!(
        showing,
        [
            ("dashboard", false),
            ("catalog", true),
            ("stream", false),
            ("topology", false),
        ],
        "exactly the destination the journey is at is showing",
    );

    // ★ And it moves with the reader rather than with the report: the same
    // roster read from elsewhere marks a different row, which is what makes
    // this a fact about the frame and not a property of the section.
    let elsewhere = roster().conformance(&at("dashboard"));
    assert!(elsewhere.rows()[0].showing && !elsewhere.rows()[1].showing);
    assert_eq!(
        elsewhere.judged(),
        here.judged(),
        "★ and the verdicts themselves are unchanged -- this labels the report, \
         it does not narrow it",
    );
    assert_eq!(elsewhere.to_json()["rows"][1]["showing"], false);
    assert_eq!(here.to_json()["rows"][1]["showing"], true);
}
