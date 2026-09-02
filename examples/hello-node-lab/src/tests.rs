//! R1651 — the screen against its own specification, without a window.
//!
//! The demo drives the real application through the wire and a real pointer;
//! these are the claims that do not need a process, and the ones that would be
//! expensive to notice only from a running screen: that the opening state IS
//! the specification, that the gate is derived, and that the two halves of the
//! screen agree about what a node is.

use pinion_core::reactive::Owner;
use pinion_core::selection::Selection;
use pinion_core::widgets::config_form::Applies;

use super::{
    Hit, INSP_W, LabState, MIN_W, PALETTE_W, PANEL_STRIP_W, RAIL_W, TOOLBAR_LEFT_CLUSTER, ZOOM_MAX,
    ZOOM_MIN, canvas_rect, card_rect, card_shape_at, content_to_window, deploy, inspector_rect,
    palette_rect, pin_rect, scenario, spec, use_lab_state,
};
use crate::graph::{Role, Transport};
use pinion_node_graph::{Admission, NodeBody, ROOT};
use std::collections::BTreeSet;

/// R1788 — the plan's document as a value.
///
/// The framework renders it as the artifact it is, which is text; a test that
/// wants to index into it parses it back. Going through the text is deliberate
/// rather than a convenience wrapper around an internal: it means every
/// assertion below is made against **what actually leaves the screen**.
fn plan_document(plan: &deploy::Plan) -> serde_json::Value {
    let text = plan.to_document().expect("the plan's configurations write");
    serde_json::from_str(&text).expect("what we just wrote is JSON")
}

fn state() -> LabState {
    LabState::opening()
}

/// ★★★★★ R1726 — **a card you picked up stays in front after you put it down.**
///
/// The owner's report, and the half a transient lift does not answer. Measured
/// on the running screen before this existed: a dragged card painted at index
/// 101 while held and went straight back to 70 the moment it was released, so
/// the card just placed was the hidden one under the card it was dropped on.
///
/// Asserted over the card ORDER rather than over a picture, because that order
/// is the z-order: the scene paints depth-first and the hit test walks the same
/// children in reverse. And the second assertion is the one that keeps this a
/// reorder rather than a rearrangement — a drop on a free canvas must displace
/// nothing, which is the rule every node editor keeps and the opposite of the
/// tile dashboard's.
#[test]
fn r1726_a_card_you_picked_up_stays_in_front_after_you_drop_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = state();
        let before = state.cards();
        assert!(before.len() >= 3, "the opening graph has cards to stack");
        let first = before[0];
        let last = *before.last().expect("a last card");

        state.raise(first);
        let after = state.cards();
        assert_eq!(
            after.last().copied(),
            Some(first),
            "the card that was picked up is now the front one, and stays there \
             -- nothing about this is tied to a gesture still being held"
        );
        assert_eq!(
            after.len(),
            before.len(),
            "raising adds and removes no card"
        );

        // Raising another puts THAT one in front and keeps the first ahead of
        // the untouched ones: the order is a history of what has been handled.
        state.raise(last);
        let after = state.cards();
        assert_eq!(after.last().copied(), Some(last));
        let positions = |id| after.iter().position(|c| *c == id).expect("still a card");
        assert!(
            positions(first) > positions(before[1]),
            "a card picked up earlier is still in front of one never touched"
        );

        // ★ And a card nobody has touched is exactly where the specification
        // puts it, so the screen still OPENS as declared.
        let untouched: Vec<_> = before
            .iter()
            .copied()
            .filter(|id| *id != first && *id != last)
            .collect();
        let still: Vec<_> = after
            .iter()
            .copied()
            .filter(|id| untouched.contains(id))
            .collect();
        assert_eq!(
            still, untouched,
            "the cards nobody picked up keep their declared order"
        );
    });
}

/// ★★★★★ R1725 — **this screen draws its own navigation only where it is the
/// one providing it, and every rectangle follows from that one fact.**
///
/// Driven here rather than only from the shell because the property is this
/// screen's, and because the direction that must NOT change is the standalone
/// one: this binding still runs as its own window, where its rail is the only
/// navigation there is. A guard that got the sense backwards would take the
/// rail away from the standalone screen, and every rectangle assertion in
/// `painted.rs` is written against the standalone layout — so this pins the
/// hosted side, which nothing there can see.
#[test]
fn r1725_the_rail_is_drawn_only_where_this_screen_is_the_one_providing_it() {
    use pinion_core::chrome::{HostChrome, Part, with_host_chrome};

    let owner = Owner::new();
    owner.run(|| {
        // Standalone: the rail is this screen's to draw, and the palette sits
        // beside it exactly where it always did.
        assert!(super::draws_own_rail(), "nothing is providing a navigation");
        assert_eq!(super::rail_w(), RAIL_W);
        assert_eq!(
            super::palette_rect().x,
            RAIL_W,
            "the palette starts after the rail it drew"
        );

        // Placed inside an application that already has one.
        with_host_chrome(HostChrome::NONE.with(Part::Navigation), || {
            assert!(!super::draws_own_rail());
            assert_eq!(super::rail_w(), 0, "no width is reserved for it");
            assert_eq!(
                super::palette_rect().x,
                0,
                "and the room is USED -- the palette moves to the page's own \
                 left edge rather than leaving a blank strip where a rail was"
            );
            assert_eq!(
                super::rail_rect().w,
                0,
                "the rail's own rectangle is empty, so nothing can be laid out \
                 into it by a reader that asks"
            );
        });

        // …and the declaration does not leak back out.
        assert!(super::draws_own_rail());
        assert_eq!(super::palette_rect().x, RAIL_W);
    });
}

/// ★★★★★ R1822 — **the application bar is drawn only where this screen is the
/// one providing it**, and every rectangle above it follows from that one fact.
///
/// The rail's twin, 97 rounds later. Written the same way and for the same
/// reason: the direction that must NOT change is the standalone one, because
/// every rectangle assertion in `painted.rs` is written against a screen that
/// draws its own bar, and a guard with the sense backwards would take the bar
/// away from the binding that still runs as its own window.
#[test]
fn r1822_the_app_bar_is_drawn_only_where_this_screen_is_the_one_providing_it() {
    use pinion_core::chrome::{HostChrome, Part, with_host_chrome};

    let owner = Owner::new();
    owner.run(|| {
        assert!(super::draws_own_app_bar(), "nothing provides one");
        assert_eq!(super::app_bar_h(), super::APP_BAR_H);
        assert_eq!(
            super::toolbar_rect().y,
            super::APP_BAR_H,
            "the toolbar starts under the bar it drew"
        );

        with_host_chrome(HostChrome::NONE.with(Part::ApplicationBar), || {
            assert!(!super::draws_own_app_bar());
            assert_eq!(super::app_bar_h(), 0, "no height is reserved for it");
            assert_eq!(
                super::toolbar_rect().y,
                0,
                "and the room is USED -- the toolbar moves to the page's own top \
                 edge rather than leaving a blank strip where a bar was"
            );
            assert_eq!(
                super::rail_rect().y,
                0,
                "the rail starts at the top too, so the two panes cannot \
                 disagree about where this page begins"
            );
        });

        // …and the declaration does not leak back out.
        assert!(super::draws_own_app_bar());
        assert_eq!(super::toolbar_rect().y, super::APP_BAR_H);
    });
}

/// ★★★★★ R1822 — **the app bar's pane is not IN the scene where the host draws
/// one**, which is the round's headline mechanism and had no gate until a
/// counterfactual said so.
///
/// 🟥🟥🟥 This test exists because CF-1 **PASSED**: the app-bar pane was built
/// unconditionally again and the whole suite stayed green. Three tests asserted
/// the things that FOLLOW from the bar being absent — its reserved height, its
/// accessibility landmark, the silence that defers to it — and not one asserted
/// the bar itself. Each derived assertion is satisfiable while the pane is
/// still painted, so the mechanism the debt is about was the one thing nothing
/// checked.
///
/// ⇒ ★★★★★ assert the MECHANISM, not only what it implies. The implications
/// were the easy assertions to write, which is exactly why they were the ones
/// that got written.
///
/// R1725's paragraph is the standard this holds the pane to: *not painted-and-
/// hidden and not zero-height — the node must not exist*, because that is the
/// only form of the claim a reader cannot trip over.
#[test]
fn r1822_the_app_bars_pane_is_absent_from_the_scene_the_host_draws_one_in() {
    use pinion_core::chrome::{HostChrome, Part, with_host_chrome};
    use pinion_core::voice::voice_census;

    fn tags() -> Vec<String> {
        use pinion_core::widgets::text_field::TextFieldState;
        let scene = super::view((TextFieldState::Idle, 0), pinion_core::Frame::default());
        voice_census(
            &scene,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeSet::new(),
        )
        .nodes
        .into_iter()
        .map(|n| n.tag)
        .collect()
    }

    let owner = Owner::new();
    owner.run(|| {
        let standalone = tags();
        assert!(
            standalone.iter().any(|t| t == "lab.appbar"),
            "★ the screen that owns its window still paints its own bar"
        );

        with_host_chrome(HostChrome::NONE.with(Part::ApplicationBar), || {
            let mounted = tags();
            assert!(
                !mounted.iter().any(|t| t.starts_with("lab.appbar")),
                "★★★★★ and where the host draws one, NO node of it is in the \
                 scene -- not the bar, not the graph label inside it, not the \
                 run-state label. Absent, not hidden and not zero-height: {:?}",
                mounted
                    .iter()
                    .filter(|t| t.starts_with("lab.appbar"))
                    .collect::<Vec<_>>()
            );
            assert!(
                mounted.iter().any(|t| t == "lab.toolbar.title"),
                "★ and the page it leaves behind is still the node lab"
            );
        });
    });
}

/// ★★★★★ R1822 — **the height floor drops BOTH strips the host provides, not
/// just the one this round is named after.**
///
/// The floor is a `max` of two terms — what the content needs, and what the
/// rail's seats need — and standalone the **rail's wins**. So a page mounted
/// where the host draws the navigation AND the application bar needs neither
/// term's chrome, and the answer is the content term with no bar in it.
///
/// 🟥 This round's draft subtracted `APP_BAR_H` flat, the way the width axis
/// subtracts `RAIL_W`, and was wrong by exactly the amount the rail term exceeds
/// the content term — charging a mounted page for rail seats that are not on it.
/// The width has no `max` in it, which is why copying the width's form did not
/// survive being measured.
///
/// ⚠ **The figures that used to be in this paragraph were wrong**, and the
/// closing audit caught them by running the code rather than re-reading it: it
/// said *368 over 360* and *8 pixels*, which is the rail floor of a SEVEN-seat
/// rail — the count R1773 found drifted and restored to eight. The assertions
/// below therefore state the relation and print the amount, so no reader has to
/// trust a number in prose again.
#[test]
fn r1822_the_height_floor_drops_every_strip_the_host_provides() {
    use pinion_core::chrome::{HostChrome, Part, with_host_chrome};

    let owner = Owner::new();
    owner.run(|| {
        assert_eq!(
            super::layout_min_h(),
            super::MIN_H,
            "★ standalone the derivation and the window policy are one number"
        );
        assert_eq!(
            super::comfortable_size().1,
            super::MIN_H,
            "★ so nothing is subtracted from the screen that owns its window"
        );
        // ★★★★★ The premise the rest of this test rests on, asserted rather
        // than written into a comment: standalone it is the RAIL term that wins
        // the `max`. A round that read this off prose got the arithmetic of a
        // seven-seat rail and published it three times.
        let content = super::APP_BAR_H + super::TOOLBAR_H + super::CANVAS_FLOOR;
        assert!(
            super::MIN_H > content,
            "★ the rail term wins standalone: MIN_H {} against a content floor \
             of {content}",
            super::MIN_H
        );

        let host = HostChrome::NONE
            .with(Part::Navigation)
            .with(Part::ApplicationBar);
        with_host_chrome(host, || {
            assert_eq!(
                super::layout_min_h(),
                super::TOOLBAR_H + super::CANVAS_FLOOR,
                "★★★★★ mounted, neither strip is this page's, so the floor is \
                 what the CONTENT needs and nothing else -- not the rail term \
                 with a bar's height taken off it"
            );
            let flat = super::MIN_H - super::APP_BAR_H;
            assert!(
                super::comfortable_size().1 < flat,
                "★★★★★ and it is strictly LOWER than a flat subtraction of the \
                 bar would give -- {flat} against {}, the {} pixels the draft \
                 got wrong, which is exactly what the rail term exceeds the \
                 content term by",
                super::comfortable_size().1,
                flat - super::comfortable_size().1
            );
        });
    });
}

/// ★★★★★ R1822 — **the graph's name is announced by exactly one stop, in both
/// configurations**, which is the half of this round a rectangle cannot show.
///
/// `lab.toolbar.title` is silenced `name_of("lab.appbar")`: a silence is a
/// REFERENCE to the node that does say the word. Where the host draws the
/// application bar this screen draws none — so left alone, that reference
/// points at a node that is not in the tree and the graph's name is announced
/// by NOBODY, while still being painted.
///
/// ⚠ This is the failure mode the round had to go looking for. It is invisible
/// to every pixel assertion, invisible to the layout, and the naive repair —
/// delete the pane — produces it silently.
#[test]
fn r1822_the_graphs_name_is_announced_by_one_stop_in_both_configurations() {
    use pinion_core::chrome::{HostChrome, Part, with_host_chrome};

    use pinion_core::voice::{Announcement, Voice, voice_census};

    /// The census VERDICT on `lab.toolbar.title`, judged against the tree this
    /// screen actually publishes.
    ///
    /// 🟥 ★★★★★ R1825 — this used to answer `silence.is_some()`, and that is
    /// why it passed while the region was broken. `is_some()` cannot tell
    /// *declares a name* from *declares nothing*: R1822 dropped the deferral
    /// where the bar is absent and the node became `Unvoiced`, which this
    /// helper reported as "not quiet" — the answer it wanted. The running
    /// application's own census found it (one undecided region at the lab
    /// destination) two rounds later.
    ///
    /// ⇒ **assert the smallest thing that changed, but assert the RIGHT
    /// smallest thing** — a verdict has three arms here and a boolean has two.
    /// The tree is passed in for the same reason: a silence is a REFERENCE, so
    /// a census run against an empty tree cannot tell whether the node it
    /// points at exists.
    fn title_voice(state: &super::LabState) -> pinion_core::voice::Voice {
        let theme = super::use_theme(super::THEME_TAG).theme_animated();
        let scene = super::toolbar(state, super::ink(&theme));
        let announced: std::collections::BTreeMap<String, Announcement> =
            super::appbar_access(state)
                .into_iter()
                .map(|n| {
                    (
                        n.tag.clone(),
                        Announcement {
                            name: n.name.clone().unwrap_or_default(),
                            name_required: false,
                            live: false,
                            composes: Vec::new(),
                        },
                    )
                })
                .collect();
        voice_census(&scene, &announced, &std::collections::BTreeSet::new())
            .nodes
            .iter()
            .find(|n| n.tag == "lab.toolbar.title")
            .expect("the toolbar paints the graph's name")
            .voice
    }

    let owner = Owner::new();
    owner.run(|| {
        let state = state();

        // Standalone the bar is the stop that says it, so the toolbar's copy
        // defers to it and the tree holds the bar.
        assert!(super::draws_own_app_bar());
        let landmarks: Vec<String> = super::appbar_access(&state)
            .into_iter()
            .map(|n| n.tag)
            .collect();
        assert!(
            landmarks.iter().any(|t| t == "lab.appbar"),
            "★ the bar it draws is a landmark a reader can reach"
        );
        assert_eq!(
            title_voice(&state),
            Voice::Silent,
            "★ and the toolbar's copy of the name defers to it"
        );

        with_host_chrome(HostChrome::NONE.with(Part::ApplicationBar), || {
            let landmarks: Vec<String> = super::appbar_access(&state)
                .into_iter()
                .map(|n| n.tag)
                .collect();
            assert!(
                !landmarks.iter().any(|t| t == "lab.appbar"),
                "★★★★★ where it draws no bar it offers no landmark for one -- \
                 not an empty group, not a zero-height strip: absent, which is \
                 the only form of that claim a reader cannot trip over"
            );
            assert_eq!(
                title_voice(&state),
                Voice::Announced,
                "★★★★★ and the toolbar's copy becomes the stop that SAYS the \
                 name. 🟥 R1825: it used to only stop DEFERRING, and this \
                 assertion used to read `silence.is_some()`, which cannot tell \
                 `declares a name` from `declares nothing` -- so the node went \
                 Unvoiced and the test agreed. A verdict has three arms here \
                 and a boolean has two"
            );
        });
    });
}

/// ★★★★★ R1716 — **a card told where it runs runs there, in the plan.**
///
/// The screen shows the placement row, the row can be taken over, and the
/// launch plan is a different reader of the same fact — which is exactly the
/// shape this round was opened by (a plan that answered `unplaced` for every
/// card while the canvas drew two host frames). A demo caught this pair
/// disagreeing after the take-over; this pins it one layer down, where the
/// failure is one function rather than a screen.
#[test]
fn r1716_a_card_told_where_it_runs_runs_there_in_the_plan() {
    let owner = Owner::new();
    owner.run(|| {
        let state = state();
        let node = state.active_card().expect("the screen opens on a card");
        assert_eq!(
            state.host_of(node),
            "host-a",
            "it opens in the frame it is drawn in"
        );

        super::amend(&state, node, |form| form.author("host")).expect("derived");
        super::amend(&state, node, |form| form.set("host", "host-c")).expect("theirs now");

        let shown = super::shown_form(&state, node).expect("a form");
        assert_eq!(
            shown
                .field("host")
                .map(|f| f.value().into_owned())
                .as_deref(),
            Some("host-c")
        );
        assert_eq!(
            state.host_of(node),
            "host-c",
            "★ the one walk everything asks reads the row somebody wrote"
        );
        let plan = state.plan();
        let placed: Vec<(&str, &str)> = plan
            .nodes()
            .iter()
            .map(|entry| (entry.name.as_str(), entry.host.as_str()))
            .collect();
        assert!(
            placed.contains(&("R-01", "host-c")),
            "★★ and the PLAN runs it there: {placed:?}"
        );
        assert!(
            placed.contains(&("P-01", "host-a")),
            "while a card nobody told still runs where it is drawn: {placed:?}"
        );
    });
}

/// Take the connect row over and write one address of your own over the seed —
/// the two acts a person performs to reach a row with two contributors.
///
/// It is written as the screen's own operations rather than by building the
/// state, because a state a test invents can be one no session reaches: the
/// only door into this on the real screen is the take-over seat followed by a
/// commit into the box it leaves behind.
fn share_the_connect_row(state: &std::rc::Rc<LabState>, address: &str) -> Vec<String> {
    let node = state.active_card().expect("the screen opens on a card");
    let drawn: Vec<String> = super::shown_form(state, node)
        .and_then(|form| {
            form.field("connect.endpoints")
                .map(|f| f.value().into_owned())
        })
        .map(|value| {
            pinion_core::widgets::config_form::FieldType::elements(&value)
                .map(str::to_owned)
                .collect()
        })
        .expect("the canvas draws links out of the opening card");
    super::author_row(state, node, "connect.endpoints").expect("the wires derive it");
    super::amend(state, node, |form| form.set("connect.endpoints", address))
        .expect("theirs to write now");
    drawn
}

/// ★★★★★ R1717 — **one key, two contributors.** A card may be told to dial an
/// address this canvas does not draw, and every address it DOES draw still
/// reaches the row and the exported configuration.
///
/// R1716 answered the first half by letting a written value take the whole row
/// and paid for the second with a gate warning. This pins the payment gone,
/// one layer under the demo: the failure is a form rather than a screen.
#[test]
fn r1717_a_written_address_and_every_drawn_one_stand_in_one_row() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let node = state.active_card().expect("the screen opens on a card");
        let outside = "tcp/10.0.0.21:7449";
        let drawn = share_the_connect_row(&state, outside);
        assert!(
            drawn.len() >= 2,
            "the opening card is drawn to more than one peer: {drawn:?}"
        );
        let form = super::shown_form(&state, node).expect("a form");
        let row = form.field("connect.endpoints").expect("held");
        assert_eq!(
            row.written(),
            Some(outside),
            "★ their half is exactly what they typed"
        );
        let value = row.value();
        let shown: Vec<&str> =
            pinion_core::widgets::config_form::FieldType::elements(&value).collect();
        assert_eq!(
            shown[0], outside,
            "★★ written first — the half a person is looking for is the one \
             they meet first"
        );
        for address in &drawn {
            assert!(
                shown.contains(&address.as_str()),
                "★★★★★ the canvas still reaches the row: {address} missing from {shown:?}"
            );
        }
        assert_eq!(
            row.derived_elements(),
            drawn.len(),
            "★★ and the row says how many of them are the canvas's"
        );
        let document = form.document().expect("shippable");
        assert_eq!(
            document["connect"]["endpoints"],
            serde_json::Value::Array(
                shown
                    .iter()
                    .map(|a| serde_json::Value::String((*a).to_string()))
                    .collect()
            ),
            "★★★★★ and the exported configuration ships both — the picture and \
             the file saying the same thing is the whole point"
        );
    });
}

/// ★★★★★ R1717 — the gate's surviving warning: an address **somebody wrote**
/// that nothing on this canvas listens on.
///
/// R1716 warned about the mirror image — a drawn link the card did not dial —
/// which was compensation for a row that could not hold two contributions.
/// That warning is now unreachable by construction, and this is the fact
/// underneath it: the graph is no longer the whole picture, so anything
/// concluded from it is being concluded from a partial one.
#[test]
fn r1717_an_address_outside_the_graph_warns_and_does_not_block() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let node = state.active_card().expect("the screen opens on a card");
        let opening = state.defects();
        let outside = "tcp/10.0.0.21:7449";
        let drawn = share_the_connect_row(&state, outside);
        let lines: Vec<String> = state
            .gate_lines()
            .into_iter()
            .map(|(_, sentence)| sentence)
            .collect();
        let said = lines.join(" | ");
        assert!(
            said.contains(outside) && said.contains("nothing here listens on"),
            "★★★ the address nothing here listens on is named: {said}"
        );
        // ★★★★★ R1717 — and the SENTENCE READS. R1716 smuggled this warning
        // inside an unknown-key defect whose key was really a sentence, so the
        // panel said "R-01 · R-01 · … is outside this graph is not a key the
        // target knows" for a whole round with every gate green — because
        // every check asked whether the address was NAMED. A photograph of the
        // panel answered it in one look; this is that question, in a test.
        let about: Vec<&String> = lines.iter().filter(|line| line.contains(outside)).collect();
        assert_eq!(about.len(), 1, "one line is about it: {said}");
        assert_eq!(
            about[0].matches("R-01").count(),
            1,
            "★★★★★ and the card is named ONCE in it: {}",
            about[0]
        );
        assert!(
            !about[0].contains("is not a key the target knows"),
            "★★ with a sentence about the graph, not about an unknown key: {}",
            about[0]
        );
        for address in &drawn {
            assert!(
                !said.contains(address.as_str()),
                "★★★★★ and no DRAWN address is — they are in the row by \
                 construction, so R1716's warning could never fire again: \
                 {address} in {said}"
            );
        }
        assert!(
            state.verdict().may_launch(),
            "★★ it warns and does not block — a node may legitimately be told \
             to reach an already-running peer"
        );
        super::amend(&state, node, |form| form.remove("connect.endpoints"))
            .expect("their half goes");
        let back = super::shown_form(&state, node).expect("a form");
        let row = back.field("connect.endpoints").expect("★ the row STAYS");
        assert_eq!(row.written(), None, "★ and their half is gone");
        assert_eq!(
            state.defects().len(),
            opening.len(),
            "★ leaving the gate exactly as the screen opened it"
        );
    });
}

/// ★★★★★ R1718 — **the launch gate says every finding it can have, no two of
/// them read alike, and none of them names the card the panel puts in front.**
///
/// The last clause is the one this screen shipped wrong for a round. R1717's
/// `Finding::sentence` doc says the sentence comes "without the card's name —
/// the caller puts that in front, once", and before R1717 a situation carried
/// inside another type's variant put the name inside as well, so the panel read
/// `R-01 · R-01 · … is not a key the target knows`. It was a claim in a doc
/// comment and nothing read the doc; this reads it.
///
/// The arm count is the type's own, so a fifth finding fails here rather than
/// shipping unworded — which is exactly how the fourth one shipped.
///
/// ★★★★★ R1818 was the fifth, and this gate caught it within one run of the arm
/// being added: `Finding has 5 arm(s) and 4 was/were driven`. The prediction in
/// the sentence above is the measurement in this one — see the driver list in
/// the test itself, further down.
///
/// **The graph this screen opens with hands no identifier to two cards.**
///
/// ★ R1818 — worth asserting rather than assuming, because the check that makes
/// it assertable is new: until this round the identifier's SHAPE was enforced
/// and its uniqueness by nothing, so "the opening graph is clean" was not a
/// claim anybody could have made.
#[test]
fn r1818_the_opening_graph_holds_no_two_cards_to_one_identifier() {
    use pinion_core::reactive::Owner;
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_lab_state();
        let clashes: Vec<String> = state
            .gate_lines()
            .into_iter()
            .map(|(_, line)| line)
            .filter(|line| line.contains("must be unique"))
            .collect();
        assert!(
            clashes.is_empty(),
            "the graph this screen opens with hands one identifier to two \
             cards: {clashes:?}"
        );
    });
}

/// ★★★★★ And a graph that DOES is refused, by name, on both cards.
///
/// The counterfactual to the test above: without it, "no collisions" is
/// satisfied by a check that never fires.
#[test]
fn r1818_two_cards_holding_one_identifier_are_both_named() {
    use pinion_core::reactive::Owner;
    let owner = Owner::new();
    owner.run(|| {
        let state = super::use_lab_state();
        let victim = state.node_of("P-02").expect("the opening graph has it");
        let router = state.node_of("R-01").expect("the opening graph has it");
        state.selection.set(super::Selection::one(victim));
        // `a1` is what the router opens with, so writing it here makes two.
        super::set_and_sync(&state, "id", "a1");

        let said: Vec<String> = state
            .gate_lines()
            .into_iter()
            .map(|(_, line)| line)
            .filter(|line| line.contains("must be unique"))
            .collect();
        assert_eq!(
            said.len(),
            2,
            "both holders are named, so whichever card a reader is looking at \
             says so: {said:?}"
        );
        assert!(
            said.iter().any(|s| s.starts_with("P-02"))
                && said.iter().any(|s| s.starts_with("R-01")),
            "and each names the OTHER holder: {said:?}"
        );
        assert!(
            state
                .gate_lines()
                .iter()
                .any(|(blocks, line)| *blocks && line.contains("must be unique")),
            "an identifier two nodes answer to blocks the launch"
        );
        let _ = router;
    });
}

#[test]
fn r1718_every_gate_finding_is_said_distinctly_and_never_names_its_card() {
    use pinion_core::test_fixtures::speech::assert_speaks_of;
    use pinion_core::widgets::config_form::ConfigDefect;

    // A subject that could not occur inside any of these clauses by accident,
    // and the one the panel actually prefixes: a card's name.
    const CARD: &str = "R-01";

    let said = [
        (
            "Value",
            super::Finding::Value(ConfigDefect::OutOfRange {
                key: "transport.link.tx.batch_size".to_owned(),
                allowed: "0..=65535".to_owned(),
            })
            .sentence(),
        ),
        (
            "NothingListening",
            super::Finding::NothingListening.sentence(),
        ),
        ("DiscoveryOn", super::Finding::DiscoveryOn.sentence()),
        (
            "DialsOutside",
            super::Finding::DialsOutside("tcp/10.0.0.21:7449".to_owned()).sentence(),
        ),
        // ★★★★★ R1818 — the fifth situation, and the gate that demands this
        // list caught its absence within one run of the arm being added. An arm
        // nobody drives is an arm that can say anything.
        (
            "Collision",
            super::Finding::Collision {
                path: "id".to_owned(),
                value: "beef".to_owned(),
                others: vec!["P-02".to_owned()],
            }
            .sentence(),
        ),
        // ★★★★★ R1885 — the sixth situation, and this gate caught its absence
        // on the first run after the arm was added, exactly as it caught the
        // fifth. An arm nobody drives is an arm that can say anything.
        // ★★★★★ R1927 — the seventh situation, and this gate caught its absence
        // on the first run after the arm was added, exactly as it caught the
        // fifth and the sixth. Three for three: an arm nobody drives is an arm
        // that can say anything.
        //
        // ⚠ The sentence is the MODEL's, carried verbatim, so what is driven
        // here is a sentence this file did not write — which is the point of
        // the arm. A wording of its own would be the screen answering a
        // question the framework already answered.
        (
            "Unwired",
            super::Finding::Unwired(pinion_node_graph::Objection::Warns(
                "listening, and nothing on this canvas dials it — the drawing \
                 is not the whole picture"
                    .to_owned(),
            ))
            .sentence(),
        ),
        (
            "Incompatible",
            super::Finding::Incompatible {
                peer: "P-02".to_owned(),
                because: "legacy speaks v4-v5 and reference speaks v6-v8, so \
                          they share no wire revision"
                    .to_owned(),
            }
            .sentence(),
        ),
    ];
    assert_speaks_of("Finding", CARD, super::Finding::ARMS, &said, &[]);
}

/// ★★★★★ R1941 — **the weight of a framework answer is REPORTED here, not
/// decided here.**
///
/// ⚠ Asserted in this file rather than in the walk, and the reason is a
/// measurement rather than a preference: the walk that drives this screen
/// cannot reach a card whose kind objects. The taxonomy's rule needs a card
/// that accepts, is listening, and has nothing wired to it, and no gesture this
/// screen publishes removes a link or changes a card's role — so a
/// counterfactual that put this arm back to a flat `false` left that walk
/// GREEN. An empty population is a green light for a wrong assertion, which is
/// the failure this project keeps meeting; the repair is to assert where the
/// population is not empty.
///
/// What it holds is the whole of what R1941 changed here: this arm used to
/// answer `false` whatever the framework said, on the stated ground that a
/// kind's answer "says the drawing is partial, not that it is wrong" — a
/// judgement about somebody else's answer. Now it forwards.
#[test]
fn r1941_a_framework_objection_carries_its_own_weight() {
    use pinion_node_graph::Objection;

    let blocking = super::Finding::Unwired(Objection::Blocks("cannot start".to_owned()));
    let warning = super::Finding::Unwired(Objection::Warns("looks odd".to_owned()));
    let note = super::Finding::Unwired(Objection::Notes("worth knowing".to_owned()));

    assert!(
        blocking.blocks(),
        "★★★★★ a kind that BLOCKS blocks — the screen reports the weight"
    );
    assert!(
        !warning.blocks(),
        "★ and one that only warns does not, so the forwarding is not a \
         blanket yes"
    );
    assert!(!note.blocks());

    // ★ And the sentence is the framework's either way, so weight and wording
    // travel together rather than being two statements this screen joins.
    assert_eq!(blocking.sentence(), "cannot start");
    assert_eq!(warning.sentence(), "looks odd");
    assert_eq!(note.sentence(), "worth knowing");
}

/// ★★★★ R1718 — and nothing on this screen speaks to a person without being
/// driven.
///
/// The screen matters more than the framework here: the framework's speaking
/// types are few and central, and a SCREEN grows a vocabulary a situation at a
/// time — which is exactly how the launch gate came to have a fourth situation
/// with no wording of its own.
#[test]
fn r1718_every_speaking_type_on_this_screen_is_driven() {
    pinion_core::test_fixtures::speech::census::assert_every_speaker_is_driven(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        1,
    );
}

/// ★★★★★ R1969.1 — **how many declared links land on a card that says nowhere
/// to reach it**, held to the number somebody measured instead of left to
/// prose.
///
/// # What was measured, and what it corrected
///
/// The behaviour canon refuses this outright and TELLS THE PERSON WHY: its
/// candidate rule drops an acceptor with no listen endpoint before a wire can
/// follow the cursor, and letting go over one produces *cannot connect · <id>
/// has no listen endpoint*. Its own comment beside that rule names a node of
/// its fixture as the case. We draw the link instead, and say nothing.
///
/// R1961 recorded the size of that gap as **three of the seven links**. Measured
/// again at R1969.1, after R1969 removed the scheme comparison that was
/// producing the other two, it is **ONE** — and the correction matters more than
/// the number: R1961 read the gap as an artifact of seeding order, which made it
/// look unrepairable, and the one that remains has nothing to do with seeding.
/// `T-02` is a subscriber in client mode that declares no listen row at all, at
/// any moment of the build. ⇒ a FIXTURE defect, not a rule defect.
///
/// Two instruments agreed on the one: making the model refuse an acceptor that
/// says nowhere reddens ten tests with `6 != 7`, and the count below — derived
/// from the specification alone — is 1.
///
/// # Why this is a gate rather than a sentence
///
/// The number lived only in `debt-a-listening-node-with-no-address-still-takes-
/// links`, in prose, and it was WRONG there for eight rounds with nothing able
/// to notice. A second such link added to the fixture would be a second silent
/// divergence from the canon; this fails instead. It does not assert the gap is
/// acceptable — it asserts the gap is the size the debt says it is.
#[test]
fn r1969_1_every_declared_link_lands_on_a_card_that_says_where_to_reach_it() {
    let listens = |id: &str| -> bool {
        spec::NODES
            .iter()
            .find(|n| n.id == id)
            .is_some_and(|n| n.rows.iter().any(|(key, _)| *key == "listen"))
    };
    let blind: Vec<String> = spec::LINKS
        .iter()
        .filter(|(_, to)| !listens(to))
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect();
    // ★ Not zero, and that is the DEBT rather than the assertion being weak.
    // The canon refuses these; this screen draws them. What is held here is the
    // size, so a third one cannot arrive unremarked while the debt is open.
    assert_eq!(
        blind.len(),
        1,
        "★★★★★ {} declared link(s) land on a card that declares no listen \
         address: {blind:?}. The canon drops such an acceptor from its \
         candidates and names the reason on a toast; while \
         `debt-a-listening-node-with-no-address-still-takes-links` is open this \
         screen draws them, and the number is pinned so a new one is a failure \
         rather than a silence.",
        blind.len(),
    );
    // ★ And every OTHER acceptor really does say where to reach it, so the one
    // above is a named exception and not the shape of the whole fixture.
    for (from, to) in spec::LINKS {
        assert!(
            listens(to) || blind.contains(&format!("{from} -> {to}")),
            "{from} -> {to}",
        );
    }
}

#[test]
fn r1651_the_opening_graph_is_the_specification() {
    let owner = Owner::new();
    owner.run(|| {
        let state = state();
        let names: Vec<String> = state.cards().iter().map(|n| state.name_of(*n)).collect();
        let declared: Vec<&str> = spec::NODES.iter().map(|n| n.id).collect();
        assert_eq!(
            names, declared,
            "every declared node, in order, and no other"
        );
        assert_eq!(
            state.link_count(),
            spec::LINKS.len(),
            "and every declared link was accepted by the model"
        );
        assert_eq!(
            state.active_card().map(|n| state.name_of(n)).as_deref(),
            Some(spec::SELECTED_NODE),
            "the screen opens on the node the reference opens on"
        );
        assert_eq!(state.zoom.get(), spec::OPENING_ZOOM);
        assert!(
            !state.discovery.get(),
            "★ discovery is OFF by default — a graph whose links are all \
             authored is the one whose behaviour is a function of the canvas"
        );
    });
}

#[test]
fn r1651_the_selected_node_shows_the_fields_the_reference_shows() {
    let owner = Owner::new();
    owner.run(|| {
        // ★★★ R1716 — the form the screen SHOWS, not the rows it seeds. Two of
        // the specification's rows are worked out from the graph (the mode a
        // router's role implies, and the addresses its drawn links dial) and a
        // third from where the card runs, so a check that read the seed would
        // be checking the smaller half and calling it the inspector.
        let state = state();
        let node = state.active_card().expect("the screen opens on a card");
        let form = super::shown_form(&state, node).expect("the card has a form");
        let keys: Vec<&str> = form
            .fields()
            .iter()
            .map(pinion_core::widgets::config_form::ConfigField::key)
            .collect();
        let declared: Vec<&str> = spec::FIELDS.iter().map(|f| f.key).collect();
        assert_eq!(keys, declared, "the same rows, in the same order");
        for want in spec::FIELDS {
            let held = form.field(want.key).expect("declared");
            assert_eq!(held.ty(), want.ty, "{}", want.key);
            assert_eq!(held.applies().wire(), want.applies, "{}", want.key);
            assert_eq!(held.value(), want.value, "{}", want.key);
        }
        let hot = form
            .fields()
            .iter()
            .filter(|f| f.applies() == Applies::Hot)
            .count();
        assert_eq!(
            hot, 1,
            "★ exactly one row reaches a running node, which is the reference's \
             own point and the reason the badge exists"
        );
        let offered: Vec<&str> = form
            .addable()
            .into_iter()
            .map(pinion_core::widgets::config_form::ConfigField::key)
            .collect();
        for key in spec::ADDABLE {
            if form.field(key).is_some() {
                continue;
            }
            assert!(offered.contains(key), "{key} is offered");
        }
    });
}

#[test]
fn r1651_the_gate_reports_the_two_warnings_the_reference_reports() {
    let owner = Owner::new();
    owner.run(|| {
        let state = state();
        let lines = state.gate_lines();
        let warnings: Vec<&String> = lines.iter().filter(|(b, _)| !b).map(|(_, s)| s).collect();
        assert!(
            warnings
                .iter()
                .any(|s| s.starts_with("S-01") && s.contains("listening")),
            "a store with nowhere to listen is a pin nobody can dial: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|s| s.starts_with("P-01") && s.contains("discovery")),
            "a peer with discovery on can acquire links nobody authored: {warnings:?}"
        );
        assert!(
            lines.iter().all(|(blocks, _)| !blocks),
            "and none of them blocks — that is what separates a warning from an \
             error, and the reference's gate opens: {lines:?}"
        );
        assert!(state.verdict().may_launch());
    });
}

#[test]
fn r1651_a_value_that_would_fail_at_start_up_closes_the_gate() {
    let owner = Owner::new();
    owner.run(|| {
        let state = state();
        let node = state.node_of(spec::SELECTED_NODE).expect("selected");
        assert!(state.verdict().may_launch(), "it opens to begin with");
        state
            .forms
            .borrow_mut()
            .get_mut(&node)
            .expect("a form")
            .set("transport.link.tx.batch_size", "70000")
            .expect("held");
        let verdict = state.verdict();
        assert!(!verdict.may_launch(), "and a value out of range closes it");
        assert_eq!(verdict.blocking(), 1);
        assert!(
            verdict.warning() >= 2,
            "★ while the warnings still stand and are still said: {}",
            verdict.sentence()
        );
    });
}

#[test]
fn r1651_the_pin_a_node_shows_is_derived_from_the_form_it_holds() {
    // ★ The rule the legend draws, and the one place the inspector and the
    // canvas meet: an endpoint edited on the right changes the pin on the left
    // and the warning at the bottom, by re-deriving rather than by a second
    // write.
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let store = state.node_of("S-01").expect("on the canvas");
        // ⚠ R1927 — the finding is named by ITS OWN sentence, not by the word
        // "listening". The loose predicate was written when that word could
        // only mean one thing on this screen; it now means two, and the
        // opposite two: `nothing is listening` (this one, retired by giving the
        // card an endpoint) and `listening, and nothing dials it` (the model's,
        // RAISED by the same edit, because S-01 has no inbound link). The loose
        // form failed here the moment the second existed — correctly, and for a
        // reason that had nothing to do with what this test is about.
        let warned = |state: &LabState| {
            state
                .gate_lines()
                .into_iter()
                .any(|(_, s)| s.starts_with("S-01") && s.contains("nothing is listening"))
        };
        // The other half of the same fact, so the pair is asserted rather than
        // one of them being quietly filtered out of the question.
        let undialled = |state: &LabState| {
            state
                .gate_lines()
                .into_iter()
                .any(|(_, s)| s.starts_with("S-01") && s.contains("nothing on this canvas dials"))
        };
        let listening = |state: &LabState| {
            state
                .doc
                .borrow()
                .tree(super::ROOT)
                .and_then(|t| t.node(store))
                .and_then(|n| match &n.body {
                    super::NodeBody::Kind(kind) => Some(kind.listening),
                    _ => None,
                })
                .expect("a kind node")
        };
        assert!(warned(&state), "it opens with nowhere to listen");
        assert!(!listening(&state), "so the pin is drawn closed");
        assert!(
            !undialled(&state),
            "and a card that listens nowhere cannot be undialled — the model's \
             rule has nothing to say about it yet"
        );

        state
            .forms
            .borrow_mut()
            .get_mut(&store)
            .expect("a form")
            .set("listen.endpoints", "quic/0.0.0.0:7460")
            .expect("held");
        super::sync_node(&state, store);

        assert!(listening(&state), "giving it an endpoint opens the pin");
        assert!(!warned(&state), "and retires the warning it raised");
        // ★★★★★ R1927 — and it RAISES the other one, which is a real finding
        // and not an artefact of this edit: S-01 dials R-01 and nothing dials
        // S-01, so the moment it has somewhere to listen it is a service the
        // drawing shows nobody using. One edit, one finding retired and one
        // raised — a screen that only asserted the retirement would have been
        // describing half of what it did.
        assert!(
            undialled(&state),
            "and raises the model's own, because nothing here dials it"
        );
        let transport = state
            .doc
            .borrow()
            .tree(super::ROOT)
            .and_then(|t| t.node(store))
            .and_then(|n| match &n.body {
                // ★ R1962 — the LISTEN half: this edit gave the card an address
                // of its own, and that is the scheme its accept pin wears.
                super::NodeBody::Kind(kind) => Some(kind.listens_over),
                _ => None,
            })
            .expect("a kind node");
        assert_eq!(
            transport,
            Some(crate::graph::Transport::Quic),
            "★ and the pin's COLOUR follows the locator, because the colour is \
             the type the taxonomy refuses a mismatched link on"
        );
    });
}

// ★★★★★ R1968 — `r1651_the_specification_and_the_taxonomy_agree_about_every_role`
// stood here and is DELETED rather than moved, because there is no longer a
// way for it to fail.
//
// It compared `spec::ROLES` — an authored table of eight records — with the
// eight `match self` accessors on `Role`, field by field, in both directions.
// That check was worth its cost while there were two authorings: the four
// columns it compared (`name`, `gist`, `group`, `accepts`) were spelled twice
// and could drift. `spec::ROLES` is now `Role::specs()`, so every line of it
// read one declaration and compared it with itself.
//
// An assertion with no failing path is worse than absent: it counts toward
// coverage, it reads as a guarantee, and it goes green for a reason unrelated
// to the property. The guarantee it used to give is now structural — one
// record per role, `RoleSpec`'s fields required — which is the trade this
// round made, and the trade is only honest if the dead check goes with it.

#[test]
fn r1651_a_listening_node_takes_as_many_dials_as_reach_it() {
    // ★ The reference's router shows four inbound links on ONE pin. A dataflow
    // input holds one wire, because a value has one source; a listening
    // endpoint does not, so the accept pin is a variadic run and the arithmetic
    // that hides the run is `free_accept_port`.
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let router = state.node_of("R-01").expect("on the canvas");
        let (inbound, _) = state.degree(router);
        assert_eq!(
            inbound, 3,
            "the opening graph dials the router from three nodes"
        );
        // ★★★★★ R1962 — the fourth dialler is a card the PALETTE adds, which is
        // a gesture this screen has, rather than a card already on the canvas.
        // It was `T-01`, and that only worked while every card spoke one
        // transport: T-01 dials P-01, P-01 listens on quic since R1962, and a
        // card has ONE dial pin — so T-01 cannot also dial the tcp router. The
        // three cards with free dial pins would each close a cycle. A fresh
        // card speaks nothing yet, so it may dial anything, which is the same
        // reasoning `r1915_a_wire_on_a_member_is_cut_by_the_fold_and_named`
        // recorded when it hit the identical wall.
        let before = state.cards();
        super::add_node(&state, crate::graph::Role::Store);
        let extra = *state
            .cards()
            .iter()
            .rev()
            .find(|n| !before.contains(n))
            .expect("★ the palette put a card on the canvas");
        super::connect(&state, extra, router).expect("a fourth dial is accepted");
        assert_eq!(state.degree(router).0, 4, "and the pin took it");

        // And a role that never listens still refuses, by name.
        let publisher = state.node_of("T-01").expect("on the canvas");
        let refused =
            super::connect(&state, router, publisher).expect_err("a publisher does not listen");
        assert!(
            format!("{refused:?}").contains("T-01"),
            "and the refusal names it: {refused:?}"
        );
    });
}

#[test]
fn r1651_every_control_the_screen_paints_is_hit_at_the_centre_it_paints_in() {
    // The two-direction property R1649's sweep established, at the level a unit
    // test can hold it: the rectangles the painter uses and the rectangles the
    // hit test uses are the same values, so a control cannot be drawn somewhere
    // a press does not reach.
    let owner = Owner::new();
    owner.run(|| {
        // ★★★★★ R1736 — the state the VIEW reads, and painted first. A press is
        // resolved from the paint now, so this test has two new obligations: it
        // must drive the screen the view function draws (a state of its own
        // would be painted by nobody), and it must have drawn something.
        super::reset_lab_state();
        let state = super::use_lab_state();
        crate::painted::render_so_a_press_can_be_asked(&state);
        // ★ R1653 — a card's rectangle is on the WORLD SURFACE the canvas is a
        // viewport onto, and a press arrives in window coordinates. The
        // conversion is the app's own, so this cannot drift from it.
        let window = |x: u32, y: u32| {
            content_to_window(&state, i64::from(x), i64::from(y)).expect("on screen")
        };
        for node in state.cards() {
            let card = card_rect(&state, node).expect("a declared node has a card");
            let centre = window(card.x + card.w / 2, card.y + card.h / 2);
            assert_eq!(
                Hit::at(&state, centre.0, centre.1),
                Hit::Node(node),
                "{} is pressable at the centre of its card",
                state.name_of(node)
            );
            let dial = pin_rect(&state, card, true);
            let dial_centre = window(dial.x + dial.w / 2, dial.y + dial.h / 2);
            assert_eq!(
                Hit::at(&state, dial_centre.0, dial_centre.1),
                Hit::Pin {
                    node,
                    side: pinion_node_graph::Side::Output,
                    at: pinion_node_graph::PortPath::root(0),
                },
                "★ and its dial pin is reachable — a pin overhangs its card, so \
                 testing the card first would make a link impossible to author"
            );
        }
    });
}

#[test]
fn r1651_the_panes_are_the_widths_the_specification_gives_them() {
    let rail = spec::PANES[0].width;
    let palette = spec::PANES[1].width;
    let inspector = spec::PANES[3].width;
    assert_eq!(super::RAIL_W, rail);
    assert_eq!(super::PALETTE_W, palette);
    assert_eq!(super::INSP_W, inspector);
    let canvas = canvas_rect();
    assert_eq!(
        canvas.x,
        rail + palette,
        "the canvas starts where the palette ends"
    );
    assert_eq!(
        canvas.x + canvas.w,
        inspector_rect().x,
        "and ends where the inspector begins — no gap, no overlap"
    );
}

#[test]
fn r1651_every_role_the_palette_offers_is_pressable_and_adds_a_node() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let before = state.cards().len();
        for (n, role) in Role::ALL.into_iter().enumerate() {
            let row = super::palette_row(n);
            assert_eq!(
                Hit::at(&state, row.x + row.w / 2, row.y + row.h / 2),
                Hit::Role(role),
                "{} is pressable in the palette",
                role.name()
            );
        }
        super::add_node(&state, Role::Responder);
        assert_eq!(
            state
                .doc
                .borrow()
                .tree(super::ROOT)
                .map(pinion_node_graph::Tree::node_count),
            Some(before + spec::FRAMES.len() + 1),
            "and pressing one adds a node"
        );
    });
}

/// R1653 — the canvas's two coordinate conversions invert each other.
///
/// `content_to_window` exists for the tests, so nothing in the running app
/// would notice it drifting from `window_to_content`. This is what would.
#[test]
fn r1653_the_two_canvas_conversions_invert_each_other() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let canvas = canvas_rect();
        // A pan the hint strip's gesture can produce, in both directions.
        for pan in [(0, 0), (30, 20), (-30, -20), (-400, 250)] {
            state.pan.set(pan);
            for (px, py) in [
                (canvas.x, canvas.y),
                (canvas.x + canvas.w / 3, canvas.y + canvas.h / 2),
                (canvas.x + canvas.w - 1, canvas.y + canvas.h - 1),
            ] {
                let (cx, cy) = super::window_to_content(&state, px, py);
                assert!(cx >= 0 && cy >= 0, "the surface has no negative side");
                assert_eq!(
                    super::content_to_window(&state, cx, cy),
                    Some((px, py)),
                    "pan {pan:?} at ({px},{py})"
                );
            }
        }
    });
}

/// R1654 — a node can be dragged ABOVE and LEFT of where the graph opened.
///
/// Reported from the running window: a card stopped dead partway up the canvas.
/// The cause was two clamps for one fact — `clamp_to_world` bounds the position
/// to the world surface, and a `.max(0)` on the next line pinned it at the
/// origin, so the world's negative half was unreachable by the gesture that
/// exists to reach it.
#[test]
fn r1654_a_node_drags_above_and_left_of_the_opening_graph() {
    let owner = Owner::new();
    owner.run(|| {
        // ★★★★★ R1736 — the state the view reads, painted first: a press is
        // resolved from the paint now, so a drag that starts without one is
        // starting on an empty screen.
        super::reset_lab_state();
        let state = super::use_lab_state();
        crate::painted::render_so_a_press_can_be_asked(&state);
        let node = state.node_of("R-01").expect("on the canvas");
        let before = super::card_rect(&state, node).expect("a card");
        let start = super::content_to_window(
            &state,
            i64::from(before.x + before.w / 2),
            i64::from(before.y + before.h / 2),
        )
        .expect("on screen");

        super::move_cursor(&state, start.0, start.1);
        super::press(&state);
        // Up and to the left, far enough to pass the canvas origin.
        super::move_cursor(
            &state,
            start.0.saturating_sub(260),
            start.1.saturating_sub(240),
        );
        super::release(&state);

        let (x, y) = state
            .doc
            .borrow()
            .tree(super::ROOT)
            .and_then(|t| t.node(node).map(|n| (n.x, n.y)))
            .expect("the node");
        assert!(
            y < 0,
            "the drag reached above the opening graph's origin: y = {y}"
        );
        assert!(x < 100, "and left of where it started: x = {x}");
    });
}

/// R1654 — the group behaviour the reference has: a frame's box is DERIVED from
/// what it holds, a card dropped inside joins it, and dragging the frame takes
/// its members along.
///
/// Reported from the running window as "the group behaviour does not match".
/// The frames were rectangles out of the specification table: they did not grow
/// when a card was dragged in, did not shrink when one left, and could not be
/// moved. Three of the reference's nine frame verbs are exactly these.
#[test]
fn r1654_a_frame_is_derived_from_its_members_and_moves_them() {
    let owner = Owner::new();
    owner.run(|| {
        // ★★★★★ R1736 — the state the view reads, because the drags below are
        // resolved from what the screen painted.
        super::reset_lab_state();
        let state = super::use_lab_state();
        let frames = super::frames_of(&state);
        let (host_a, _) = frames
            .iter()
            .find(|(_, name)| name == "host-a")
            .cloned()
            .expect("declared");
        let (host_b, _) = frames
            .iter()
            .find(|(_, name)| name == "host-b")
            .cloned()
            .expect("declared");

        // (1) DERIVED: the box holds every one of its cards.
        let before = super::frame_rect_of(&state, host_a);
        for member in super::members_of(&state, host_a) {
            let card = super::card_rect(&state, member).expect("a card");
            assert!(
                card.x >= before.x
                    && card.y >= before.y
                    && card.x + card.w <= before.x + before.w
                    && card.y + card.h <= before.y + before.h,
                "{} is outside the frame that holds it",
                state.name_of(member)
            );
        }

        // (2) MEMBERSHIP FOLLOWS THE DROP: drag a card from one host to the
        // other and the two boxes re-derive.
        let moving = state.node_of("T-01").expect("on host-b");
        assert!(super::members_of(&state, host_b).contains(&moving));
        // ★★★★★ R1736 — painted before the drag, because the press that starts
        // it is resolved from the paint. Re-painted rather than painted once at
        // the top: this test moves cards between the two acts, and a press
        // aimed with a frame the screen has since redrawn is aimed at history.
        crate::painted::render_so_a_press_can_be_asked(&state);
        let target =
            super::card_rect(&state, state.node_of("P-02").expect("on host-a")).expect("a card");
        let from = super::card_rect(&state, moving).expect("a card");
        let start = super::content_to_window(
            &state,
            i64::from(from.x + from.w / 2),
            i64::from(from.y + from.h / 2),
        )
        .expect("on screen");
        let onto = super::content_to_window(
            &state,
            i64::from(target.x + target.w / 2),
            i64::from(target.y + target.h + 30),
        )
        .expect("on screen");
        super::move_cursor(&state, start.0, start.1);
        super::press(&state);
        super::move_cursor(&state, onto.0, onto.1);
        super::release(&state);
        assert!(
            super::members_of(&state, host_a).contains(&moving),
            "the card joined the host it was dropped on"
        );
        assert!(
            !super::members_of(&state, host_b).contains(&moving),
            "and left the one it came from"
        );

        // (3) THE FRAME IS A HANDLE: dragging its tab moves every member.
        // ★ R1736 — repainted again, for the reason above: act (2) moved a card
        // between hosts, so both frames have new boxes.
        crate::painted::render_so_a_press_can_be_asked(&state);
        let tab = super::frame_rect_of(&state, host_a);
        let grip = super::content_to_window(&state, i64::from(tab.x + 40), i64::from(tab.y + 4))
            .expect("on screen");
        let positions = |state: &LabState| -> Vec<(i32, i32)> {
            super::members_of(state, host_a)
                .into_iter()
                .filter_map(|n| {
                    state
                        .doc
                        .borrow()
                        .tree(super::ROOT)
                        .and_then(|t| t.node(n).map(|s| (s.x, s.y)))
                })
                .collect()
        };
        let was = positions(&state);
        super::move_cursor(&state, grip.0, grip.1);
        super::press(&state);
        super::move_cursor(&state, grip.0 + 40, grip.1 + 25);
        super::release(&state);
        let now = positions(&state);
        assert_eq!(was.len(), now.len(), "the same members");
        assert!(
            was.iter().zip(&now).all(|(a, b)| b.0 > a.0 && b.1 > a.1),
            "every member moved with the frame: {was:?} -> {now:?}"
        );
    });
}

/// R1669 — every reserved rail seat names a REQUIREMENT, and the open ones name
/// nothing.
///
/// ★ Here because a counterfactual passed without it. The painted gate and the
/// demo both read the booking out of `spec` and compare it to what the screen
/// reports — so changing the specification's `"requirement 12"` to `"later"`
/// changed both sides and neither noticed. A check that compares a thing to
/// itself is not a check; this pins the SHAPE, which is the part the
/// specification cannot move on its own.
#[test]
fn r1669_a_reserved_seat_names_a_requirement() {
    let reserved: Vec<&str> = super::spec::RAIL
        .iter()
        .filter_map(|(_, booking)| *booking)
        .collect();
    assert_eq!(reserved.len(), 2, "the reference locks two seats");
    for booking in reserved {
        assert!(
            booking.starts_with("requirement "),
            "a seat is booked under {booking:?}, which names no requirement",
        );
        assert!(
            booking["requirement ".len()..].parse::<u32>().is_ok(),
            "{booking:?} names no requirement NUMBER, so nothing traces to it",
        );
    }
    assert!(
        super::spec::RAIL
            .iter()
            .any(|(name, booking)| *name == super::spec::RAIL_ACTIVE && booking.is_none()),
        "the seat this screen IS cannot be a reserved one",
    );
}

/// ★★★★★ R1773 — **the rail this screen draws is the one the reference
/// states**, seat for seat.
///
/// The check the round above says it cannot make. `r1669_a_reserved_seat_names_a_requirement`
/// pins the SHAPE of a booking and says in as many words why it cannot pin the
/// value: both sides read the same constant, and a thing compared with itself
/// is not compared. This compares against the OTHER document —
/// `docs/analyzer-rail-spec.json`, extracted from the behaviour reference by
/// another hand — which is the only comparison that can fail for the right
/// reason.
///
/// # What it found the first time it ran (R1773)
///
/// The sibling screen has read this pin since R1728. This screen never did, and
/// its hand-written copy had drifted **twice**:
///
/// * **a missing seat** — the reference's rail has eight and this had seven,
///   with `settings` absent;
/// * **a wrong requirement** — `sessions` was booked under `requirement 14`
///   where the reference books it under 18.
///
/// Neither was reachable by any check that existed: the shape gate accepts any
/// number, and the census counts what is drawn.
#[test]
fn r1773_the_rail_this_screen_draws_is_the_one_the_reference_states() {
    let doc: serde_json::Value = serde_json::from_str(super::spec::RAIL_SPEC_JSON)
        .expect("the rail specification is readable JSON");
    let canon = doc["canon"]
        .as_array()
        .expect("the rail specification declares a canon array");

    let stated: Vec<&str> = canon
        .iter()
        .map(|seat| seat["key"].as_str().expect("a specified seat has a key"))
        .collect();
    let drawn: Vec<&str> = super::spec::RAIL.iter().map(|(key, _)| *key).collect();
    assert_eq!(
        drawn, stated,
        "★ this screen's rail and the reference's are different rosters. One \
         application has one navigation, and a second copy of the seats is how \
         two screens of this tool came to disagree about what the tool contains",
    );

    for (seat, (key, booking)) in canon.iter().zip(super::spec::RAIL) {
        let standing = seat["standing"].as_str().expect("a seat has a standing");
        assert_eq!(
            booking.is_some(),
            standing == "closed",
            "★ `{key}` is {standing} in the reference and this screen {} it",
            if booking.is_some() { "locks" } else { "opens" },
        );
        // ★★ The requirement NUMBER, against the reference's own note rather
        // than against this screen's constant. The note is prose — the pin has
        // no machine-readable field for it — so the parse is narrow and the
        // limitation is stated rather than hidden: this checks the number a
        // seat is booked under, and nothing about the wording around it.
        if let Some(booked) = booking {
            let note = seat["$note"]
                .as_str()
                .expect("a closed seat carries the note that books it");
            let stated_number = note
                .split("requirement ")
                .nth(1)
                .and_then(|rest| {
                    rest.split(|c: char| !c.is_ascii_digit())
                        .next()
                        .filter(|d| !d.is_empty())
                })
                .expect("the reference books a closed seat under a numbered requirement");
            let drawn_number = booked
                .strip_prefix("requirement ")
                .expect("this screen books a seat under a numbered requirement");
            assert_eq!(
                drawn_number, stated_number,
                "★★ `{key}` is booked under requirement {drawn_number} here and \
                 under {stated_number} in the reference. Nothing could see this \
                 before: the shape gate accepts any number and the census counts \
                 what is drawn",
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// R1681 — a link's life
// ─────────────────────────────────────────────────────────────────

/// R1681 — the endpoint a link dialled is a property OF THE LINK, and a target
/// that listens twice offers a seat per address.
///
/// The one claim the whole endpoint axis rests on: growing the target's listen
/// list has to make a choice appear on a wire that was already drawn, without
/// anything being re-authored.
#[test]
fn r1681_a_target_that_listens_twice_offers_a_seat_per_address() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let target = state
            .node_of(spec::SELECTED_NODE)
            .expect("the opening node");
        assert_eq!(
            super::endpoints_of(&state, target).len(),
            1,
            "the screen opens with the target listening in one place"
        );
        assert!(
            super::link_chrome(&state).is_some_and(|c| c.chips.is_empty()),
            "so the picked link offers no choice — a choice between one address \
             is not a choice, and the reference draws the row only when there \
             is more than one"
        );

        // Grow the list the way the inspector's `+` row does.
        super::add_element(&state, "listen.endpoints");
        let endpoints = super::endpoints_of(&state, target);
        assert_eq!(
            endpoints.len(),
            2,
            "the target now listens in two places: {endpoints:?}"
        );
        let chrome = super::link_chrome(&state).expect("a link is picked");
        assert_eq!(
            chrome.chips.len(),
            2,
            "and the wire that was already drawn now offers a seat per address"
        );
        assert_eq!(
            chrome
                .chips
                .iter()
                .map(|(one, _)| one.clone())
                .collect::<Vec<_>>(),
            endpoints,
            "each seat is one of the target's own addresses, in its order"
        );
        assert_eq!(chrome.current, 0, "the link took the first");

        // ★ And every seat is somewhere a person can actually reach: the
        // column is nudged clear of the cards it would otherwise cover, and a
        // nudge that pushed it out of the viewport would trade one unreachable
        // affordance for another.
        let canvas = canvas_rect();
        for (endpoint, seat) in &chrome.chips {
            let at = content_to_window(&state, i64::from(seat.x), i64::from(seat.y));
            assert!(
                at.is_some_and(|(x, y)| x >= canvas.x
                    && y >= canvas.y
                    && x + seat.w <= canvas.x + canvas.w
                    && y + seat.h <= canvas.y + canvas.h),
                "the seat for {endpoint} is at {seat:?} -> window {at:?}, which \
                 is not inside the canvas {canvas:?}"
            );
        }
    });
}

/// R1681 — the accept run holds exactly one slot per thing landing on it, over
/// every operation that opens or closes one.
///
/// ★ The bookkeeping this round introduced, and the kind that goes wrong
/// silently: every arriving link opens a slot, so every departing one has to
/// close one, and a slot a REPORTED link holds must survive the drawn link
/// beside it being deleted. A run that only grew would still draw correctly and
/// would leak a port per edit, which nothing else here would notice.
#[test]
fn r1681_the_accept_run_holds_one_slot_per_thing_that_lands_on_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        // A node with `at_least(1)` keeps one empty slot, so the invariant is
        // "as many slots as things land on it, and no more" — with that floor.
        //
        // BOTH directions. A run that only grew leaks a port per edit; a run
        // that closed a slot something still lands on re-points a link or a
        // report onto somebody else's address, which is worse and which a
        // count of the surplus alone cannot see — `saturating_sub` reads a
        // shortfall as zero, and that is the shape the first draft of this
        // test was blind to.
        let census = || {
            state
                .cards()
                .into_iter()
                .filter_map(|node| {
                    let doc = state.doc.borrow();
                    let arity = doc
                        .signature(super::ROOT, node)
                        .map_or(0, |s| s.inputs.len());
                    let landed = doc.tree(super::ROOT).map_or(0, |t| {
                        t.links().iter().filter(|l| l.to.node == node).count()
                    });
                    let reported = doc
                        .observations(super::ROOT)
                        .iter()
                        .filter(|o| o.to.node == node)
                        .count();
                    let want = landed + reported;
                    drop(doc);
                    // The floor is the crate's `at_least(1)`, and it applies to
                    // the roles that declare an accept run at all — a role that
                    // never listens has no run and no floor.
                    let floor = usize::from(state.role_of(node).is_some_and(Role::accepts));
                    (arity != want.max(floor)).then(|| (state.name_of(node), arity, want))
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(census(), Vec::new(), "the opening graph leaks no slot");

        let router = state.node_of("R-01").expect("on the canvas");
        let store = state.node_of("S-01").expect("on the canvas");
        let peer = state.node_of("P-03").expect("on the canvas");
        let link = state
            .doc
            .borrow()
            .tree(super::ROOT)
            .and_then(|t| {
                t.links()
                    .iter()
                    .find(|l| l.from.node == store && l.to.node == router)
                    .map(|l| l.id)
            })
            .expect("the store dials the router");

        super::relink_to(&state, link, peer).expect("the peer listens");
        assert_eq!(census(), Vec::new(), "a move closes the slot it left");

        super::delete_link(&state, link).expect("it is drawn");
        assert_eq!(
            census(),
            Vec::new(),
            "and a delete closes the one it was on"
        );

        // ★ The case a naive close gets wrong: adopting puts a drawn link on
        // the very slot a reported one holds, and deleting that link must NOT
        // take the slot away from the report that is still there.
        let seen = *state
            .doc
            .borrow()
            .observations(super::ROOT)
            .first()
            .expect("the screen opens with something reported");
        // Captured BEFORE, because the point is that it is unchanged after —
        // reading it again afterwards and comparing it with itself is an
        // assertion that cannot fail, which is what the first draft did.
        let reported_on = super::endpoint_at(&state, seen.to).expect("reported on an address");
        super::adopt_link(&state, seen.from, seen.to).expect("the model can hold it");
        let adopted = state
            .doc
            .borrow()
            .tree(super::ROOT)
            .and_then(|t| t.links().iter().find(|l| l.to == seen.to).map(|l| l.id))
            .expect("the adopted link");
        super::delete_link(&state, adopted).expect("it is drawn");
        assert!(
            state.doc.borrow().observations(super::ROOT).contains(&seen),
            "the report is still there"
        );
        assert_eq!(
            super::endpoint_at(&state, seen.to).as_deref(),
            Some(reported_on.as_str()),
            "★ and its slot still names the address it was reported on — a \
             close that only counted DRAWN links takes the slot away, and then \
             the report points at whatever moved down into its place"
        );
        assert_eq!(census(), Vec::new());
    });
}

// ── R1682 — a node's own life ───────────────────────────────────────────────

/// R1682 — ★★ a rename moves ONE string, and this is the list of things it
/// therefore does not have to carry.
///
/// The reference prototype renames by copying the node under the new name and
/// covering the old one with a deletion, so it has to hand-move ten side tables
/// afterwards — its placement, its containment, its edits, its extra fields,
/// its hidden fields, its collapse, its mute, its wire list, its selection.
/// Here the card keeps its identity, and every one of those is keyed by that
/// identity, so the assertion is that they are all still exactly where they
/// were. If this screen ever grows a table keyed by a card's NAME, this test is
/// what fails.
#[test]
fn r1682_a_rename_carries_nothing_because_nothing_is_keyed_by_a_name() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let node = state.node_of("P-03").expect("the specification has it");
        state.selection.set(Selection::one(node));

        let form_before = state.forms.borrow().get(&node).cloned();
        let placed_before = state.opened_at.borrow().get(&node).cloned();
        let degree_before = state.degree(node);
        let links_before: Vec<u32> = state
            .doc
            .borrow()
            .tree(super::ROOT)
            .map_or_else(Vec::new, |t| t.links().iter().map(|l| l.id.0).collect());

        super::rename_card(&state, node, "edge-01").expect("the name is free");

        assert_eq!(
            state.node_of("edge-01"),
            Some(node),
            "the same card answers to the new name"
        );
        assert_eq!(state.node_of("P-03"), None, "and not to the old one");
        assert_eq!(state.name_of(node), "edge-01");
        assert_eq!(
            state.active_card(),
            Some(node),
            "the selection is the thing that moved, so nothing had to repair it"
        );
        assert_eq!(state.forms.borrow().get(&node).cloned(), form_before);
        assert_eq!(state.opened_at.borrow().get(&node).cloned(), placed_before);
        assert_eq!(state.degree(node), degree_before);
        assert_eq!(
            state
                .doc
                .borrow()
                .tree(super::ROOT)
                .map_or_else(Vec::new, |t| t
                    .links()
                    .iter()
                    .map(|l| l.id.0)
                    .collect::<Vec<_>>()),
            links_before,
            "★ no link was re-minted — the whole point of renaming in place"
        );
    });
}

/// R1682 — a name another card already answers to is refused, by the MODEL,
/// and the screen is left exactly as it was.
#[test]
fn r1682_a_name_already_on_the_canvas_is_refused_and_changes_nothing() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let node = state.node_of("P-03").expect("the specification has it");
        let taken = state.node_of("P-01").expect("and this one");

        let why = super::rename_card(&state, node, "P-01").expect_err("P-01 is taken");
        assert!(
            format!("{why:?}").contains("already called"),
            "the refusal says why, not just no: {why:?}"
        );
        assert_eq!(state.name_of(node), "P-03", "and nothing moved");
        assert_eq!(state.node_of("P-01"), Some(taken));

        // A blank name is a different refusal, and also leaves the card named.
        super::rename_card(&state, node, "   ").expect_err("a blank name is not a name");
        assert_eq!(state.name_of(node), "P-03");
    });
}

/// R1682 — ★★★ the defect renaming forced, and the reason it is worth a test of
/// its own.
///
/// The node reset told an opening card from a palette-added one by asking
/// whether its NAME is in the specification — which is exactly the thing a
/// rename changes. A renamed opening card read as a stray, so the reset that
/// exists to put its name back would have DELETED it instead. Both halves are
/// asserted: the card survives, and it is called what it opened as.
#[test]
fn r1682_the_node_reset_puts_a_renamed_card_back_rather_than_deleting_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let node = state.node_of("P-03").expect("the specification has it");
        super::rename_card(&state, node, "edge-01").expect("the name is free");

        assert!(
            super::ResetScope::Nodes.changed(&state),
            "a renamed card is a changed node set, so the affordance is there"
        );

        super::ResetScope::Nodes.apply(&state);

        assert_eq!(
            state.cards().len(),
            spec::NODES.len(),
            "★ the card is still on the canvas — selecting strays by name \
             deleted it"
        );
        assert_eq!(state.name_of(node), "P-03", "and it is called what it was");
        assert_eq!(
            state.node_of("P-03"),
            Some(node),
            "the SAME card, not a fresh one wearing the name"
        );
        assert!(
            !super::ResetScope::Nodes.changed(&state),
            "and the scope reports nothing left to put back"
        );
    });
}

/// R1682 — deleting a card takes its links with it and gives back the seats
/// they were landing on.
///
/// The accept run keeps one slot per thing that lands on it (R1681.1), so a
/// deletion that removed the links without closing their slots would leave a
/// dead port on every node the deleted card dialled — the same defect from the
/// other end.
#[test]
fn r1682_deleting_a_card_frees_the_seats_its_links_were_landing_on() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        // ★★ A card that DIALS, and the assertion below that it does. The
        // first draft deleted `P-03`, which the opening graph only ever dials
        // INTO — so the seats it had to give back were all its own, the
        // closing branch was never reached, and the counterfactual that
        // removed that branch passed. A fixture that cannot reach the code is
        // a test that reads like coverage and is not.
        let node = state.node_of("P-01").expect("the specification has it");
        let outbound_onto_survivors = {
            let doc = state.doc.borrow();
            doc.tree(super::ROOT).map_or(0, |t| {
                t.links()
                    .iter()
                    .filter(|l| l.from.node == node && l.to.node != node)
                    .count()
            })
        };
        assert!(
            outbound_onto_survivors > 0,
            "★ this card dials somebody who survives it, or the seat-closing \
             branch is never reached and this test proves nothing"
        );
        // ★★ The SURPLUS, not the counts. A card that was dialled by the
        // deleted one legitimately loses a landing AND the seat that held it —
        // both counts move, and comparing them directly asserts that a correct
        // deletion changed nothing, which is false. What must not move is how
        // many seats a card holds that nothing lands on: the leak is a seat
        // left behind, and it shows up here and nowhere in a count.
        let spare = |state: &LabState| {
            state
                .cards()
                .into_iter()
                .filter(|n| *n != node)
                .map(|n| {
                    let doc = state.doc.borrow();
                    let arity = doc.signature(super::ROOT, n).map_or(0, |s| s.inputs.len());
                    let landed = doc
                        .tree(super::ROOT)
                        .map_or(0, |t| t.links().iter().filter(|l| l.to.node == n).count());
                    (state.name_of(n), arity.saturating_sub(landed))
                })
                .collect::<Vec<_>>()
        };
        let before = spare(&state);
        let touching = state.degree(node);

        super::delete_card(&state, node).expect("it is not the last card");

        assert_eq!(state.node_of("P-01"), None, "the card is gone");
        assert!(
            state.forms.borrow().get(&node).is_none(),
            "and so is its form"
        );
        assert!(
            state.opened_at.borrow().get(&node).is_none(),
            "and its placement, which nothing else would ever clean up"
        );
        assert_ne!(
            (touching.0 + touching.1),
            0,
            "the card this test deletes has links, or it proves nothing"
        );
        assert_eq!(
            spare(&state),
            before,
            "★ no card is left holding a seat nothing lands on — the links went \
             with the card and so did the ports that held them"
        );
    });
}

/// R1682 — the last card stays, and the refusal says so.
///
/// The reference refuses the same way. A canvas with no cards has no selection,
/// so the inspector, the gate panel and every affordance keyed to a selected
/// node vanish at once — a state a person reaches by pressing delete one time
/// too many and cannot leave.
#[test]
fn r1682_the_last_card_cannot_be_deleted() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let mut cards = state.cards();
        let keep = cards.pop().expect("the opening graph has cards");
        for node in cards {
            super::delete_card(&state, node).expect("not the last one yet");
        }
        assert_eq!(state.cards(), vec![keep]);

        super::delete_card(&state, keep).expect_err("★ the last one stays");
        assert_eq!(state.cards(), vec![keep], "and it really is still there");
        assert_eq!(
            state.active_card(),
            Some(keep),
            "so something is still selected and the inspector still has a \
             subject"
        );
    });
}

/// R1682 — the two switches are two facts, and the wire keeps them apart.
///
/// Collapsing is a LOOK and switching off is what the graph MEANS, so a screen
/// that folded them into one "state" word would make a reader guess which half
/// to trust. Driven through the same two functions the seats and the wire both
/// call.
#[test]
fn r1682_collapsing_a_card_and_switching_it_off_are_two_independent_facts() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let node = state.node_of("P-03").expect("the specification has it");
        let read = |state: &LabState| {
            let doc = state.doc.borrow();
            let slot = doc.tree(super::ROOT).and_then(|t| t.node(node)).unwrap();
            (slot.appearance.collapsed, slot.disabled, slot.bypassed)
        };
        assert_eq!(read(&state), (false, false, false), "it opens plain");

        super::collapse_card(&state, node).expect("the card is there");
        assert_eq!(
            read(&state),
            (true, false, false),
            "collapsing touches the look and nothing else"
        );

        super::disable_card(&state, node).expect("the card is there");
        assert_eq!(
            read(&state),
            (true, true, false),
            "★ and switching off is NOT bypass — a bypassed node passes its \
             input through, which is the opposite of what this tool means"
        );

        // Both are toggles, and each puts back only its own fact.
        super::collapse_card(&state, node).expect("the card is there");
        assert_eq!(read(&state), (false, true, false));
        super::disable_card(&state, node).expect("the card is there");
        assert_eq!(read(&state), (false, false, false));
    });
}

/// ★★ R1687 — **the two artifacts are one derivation**, and the test is what
/// says so: every node the document names appears in the script, in the same
/// order, and neither knows anything the other does not.
///
/// This is the claim that would rot first if the two renderings were written
/// separately, which is why the reference groups them and why doing one alone
/// was not an option.
#[test]
fn r1687_the_document_and_the_script_are_the_same_plan() {
    let owner = Owner::new();
    owner.run(|| {
        let state = state();
        let plan = state.plan();
        assert!(!plan.nodes().is_empty(), "the opening graph has cards");

        let document = plan_document(&plan);
        let script = plan.to_script().expect("this screen labels every card");

        let ordered: Vec<&str> = plan.nodes().iter().map(|e| e.name.as_str()).collect();
        let in_document: Vec<String> = document["order"]
            .as_array()
            .expect("an order")
            .iter()
            .map(|row| row["node"].as_str().expect("a name").to_string())
            .collect();
        assert_eq!(
            in_document, ordered,
            "the document renders the plan's order"
        );

        // The script writes one configuration file per node, and the order the
        // heredocs appear in is the order the plan is in.
        let heredocs: Vec<&str> = script
            .lines()
            .filter_map(|line| line.strip_prefix("cat > \"$OUT/"))
            .filter_map(|rest| rest.split(".json").next())
            .collect();
        assert_eq!(heredocs, ordered, "and so does the script");

        // Every host the plan spreads across gets a branch, and every node is
        // started inside exactly one of them.
        for host in plan.hosts() {
            assert!(
                script.contains(&format!("if [ \"$HOST\" = \"{host}\" ]; then")),
                "the script has no branch for {host}:\n{script}"
            );
        }
        for entry in plan.nodes() {
            let start = format!("\"$BIN/{}\" -c \"$OUT/{}.json\"", entry.program, entry.name);
            assert_eq!(
                script.matches(start.as_str()).count(),
                1,
                "{} is started exactly once:\n{script}",
                entry.name
            );
        }
    });
}

/// ★★★ R1687 — **a card switched off leaves both artifacts at once**, because
/// the order they are both rendered from is the model's.
///
/// The screen has no opinion here and that is the point: neither rendering
/// filters anything, so there is no way for one to drop the node and the other
/// to keep it.
#[test]
fn r1687_a_disabled_card_is_in_neither_artifact() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let node = state.node_of("P-03").expect("the card is there");
        let before = state.plan();
        assert!(
            before.nodes().iter().any(|e| e.name == "P-03"),
            "it starts in the plan"
        );

        super::disable_card(&state, node).expect("the card is there");
        let after = state.plan();
        assert!(
            !after.nodes().iter().any(|e| e.name == "P-03"),
            "a card that produces nothing is not started"
        );
        assert_eq!(
            after.nodes().len() + 1,
            before.nodes().len(),
            "and nothing else moved"
        );
        let script = after.to_script().expect("this screen labels every card");
        assert!(
            !script.contains("P-03"),
            "the script does not write a configuration for it either:\n{script}"
        );
    });
}

/// ★★★★ R1687 — **a row that cannot be expressed is reported, not dropped**,
/// and it reaches both the artifact and the sentence a person reads.
///
/// This is the whole reason `ConfigForm::compose` exists. Before it, a form
/// holding one unparseable value answered `document()` with an error and the
/// export had nothing to ship — so the choice was to ship nothing or to ship
/// silently. The third option is the one the reference takes and the one a
/// person can act on: ship what fits, and say what did not.
#[test]
fn r1687_a_value_that_cannot_be_expressed_is_carried_as_news() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let node = state
            .node_of(spec::SELECTED_NODE)
            .expect("the card is there");
        {
            let mut forms = state.forms.borrow_mut();
            let form = forms.get_mut(&node).expect("the card has a form");
            form.set("transport.link.tx.batch_size", "70000")
                .expect("the row is there");
        }

        let plan = state.plan();
        let uncarried = plan.uncarried();
        assert_eq!(uncarried.len(), 1, "one row, named");
        assert_eq!(uncarried[0].0, spec::SELECTED_NODE);
        assert_eq!(uncarried[0].1.key, "transport.link.tx.batch_size");

        // ★ The rest of that node's configuration still ships. A refusal that
        // took the other rows with it would make one bad value cost the file.
        let entry = plan
            .nodes()
            .iter()
            .find(|e| e.name == spec::SELECTED_NODE)
            .expect("still in the plan");
        assert!(
            entry
                .config
                .document
                .as_object()
                .is_some_and(|o| !o.is_empty()),
            "the node still has a configuration: {}",
            entry.config.document
        );
        assert!(
            entry
                .config
                .document
                .pointer("/transport/link/tx/batch_size")
                .is_none(),
            "and the refused row is not silently in it"
        );

        let document = plan_document(&plan);
        assert_eq!(
            document["uncarried"].as_array().map(Vec::len),
            Some(1),
            "the artifact carries it"
        );
        assert!(
            plan.to_script()
                .expect("this screen labels every card")
                .contains("not in any file above"),
            "and so does the script, as a comment somebody keeps"
        );

        // ★★ And the sentence says so, because the toast is what a person
        // reads. A clean count over a graph that will not start is the report
        // that would send somebody away with the wrong files.
        let said = deploy::export_sentence(&plan, Some("something is wrong"));
        assert!(said.contains("1 not expressed"), "{said}");
        assert!(said.contains("something is wrong"), "{said}");
    });
}

/// ★ R1687 — before either operation has run, the slot says so — and a null is
/// an answer where a missing key would be a question.
#[test]
fn r1687_nothing_is_produced_until_somebody_asks_for_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let wire = state.produced.borrow().wire();
        assert!(wire["config"].is_null() && wire["script"].is_null());
        assert!(
            wire.as_object().is_some_and(|o| o.len() == 2),
            "both halves are present as keys: {wire}"
        );

        super::export_configuration(&state);
        let wire = state.produced.borrow().wire();
        assert!(!wire["config"].is_null(), "the export landed");
        // ★ The slot is named `produced` and not `export` — it holds BOTH
        // artifacts, so a name taken from one of them would be wrong the moment
        // the other one moved it.
        assert!(
            wire["script"].is_null(),
            "★ and it did not produce the other one — two operations, two \
             artifacts, or the witness for each would be the same fact"
        );

        super::produce_script(&state);
        let wire = state.produced.borrow().wire();
        assert!(!wire["script"].is_null());
    });
}

/// ★★★★★ R1687 — **the toolbar's declared width is derived from what it
/// paints**, instead of being a sentence beside the layout.
///
/// [`super::TOOLBAR_RIGHT_CLUSTER`] is what the window's minimum width reserves
/// for the right-anchored seats. It said 300 from R1656 until this round, and
/// it had been wrong since R1678 put the view-reset seat 340 px in from the
/// right — nothing read the constant, so the layout outgrew it in silence and
/// the number stayed a claim about a screen that no longer existed.
///
/// This is the class [[debt-a-stated-limit-is-not-checked-by-anything]]: a
/// limit written in prose is re-derived by whoever needs it next, and the
/// original is never read again. So the requirement is now *measured* — every
/// right-anchored rectangle, asked how far in from the pane's right edge it
/// reaches — and the constant has to cover the furthest.
///
/// ★ It also asserts the cluster does not eat its sibling, because that is the
/// failure a reader would actually see: the two halves share the toolbar, and a
/// right cluster that grew past its declaration would paint over the launch-gate
/// chip rather than off the pane.
#[test]
fn r1791_the_toolbar_fits_at_the_floor_it_declares() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let owner = Owner::current().expect("this test runs inside a scope");
        pinion_core::reactive::VIEWPORT_SIZE
            .resolve(&owner)
            .set((MIN_W, super::MIN_H));
        // ★★★★★ **The round's own assertion: it is never cut.** At the declared
        // floor the row still fits, which is what `short_by == 0` means. A floor
        // that did not satisfy this is exactly the state a reader reported —
        // 1625 declared, 607 of cluster against 595 of room, seats painted past
        // the pane and the inspector clipped by 237.
        assert_eq!(
            super::right_cluster().short_by(),
            0,
            "at its own floor the toolbar still does not fit — {:?} on the row, \
             {:?} behind the control",
            super::right_cluster().shown(),
            super::right_cluster().moved()
        );
        assert_eq!(
            MIN_W,
            RAIL_W + PALETTE_W + super::TOOLBAR_RIGHT_FLOOR + TOOLBAR_LEFT_CLUSTER + INSP_W,
            "★ and the floor is DERIVED from the two clusters. ★★★★★ The sentence \
             that used to stand beside this said a 1366-wide laptop no longer \
             shows this screen unclipped and that adding a button was 'a real \
             cost'. Both are now false, and making them false is the round: the \
             right cluster gives groups up instead of demanding its whole width, \
             so a seat added to it moves what the toolbar WANTS and not what the \
             window must be."
        );
        // ★★ And the launch seat is on the row even here, because it is the one
        // that may not move — a floor that hid it would be the wrong trade
        // dressed as a fix.
        assert!(
            super::right_cluster()
                .shown()
                .iter()
                .any(|g| g.word() == "run"),
            "the launch seat moved at the floor: {:?}",
            super::right_cluster().shown()
        );
    });
}

#[test]
fn r1687_the_toolbars_declared_width_covers_what_it_paints() {
    let owner = Owner::new();
    owner.run(|| {
        // ★★ At the FLOOR, which is the only size this claim is about: the
        // shell is given `MIN_W` as the window's minimum and enforces it, so
        // the narrowest toolbar that can exist is the one `MIN_W` produces.
        // The first draft judged at whatever the default was (1440) and fired,
        // correctly reporting a 2 px overlap — at a width the application
        // cannot be shown at. A gate measuring a state the product cannot reach
        // is a gate that will be widened to shut it up.
        super::reset_lab_state();
        let owner = Owner::current().expect("this test runs inside a scope");
        pinion_core::reactive::VIEWPORT_SIZE
            .resolve(&owner)
            .set((MIN_W, super::MIN_H));
        // ★★★★★ R1909 — with the inspector OPEN, which is what this floor is
        // about. `MIN_W` is the narrowest window in which every pane fits
        // BESIDE the others, so it is a claim about the widest arrangement the
        // screen can be in; measured against a folded inspector the toolbar
        // simply takes the 294 px the fold gave up and the equality reads 886
        // against 592 — a true statement about a different question.
        let _open = super::WithTheInspectorOpen::now(&use_lab_state());
        let bar = super::toolbar_rect();
        assert_eq!(
            bar.w,
            super::TOOLBAR_RIGHT_FLOOR + TOOLBAR_LEFT_CLUSTER,
            "★ R1791 — at the floor the toolbar pane is the left cluster plus \
             the right one AT ITS NARROWEST: the seat that may not move and the \
             control holding the rest. It used to be the right cluster's full \
             width, which is what made the floor 1625 and cut the inspector."
        );
        let right = i64::from(bar.x) + i64::from(bar.w);
        // ★★★★ R1688 — **from the roster, not from a list written here.** R1687
        // wrote seven rectangles into this test, this round added an eighth
        // seat, and the gate went on measuring seven — green, and blind to
        // exactly the change it exists for. `toolbar_seats` is what the painter,
        // the hit test and the accessibility tree read, so a seat that is not in
        // it is not on the screen either.
        let state = super::use_lab_state();
        let seats = super::toolbar_seats(&state);
        // ★★★★★ R1791 — at the FLOOR, which is where this test runs, most of the
        // cluster is behind the overflow control by design. The roster is what
        // is on the row plus what the control holds, and both halves are
        // counted, because a seat that moved is still a seat.
        let held: usize = super::right_cluster()
            .moved()
            .iter()
            .map(|g| g.seats().len())
            .sum();
        assert!(
            seats.len() + held >= 8,
            "the toolbar's roster: {:?}, and {held} seat(s) behind the control",
            seats.iter().map(|s| s.tag).collect::<Vec<_>>()
        );
        let mut furthest = 0i64;
        // ★ The left cluster is measured the same way now. Its seats are
        // anchored to the pane's LEFT edge, so they are the ones that do not
        // reach in from the right — which is how the two halves are told apart
        // here without either being named.
        let mut left_reach = 0i64;
        for seat in &seats {
            let from_left = i64::from(seat.rect.x) + i64::from(seat.rect.w) - i64::from(bar.x);
            let reach = right - i64::from(seat.rect.x);
            if from_left <= i64::from(TOOLBAR_LEFT_CLUSTER) {
                left_reach = left_reach.max(from_left);
            } else {
                furthest = furthest.max(reach);
            }
        }
        assert!(
            left_reach > 0 && furthest > 0,
            "both halves have seats, or one of these two checks is vacuous"
        );
        assert!(
            left_reach <= i64::from(TOOLBAR_LEFT_CLUSTER),
            "the left cluster reaches {left_reach} px in and \
             TOOLBAR_LEFT_CLUSTER declares {TOOLBAR_LEFT_CLUSTER}"
        );
        // ★★★★★ R1791 — the same invariant against a DERIVED width. This used
        // to compare against `TOOLBAR_RIGHT_CLUSTER`, a hand-written 609 that
        // R1687 derived once and nothing re-derived; the cluster now says what
        // it needs from its own groups, so the check is "what it paints fits in
        // what it asked for" with no number in between. `wants` counts only the
        // groups actually ON the row, which is what makes this true at a narrow
        // size as well as a wide one — and the old form could not be, because a
        // constant cannot know that something moved.
        let wants = super::right_cluster_wants();
        assert!(
            furthest <= i64::from(wants),
            "the right-anchored cluster paints {furthest} px in and the groups \
             on the row need {wants} — a seat painted past what its group \
             declares is one painted off the pane at the minimum size"
        );
        assert!(
            i64::from(wants) - furthest <= 24,
            "and the groups declare {wants} for a cluster that paints \
             {furthest} — reserving room nothing uses makes the window bigger \
             than the screen requires"
        );

        // ★★ The two halves together ARE the window's minimum width — the
        // toolbar is what dictates it, not the canvas — so a seat added to
        // either half moves the smallest window this screen can be shown in,
        // and that should never be something a round discovers afterwards.
        assert!(
            furthest + left_reach <= i64::from(bar.w),
            "the two clusters need {} px and the toolbar pane is {} — they \
             would overlap, which is the launch-gate chip painted under the \
             view reset",
            furthest + left_reach,
            bar.w
        );
        // ★★★★★ R1688 — **and the pair the shell is HANDED is consistent.**
        // R1687 raised `MIN_W` past `WIN_W` and nothing said so:
        // `initial_size_strategy` asked for a window narrower than the minimum
        // it gave in the same call, and every headless probe laid this screen
        // out at a width the screen says it does not support. It survived on two
        // pixels of slack in the left cluster.
        //
        // ★ Asserted on what that function ANSWERS rather than on the two
        // constants: `WIN_W >= MIN_W` is now true by construction, so a test of
        // it folds to `assert!(true)` — clippy said so, which is the lesson
        // R1644.1 wrote down (an assertion that cannot fail reads like
        // coverage). This one can fail: the strategy could be given a literal
        // again.
        let pinion_shell::SizeStrategy::OpenResizable { size, min } =
            <super::NodeLabView as pinion_shell::WidgetView>::initial_size_strategy()
        else {
            panic!("this screen opens resizable, with a floor");
        };
        let min = min.expect("and the floor is declared");
        assert!(
            size.0 >= min.0 && size.1 >= min.1,
            "the window is asked to open at {size:?} with a minimum of {min:?} — \
             a window cannot open smaller than its own minimum, so one of the \
             two is a claim nothing can honour"
        );
        // ★★★★★ R1712 — the floor is no longer `MIN_W`, and that is the round's
        // point: `MIN_W` is where the LAYOUT stops and this is where the WINDOW
        // stops, 119 pixels lower, which is what puts this screen back on a
        // 1600-pixel display. Asserted against the policy rather than against
        // either constant, because the policy is the one place both are
        // written — an assertion on a repeated literal here would be checking
        // this test's copy against the binding's.
        assert_eq!(min, super::SHRINK.floor());
        assert_eq!(super::SHRINK.comfortable(), (MIN_W, super::MIN_H));
        assert!(
            super::SHRINK.concedes(),
            "this screen concedes width, and the gate for what that costs is \
             `tools/demos/r1712_a_window_says_what_it_gives_up.py`"
        );
    });
}

// ★ The scrolled-seat check lives in `painted.rs`, not here: it has to take
// the rectangle from the PAINT and ask the hit test about it, and a version
// written here would have had only `node_act_seat` to get a rectangle from —
// which is the function under test. The first draft did exactly that, aimed
// the press at the centre of what that function answered, and passed with the
// scroll offset removed from it. See
// `r1682_a_node_life_seat_is_pressable_where_it_is_painted`.

// ── R1688: where the canvas is pointed ──────────────────────────────────────

/// A live screen with the reactive hooks resolved, which the view operations
/// need: they read the canvas rectangle, which reads the window size.
fn live() -> std::rc::Rc<LabState> {
    super::reset_lab_state();
    super::use_lab_state()
}

/// ★★★ R1688 — **a fit puts every card and every frame inside the canvas**,
/// judged by asking where they are painted afterwards rather than by
/// re-deriving the fit's own arithmetic.
///
/// The rectangles come from the painter (`card_rect`, `frame_rect_of`, in world
/// coordinates) and the viewport comes from the layout, so the two sides of this
/// assertion are different derivations — the property R1682 and R1681.1 both
/// wrote down after an assertion that compared a function with itself.
#[test]
fn r1688_a_fit_brings_the_whole_graph_inside_the_canvas() {
    let owner = Owner::new();
    owner.run(|| {
        let state = live();
        // ★ The assumption the specification's `needs: None` rests on, asserted
        // rather than assumed: the opening graph does not already fit, so the
        // witness for this operation genuinely moves on the screen as it opens.
        let opened_at = state.zoom.get();
        assert_eq!(opened_at, spec::OPENING_ZOOM);

        // ★★★★★ **What the fit is OVER, asserted directly — because the
        // counterfactual that took the host frames out of it PASSED.**
        //
        // The outcome check below could not see that: a frame reaches only
        // `FRAME_PAD + FRAME_TAB` = 32 units past its members and the fit pads
        // by `FIT_PAD` = 60, so the padding covers the difference and
        // everything lands on screen either way. True today, and a silent
        // dependency on one constant being larger than two others — the moment
        // the padding is trimmed, a fit over the cards alone starts cutting the
        // frames and nothing would have said so.
        //
        // So the choice is stated where it can fail: the box `drawn_boxes`
        // answers is STRICTLY larger than the cards' own, on every side.
        let over = super::drawn_boxes(&state);
        let bounds = |boxes: &[((i32, i32), pinion_node_graph::Extent)]| {
            boxes.iter().fold(
                (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
                |(l, t, r, b), ((x, y), e)| {
                    (
                        l.min(*x),
                        t.min(*y),
                        r.max(x + e.width),
                        b.max(y + e.height),
                    )
                },
            )
        };
        let cards: Vec<_> = over.iter().take(state.cards().len()).copied().collect();
        let (cl, ct, cr, cb) = bounds(&cards);
        let (al, at, ar, ab) = bounds(&over);
        assert_eq!(
            over.len(),
            state.cards().len() + super::frames_of(&state).len(),
            "one box per card and one per host frame"
        );
        assert!(
            al < cl && at < ct && ar > cr && ab > cb,
            "the frames extend the fitted box on every side: cards \
             {cl},{ct}..{cr},{cb} against everything {al},{at}..{ar},{ab}"
        );

        let said = super::fit_view(&state);
        assert_ne!(
            state.zoom.get(),
            opened_at,
            "the opening zoom is not the fitting zoom: {said}"
        );
        assert!(said.starts_with("the whole graph"), "{said}");

        let canvas = canvas_rect();
        let (ox, oy) = super::world_offset(&state, state.pan.get());
        let mut checked = 0;
        let mut boxes: Vec<(String, super::Rect)> = Vec::new();
        for node in state.cards() {
            boxes.push((
                state.name_of(node),
                card_rect(&state, node).expect("a card"),
            ));
        }
        for (frame, name) in super::frames_of(&state) {
            boxes.push((name, super::frame_rect_of(&state, frame)));
        }
        assert!(boxes.len() >= spec::NODES.len() + spec::FRAMES.len());
        for (name, rect) in boxes {
            // World coordinates to the window, which is what a person sees.
            let left = i64::from(rect.x) - i64::from(ox) + i64::from(canvas.x);
            let top = i64::from(rect.y) - i64::from(oy) + i64::from(canvas.y);
            let right = left + i64::from(rect.w);
            let bottom = top + i64::from(rect.h);
            assert!(
                left >= i64::from(canvas.x)
                    && top >= i64::from(canvas.y)
                    && right <= i64::from(canvas.x + canvas.w)
                    && bottom <= i64::from(canvas.y + canvas.h),
                "{name} is painted {left},{top}..{right},{bottom} and the canvas \
                 is {canvas:?} — a fit that leaves part of the graph off screen \
                 is the one thing this operation must not do"
            );
            checked += 1;
            let _ = (right, bottom);
        }
        assert!(checked >= 10, "{checked} boxes were judged");

        // ★★★ And it is CENTRED. Containment alone would pass for a fit that
        // pinned the graph to a corner, and it would also pass for one that
        // rounded the scale to a whole percent and kept the pan computed at the
        // unrounded one — which is the R1684.4 error shape (a derivation
        // rounded on one axis and not the other, worse the further from the
        // origin). The gutters left and right have to match.
        let mut left = i64::MAX;
        let mut right = i64::MIN;
        for node in state.cards() {
            let rect = card_rect(&state, node).expect("a card");
            left = left.min(i64::from(rect.x));
            right = right.max(i64::from(rect.x) + i64::from(rect.w));
        }
        for (frame, _) in super::frames_of(&state) {
            let rect = super::frame_rect_of(&state, frame);
            left = left.min(i64::from(rect.x));
            right = right.max(i64::from(rect.x) + i64::from(rect.w));
        }
        let before = left - i64::from(ox);
        let after = i64::from(canvas.w) - (right - i64::from(ox));
        // ★★★★★ R1791 — the invariant, stated instead of a tolerance tuned to
        // one canvas width. The two margins share whatever the canvas has left
        // over, so when that leftover is ODD they cannot be equal and the best
        // achievable difference is 1. What this asserts is that each side is
        // within a pixel of its ideal half — which is the rounding a painted
        // integer rectangle can introduce, and is true at any width. The old
        // form allowed a difference of 2 and failed at 3 the moment the floor
        // moved, which is a tolerance measuring the window rather than the fit.
        let leftover = before + after;
        let ideal_twice = leftover; // each side's ideal is leftover / 2
        assert!(
            (2 * before - ideal_twice).abs() <= 3 && (2 * after - ideal_twice).abs() <= 3,
            "the graph sits {before} px from the left of the canvas and {after} \
             from the right, sharing {leftover} — each side's ideal is \
             {ideal_twice} halved, and neither may be more than 1.5 px off it"
        );
    });
}

/// ★★★ R1688 — **a fit is idempotent**, which is the property the reference is
/// documented not to have (its own advice is to call its fit twice).
///
/// Pressing it a second time must answer the same camera. The measurement that
/// makes this non-trivial is the one above it: the fit is computed from the
/// cards' sizes in canvas units, and those are asked at a stated scale rather
/// than measured off the screen and divided back out — so the answer does not
/// depend on where the view happened to be.
#[test]
fn r1688_fitting_a_fitted_graph_does_not_move_it() {
    let owner = Owner::new();
    owner.run(|| {
        let state = live();
        super::fit_view(&state);
        let settled = (state.zoom.get(), state.pan.get());
        super::fit_view(&state);
        assert_eq!((state.zoom.get(), state.pan.get()), settled);

        // And it does not depend on where you were looking when you asked: from
        // a different zoom and a panned view, the same camera.
        super::zoom_to(&state, super::ZOOM_MIN);
        state.pan.set((-317, 208));
        super::fit_view(&state);
        assert_eq!(
            (state.zoom.get(), state.pan.get()),
            settled,
            "★ frame-the-graph is a function of the GRAPH. Measuring the cards \
             on screen and dividing the zoom back out would make it a function \
             of the view as well, and pressing it from two places would answer \
             two cameras"
        );
    });
}

/// ★★★★ R1688 — **a graph the zoom range cannot hold says so**, and the
/// sentence is the one the reference has no way to produce.
///
/// Driven by moving a card far enough out that the floor cannot shrink the
/// graph into the pane, which is a state a person can reach by dragging.
#[test]
fn r1688_a_graph_too_large_to_frame_is_reported_rather_than_pretended() {
    let owner = Owner::new();
    owner.run(|| {
        let state = live();
        // ★ At the FLOOR window, which is a size the product genuinely has —
        // R1687 wrote down that a gate measuring a state the application cannot
        // reach is a gate that gets widened to shut it up. A canvas 260 tall and
        // a card dragged 1,200 units down is a graph the 25% floor cannot hold.
        let owner = Owner::current().expect("this test runs inside a scope");
        pinion_core::reactive::VIEWPORT_SIZE
            .resolve(&owner)
            .set((MIN_W, super::MIN_H));
        let far = state.node_of("P-03").expect("on the canvas");
        {
            let mut doc = state.doc.borrow_mut();
            if let Some(tree) = doc.tree_mut(super::ROOT)
                && let Some(node) = tree.node_mut(far)
            {
                node.x = 600;
                node.y = 1_200;
            }
        }
        let said = super::fit_view(&state);
        assert!(
            said.starts_with("as much as") && said.contains("wider than the view"),
            "{said}"
        );
        assert_eq!(
            state.zoom.get(),
            super::ZOOM_MIN,
            "it goes as far out as the range allows and stops there"
        );
    });
}

/// ★★★ R1688 — **the jump goes to the card the first finding is on**, and the
/// finding it names is the one the gate panel shows first.
///
/// Both halves matter. Selecting *a* card with a problem would pass a check that
/// only asked whether the selection moved; what makes this the reference's
/// operation is that it is the FIRST one, in the order a reader meets them.
#[test]
fn r1688_the_jump_lands_on_the_card_the_first_finding_is_on() {
    let owner = Owner::new();
    owner.run(|| {
        let state = live();
        // ★ The assumption the specification's `needs: None` rests on: the
        // opening screen HAS a finding, and it is not on the card the screen
        // opens with — so the witness moves without anything being caused
        // first.
        let problems = state.problems();
        assert!(!problems.is_empty(), "the opening graph has findings");
        let first = problems.first().expect("checked");
        let target = first.node.expect("the finding names a card");
        assert_ne!(
            Some(target),
            state.active_card(),
            "and it is not the card the screen opens on"
        );

        let said = super::go_to_problem(&state);
        assert_eq!(state.active_card(), Some(target));
        assert_eq!(said, first.sentence);
        assert_eq!(
            state.toast.showing().map(|said| said.sentence()),
            Some(first.sentence.clone()),
            "and the person is told which finding they were taken to"
        );
        // ★★ The panel and the jump are ONE walk. A second derivation of "what
        // is wrong first" is the thing `problems` exists to prevent, and this
        // is what would notice it coming back.
        assert_eq!(
            state.gate_lines().first().map(|(_, line)| line.clone()),
            Some(first.sentence.clone())
        );
    });
}

/// ★★★ R1688 — **the jump brings the card into view when it is off screen, and
/// leaves the view alone when it is not.**
///
/// Past the reference, which only moves the selection: a graph panned away from
/// the card being named leaves the person told about something they cannot see.
/// Minimal, so it does not throw away a view somebody chose on purpose.
#[test]
fn r1688_the_jump_reveals_the_card_only_when_it_has_to() {
    let owner = Owner::new();
    owner.run(|| {
        let state = live();
        super::fit_view(&state);
        let framed = state.pan.get();
        super::go_to_problem(&state);
        assert_eq!(
            state.pan.get(),
            framed,
            "everything is on screen after a fit, so a jump moves nothing"
        );

        // Now pan the card off the left edge and ask again.
        state.pan.set((framed.0 - 1_400, framed.1));
        super::select_card(&state, None);
        super::go_to_problem(&state);
        assert_ne!(state.pan.get(), (framed.0 - 1_400, framed.1));
        let target = state
            .problems()
            .first()
            .and_then(|p| p.node)
            .expect("a finding on a card");
        let rect = card_rect(&state, target).expect("painted");
        let canvas = canvas_rect();
        let (ox, oy) = super::world_offset(&state, state.pan.get());
        let left = i64::from(rect.x) - i64::from(ox) + i64::from(canvas.x);
        let top = i64::from(rect.y) - i64::from(oy) + i64::from(canvas.y);
        assert!(
            left >= i64::from(canvas.x)
                && left + i64::from(rect.w) <= i64::from(canvas.x + canvas.w)
                && top >= i64::from(canvas.y)
                && top + i64::from(rect.h) <= i64::from(canvas.y + canvas.h),
            "the card is at {left},{top} and the canvas is {canvas:?}"
        );
    });
}

/// ★★★ R1688 — **a zoom step keeps the middle of the view still**, which is the
/// reference's own behaviour and was not this screen's.
///
/// Before this round the zoom changed the scale and left the pan, which anchors
/// at the canvas ORIGIN: zooming out from a graph you had panned to walked it
/// off the top-left corner. The check is on the canvas point under the middle
/// pixel, read through the screen's own conversion, so a stepper that anchored
/// anywhere else fails.
#[test]
fn r1688_a_zoom_step_is_anchored_at_the_middle_of_the_canvas() {
    let owner = Owner::new();
    owner.run(|| {
        let state = live();
        // Panned, because at pan zero the two anchors agree and the fixture
        // could not tell a centre anchor from an origin one.
        state.pan.set((-220, 140));
        let canvas = canvas_rect();
        let middle = (canvas.x + canvas.w / 2, canvas.y + canvas.h / 2);
        let before = super::to_canvas(&state, middle.0, middle.1);
        for up in [true, false, true, true] {
            super::zoom_to(&state, super::zoom_stepped(&state, up));
            let after = super::to_canvas(&state, middle.0, middle.1);
            assert!(
                (after.0 - before.0).abs() <= 2 && (after.1 - before.1).abs() <= 2,
                "the canvas point under the middle moved from {before:?} to \
                 {after:?} at {}%",
                state.zoom.get()
            );
        }
        assert_ne!(state.pan.get(), (-220, 140), "and the pan did move");
    });
}

/// ★★★★★ R1689 — **every toolbar caption reserves the line its face needs.**
///
/// Found by looking at the screen, which is a round obligation and earned its
/// place again here: `seat_caption` took a guessed inset off both edges, and on
/// a 24-high seat that left 12 px for an 11 px face whose line box reserves 18.
/// The `p` of `open` was painted with its descender cut off at the border. Every
/// seat already at that height carries `-`, `+`, `84%` or `fit` — not one of
/// them has a descender — so no gate had ever been given the chance.
///
/// This asks the RESERVATION, which is the half a view function can settle
/// without a shaper. Whether the shaped ink then fits inside the run's own rect
/// is a different question and a registered one.
#[test]
fn r1689_every_toolbar_caption_reserves_its_line() {
    let owner = Owner::new();
    owner.run(|| {
        let state = live();
        let line = pinion_core::containment::line_box(super::FONT_SMALL);
        let short: Vec<String> = super::toolbar_seats(&state)
            .iter()
            .map(|seat| (seat.tag, super::seat_caption(seat.rect), seat.rect))
            .filter(|(_, caption, _)| caption.h < line)
            .map(|(tag, caption, rect)| {
                format!("{tag}: caption {}px tall in a {}px seat", caption.h, rect.h)
            })
            .collect();
        assert!(
            short.is_empty(),
            "★ a caption box shorter than the face's line box paints its \
             descenders into the border — found by LOOKING, on the seat that \
             was the toolbar's first {line}px-face word with a `p` in it. \
             {} seat(s) reserve less than {line}px:\n  {}",
            short.len(),
            short.join("\n  ")
        );
    });
}

/// ★★ R1688 — every seat of the toolbar answers a press aimed at it, and every
/// one of them is named, **from the one roster all three read**.
///
/// The reachability sweep in `painted.rs` asks this of the painted scene; this
/// asks it of the roster itself, so a seat that is in the roster and not painted
/// fails there and a seat that is painted and not in the roster has no name
/// here. The two directions are what make the roster a roster.
#[test]
fn r1688_the_toolbar_roster_is_pressable_and_named() {
    let owner = Owner::new();
    owner.run(|| {
        let state = live();
        // ★★★★★ R1791 — the roster is a function of the room the toolbar has,
        // so this counts the seats a person can aim at PLUS the groups the
        // control is holding. Measured: the cluster needs 607 and gets 410 at
        // this screen's DESIGN width, where two groups therefore move; driven
        // through the wire the same day, the row is whole again at 1696. So the
        // total is what is invariant and the split between the two terms is not
        // — which is why this asserts on the sum rather than on either half.
        //
        // ★ An earlier draft of this comment said a whole row "does not exist at
        // any size this screen runs at". That was measured at one width and
        // stated about all of them, and driving it found 1696.
        let seats = super::toolbar_seats(&state);
        let held = super::right_cluster().moved().len();
        assert!(
            seats.len() + held >= 8,
            "{} seats on the row and {held} group(s) behind the control",
            seats.len()
        );
        for seat in &seats {
            assert!(!seat.name.trim().is_empty(), "{} has no name", seat.tag);
            // Every corner, not the centre: a rectangle one pixel out is
            // invisible to a centre probe on a seat this size (R1684).
            for (px, py) in [
                (seat.rect.x, seat.rect.y),
                (seat.rect.x + seat.rect.w - 1, seat.rect.y),
                (seat.rect.x, seat.rect.y + seat.rect.h - 1),
                (seat.rect.x + seat.rect.w - 1, seat.rect.y + seat.rect.h - 1),
            ] {
                assert_eq!(
                    Hit::at(&state, px, py),
                    seat.hit,
                    "{} does not answer at ({px}, {py}) — its seat is {:?}",
                    seat.tag,
                    seat.rect
                );
            }
        }
        // And they do not overlap, which is what makes the order above a
        // reading order rather than a priority.
        for (n, a) in seats.iter().enumerate() {
            for b in &seats[n + 1..] {
                assert!(
                    !(a.rect.x < b.rect.x + b.rect.w
                        && b.rect.x < a.rect.x + a.rect.w
                        && a.rect.y < b.rect.y + b.rect.h
                        && b.rect.y < a.rect.y + a.rect.h),
                    "{} and {} overlap: {:?} {:?}",
                    a.tag,
                    b.tag,
                    a.rect,
                    b.rect
                );
            }
        }
    });
}

/// ★★★★★ R1902 — **every pane opens where its own policy admits.**
///
/// The hole this closes, measured at R1902: every *change* to a placement went
/// through `EdgePolicy::admit` / `admit_fold` / `admit_extent`, and the
/// placement the screen **started in** went through nothing at all — it was two
/// `const`s in the painter. So a pane could declare `foldable: false` and open
/// folded, or open on an edge its own `allowed` list excludes, and the
/// contradiction would stand for the life of the program with no call to blame
/// and no moment to watch.
///
/// The gate does not re-spell the rules. It asks the *policy* whether it admits
/// its own declared opening, which is the same function the gestures go
/// through — so a rule that changes changes here too, and a fourth spelling of
/// "is this edge allowed" never gets written.
#[test]
fn r1902_every_pane_opens_where_its_own_policy_admits() {
    let mut checked = 0;
    let mut folded_open = 0;
    for pane in spec::PANES {
        let admitted = pane.policy.admit_opening(pane.opens).unwrap_or_else(|why| {
            panic!(
                "{} opens at {:?}, which its own policy refuses: {}",
                pane.tag,
                pane.opens,
                why.reason()
            )
        });
        assert_eq!(
            admitted, pane.opens,
            "{}: an admitted opening is returned unchanged",
            pane.tag
        );
        checked += 1;
        if pane.opens.folded {
            folded_open += 1;
            assert!(
                pane.policy.foldable,
                "{}: a pane that opens folded must declare it folds",
                pane.tag
            );
        }
    }
    // A floor on the population, because a `PANES` emptied by accident would
    // make every assertion above vacuous and this test would still pass — the
    // sweep-floor rule R1802 wrote one gate over.
    assert_eq!(
        checked,
        spec::PANES.len(),
        "every declared pane was judged; judged {checked}"
    );
    assert_eq!(checked, 4, "this screen declares four panes");
    // 🟥🟥🟥★★★★★ R1909 — this assertion was `folded_open == 0`, justified as
    // "the canon opens its palette open; a pane that starts folded here would
    // be a second-pass change that un-reproduces it".
    //
    // **The measurement was right and the generalisation was not.** What R1902
    // extracted is that the canon's DASHBOARD SHELL initialises `paletteOpen:
    // true` — and that drawer lives beside `state.widgets`, which is the
    // dashboard's model. The canon has NO OPINION about this screen's panes:
    // no state, no toggle, nothing. So "the canon opens it open" is a fact
    // about a different panel, and turning it into a floor over ALL FOUR panes
    // here made a second-pass improvement unbuildable by assertion — which is
    // exactly what happened for six rounds.
    //
    // ⇒ ★★★★★ *a measurement about one subject, asserted over a population, is
    // no longer a measurement.* The population it covers is the population it
    // was taken from.
    //
    // What is asserted instead is the property that actually protects this
    // screen, and it is stronger: a pane that opens folded must be one a reader
    // can bring BACK. A fold whose panel declares it does not fold is a panel
    // gone with no strip to grab, which is the `hide`/`fold` distinction this
    // whole axis is built on — and that check is in the loop above, applied to
    // every pane that opens folded rather than to a count of them.
    assert!(
        spec::PANES.iter().any(|p| !p.opens.folded),
        "at least one pane opens showing, or this screen opens as a bare canvas \
         with nothing to say what it is"
    );
    // 🟥🟥🟥★★★★★ R1911.1 — **and the count itself is gone, because the
    // paragraph above forbids it and then wrote one anyway.**
    //
    // R1902's `folded_open == 0` and R1909's `folded_open == 1` are the same
    // move in opposite directions: a fact about this build's arrangement,
    // asserted as a property of the screen, so whoever changed the arrangement
    // next had to edit the gate to say the new number — which is a gate that
    // records a choice instead of protecting one. Measured at R1911, R1909's
    // choice took 33 demo walks red for three CI rounds and this assertion
    // could not see any of it: it was busy agreeing with the line that caused
    // them.
    //
    // What protects this screen is in the loop above, applied per pane rather
    // than to a count: an opening its own policy admits, and — for a pane that
    // opens folded — a declaration that it folds, so a reader has a strip to
    // grab. Which panes those are is the build's business, and the walks are
    // what judge it.
    let _ = folded_open;
}

/// ★★★★★ R1802 — **the specification says where a panel may live; the layout
/// has to be able to put it there.**
///
/// R1801 published the policy and left the layout drawing from a `const`, so
/// the screen advertised "the palette may live on the left or the right" while
/// always painting it on the left — and nothing compared the two. Its own
/// closing audit recorded that as a defect it had created. This is the gate.
///
/// Not "does the palette move": that a value changes is trivial. The claim is
/// that **every edge the specification admits is an edge the layout actually
/// honours**, so a policy widened to an edge the layout cannot place lands here
/// instead of on a reader's screen.
#[test]
fn r1802_every_edge_the_specification_admits_is_one_the_layout_honours() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let (w, _) = super::window_size();

        let mut checked = 0;
        for pane in spec::PANES {
            let Some(seat) = ["lab.palette", "lab.inspector"]
                .iter()
                .position(|t| *t == pane.tag)
            else {
                // Not a movable seat. Its policy must say so, which is the
                // other half of the same agreement.
                assert!(
                    pane.policy.allowed.is_empty(),
                    "{} admits {:?} and the layout has nowhere to put it",
                    pane.tag,
                    pane.policy.allowed
                );
                continue;
            };
            for &edge in pane.policy.allowed {
                let at = pinion_core::edge_panel::EdgePlacement::open(edge, pane.width);
                if seat == 0 {
                    state.palette_at.set(at);
                } else {
                    state.inspector_at.set(at);
                }
                // The write has to be VISIBLE to the layout before the
                // rectangle means anything — the layout reads the thread-local
                // state, not this handle, and a test that asserted a rectangle
                // without checking that would be measuring the default.
                let seen = super::placements();
                let seen_edge = if seat == 0 { seen.0.edge } else { seen.1.edge };
                assert_eq!(
                    seen_edge, edge,
                    "{} was placed at {edge:?} and the layout still reads {seen_edge:?}",
                    pane.tag
                );
                let rect = if seat == 0 {
                    palette_rect()
                } else {
                    inspector_rect()
                };
                // ★ "The layout honours the edge" is BAND CONTAINMENT, not a
                // fixed coordinate. The first draft asserted `x == RAIL_W` for
                // the left and failed at 284 — which was the layout being
                // RIGHT: with both panels declared left, the inspector stacks
                // after the palette. An assertion that only holds when nothing
                // else shares the edge is an assertion about one arrangement,
                // not about the rule.
                let (left, right) = super::side_bands();
                let (lo, hi) = match edge {
                    pinion_core::style::ChromeEdge::Left => (RAIL_W, RAIL_W + left),
                    pinion_core::style::ChromeEdge::Right => (w - right, w),
                    other => panic!(
                        "{} admits {other:?}, which this screen's layout does not place",
                        pane.tag
                    ),
                };
                assert!(
                    rect.x >= lo && rect.x + rect.w <= hi,
                    "{} declares it may sit at {edge:?}; the layout put it at {}..{} \
                     and that edge's band is {lo}..{hi}",
                    pane.tag,
                    rect.x,
                    rect.x + rect.w
                );
                assert_eq!(rect.w, pane.width, "{} lost its width moving", pane.tag);
                checked += 1;
            }
            // Put the panels back through the SAME handle. ★ This used to call
            // `reset_lab_state()`, which clears the thread-local the layout
            // reads while leaving `state` pointing at the orphaned one — so the
            // writes went somewhere nothing looked at and the layout answered
            // its defaults. It failed, but it failed BLAMING THE LAYOUT, and
            // the probe above is the only reason that was not "fixed" in the
            // layout instead. A reset that empties a thread-local invalidates
            // every handle taken before it.
            state.palette_at.set(super::palette_opens_at());
            state.inspector_at.set(super::inspector_opens_at());
        }
        // A floor on the SWEEP, because a policy emptied by accident would make
        // every assertion above vacuous and the test would still pass.
        assert_eq!(
            checked, 4,
            "two movable panes times two admitted edges is four placements; checked {checked}"
        );
    });
}

/// ★ A fold leaves the strip, and the canvas does not swallow it.
///
/// The floor toolkit has no fold at all — hiding takes a panel out of the
/// layout, so there is nothing left to grab. The whole point of the strip is
/// that the person who folded it can unfold it, which is only true if the
/// canvas stops short of it.
#[test]
fn r1802_a_folded_panel_leaves_a_strip_the_canvas_does_not_take() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();

        let open_canvas = canvas_rect();
        let open_palette = palette_rect();
        assert_eq!(open_palette.w, PALETTE_W);

        let mut folded = state.palette_at.get();
        folded.folded = true;
        state.palette_at.set(folded);

        let strip = palette_rect();
        assert_eq!(strip.w, PANEL_STRIP_W, "a fold leaves the strip");
        assert_eq!(strip.x, open_palette.x, "on the same edge it was on");

        let canvas = canvas_rect();
        assert_eq!(
            canvas.w,
            open_canvas.w + PALETTE_W - PANEL_STRIP_W,
            "the canvas takes exactly what the fold gave up, and no more"
        );
        assert!(
            canvas.x >= strip.x + strip.w,
            "the canvas starts after the strip: canvas {canvas:?}, strip {strip:?}"
        );
        // Through the handle, not through a thread-local reset — see the note
        // in the gate above for why that difference cost a diagnosis.
        state.palette_at.set(super::palette_opens_at());
    });
}

// ── R1887: a panel a person can actually place ──────────────────────────────

/// ★★★★★ R1887 — **pressing the header's control moves the panel, and
/// everything derived from the placement follows it.**
///
/// # The defect this closes, measured at entry
///
/// R1802 made the placement a value and the layout honour it, and stopped
/// there. Measured at this round's entry: outside this file, `palette_at` and
/// `inspector_at` had **no writer in the tree** — so the honest answer to the
/// reader who asked three times why these panels cannot be moved had become
/// *they can, and nobody can*. This is the press.
///
/// # What it asserts beyond "the value changed"
///
/// The canvas AND the toolbar. `toolbar_rect` read the opening widths directly
/// until this round — a divergence R1802 recorded as not yet alive because
/// nothing could move a panel, which is a defect with a date, and the date is
/// the round that builds the missing thing. With the palette on the right and a
/// toolbar computed from the opening widths, the toolbar starts under the rail.
#[test]
fn r1887_pressing_the_header_control_moves_the_panel_and_the_layout_follows() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();

        let opened_at = state.palette_at.get().edge;
        let opening_toolbar = super::toolbar_rect();
        let opening_canvas = canvas_rect();

        // Pressed where it is painted, through the screen's own hit test — the
        // only way that proves a person can reach it.
        let control = super::side_panel_control(palette_rect(), 0);
        let pane = palette_rect();
        let at = (
            pane.x + control.x + control.w / 2,
            pane.y + control.y + control.h / 2,
        );
        // ★ R1950 — the expected hit is built from the panel's own roster, so
        // this says *the first control the policy offers* rather than naming an
        // act the paint might not be drawing there.
        let offered = super::SidePanel::Palette
            .controls(&state)
            .next()
            .expect("a movable, foldable palette offers a flip first");
        assert_eq!(offered.act, pinion_core::edge_panel::PanelAffordance::Flip);
        assert_eq!(
            super::Hit::at(&state, at.0, at.1),
            super::Hit::Panel(super::SidePanel::Palette, super::PanelAct::Offered(offered)),
            "the flip control is what a press at its painted centre reaches"
        );
        super::move_cursor(&state, at.0, at.1);
        super::press(&state);
        super::release(&state);

        let moved_to = state.palette_at.get().edge;
        assert_ne!(moved_to, opened_at, "the press moved the palette");
        assert!(
            super::SidePanel::Palette.spec().policy.admits(moved_to),
            "and moved it to an edge the specification admits: {moved_to:?}"
        );

        // ★ The two derivations that must follow it. The canvas already did
        // (R1802); the toolbar is this round's.
        let toolbar = super::toolbar_rect();
        let canvas = canvas_rect();
        assert_ne!(
            toolbar.x, opening_toolbar.x,
            "★ the toolbar still starts where it did with the palette on the \
             other side — it is reading the opening widths rather than the \
             placement: {toolbar:?}"
        );
        assert_eq!(
            (toolbar.x, toolbar.w),
            (canvas.x, canvas.w),
            "the toolbar spans exactly the canvas's column, whichever edge the \
             panels are on"
        );
        assert_ne!(canvas.x, opening_canvas.x, "the canvas moved with it");
    });
}

/// ★★★★★ R1889 — **dragging the grip changes the width, and every derivation
/// follows it.**
///
/// The width counterpart of the edge assertion above, and it exists because
/// this round's own debt named the trap: `toolbar_rect` read the OPENING widths
/// until R1887 made panels movable, and ten rectangles inside the inspector
/// still read the opening width until this round made them resizable. *A latent
/// divergence is a defect with a date, and the date is the round that builds
/// the missing thing.*
///
/// So this drags and then asks the three things that must have moved: the
/// canvas, the toolbar, and the panel's own body — the third being the one
/// R1887 had no reason to check.
#[test]
fn r1889_dragging_the_grip_resizes_the_panel_and_every_derivation_follows() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        // ★ R1909 — a folded panel offers no grip: its whole strip is one
        // affordance, and that is the state's own gate next door rather than a
        // hole in this one. This gate is about the grip, so it says so.
        let _open = super::WithTheInspectorOpen::now(&state);

        let opening = state.inspector_at.get().extent;
        let opening_canvas = canvas_rect();
        let opening_body = super::inspector_body_w();

        // Pressed where it is painted, through the screen's own hit test.
        let pane = inspector_rect();
        let grip = super::side_panel_grip(pane, state.inspector_at.get());
        let at = (pane.x + grip.x + grip.w / 2, pane.y + grip.y + grip.h / 2);
        assert_eq!(
            super::Hit::at(&state, at.0, at.1),
            super::Hit::PanelGrip(super::SidePanel::Inspector),
            "the grip is what a press at its painted centre reaches"
        );

        super::move_cursor(&state, at.0, at.1);
        super::press(&state);
        // The inspector is on the right, so dragging LEFT widens it. Derived
        // from the edge rather than assumed, so this still reads correctly if
        // the opening placement is ever flipped.
        let widen_by = 60;
        let to = match state.inspector_at.get().edge {
            pinion_core::style::ChromeEdge::Left => at.0 + widen_by,
            _ => at.0 - widen_by,
        };
        super::move_cursor(&state, to, at.1);
        super::release(&state);

        let now = state.inspector_at.get().extent;
        assert!(
            now > opening,
            "the drag widened the inspector: {opening} -> {now}"
        );
        assert!(
            super::SidePanel::Inspector.spec().policy.resize.clamp(now) == Some(now),
            "and landed inside the range the specification declares"
        );

        // ★★★★★ The three derivations. The body is the one this round moved:
        // before it, these rectangles read `INSP_W` and would have stayed at
        // the opening width while the panel around them grew.
        assert!(
            super::inspector_body_w() > opening_body,
            "★ the inspector's BODY grew with it — ten rectangles inside this \
             pane used to state their width from the opening constant, which is \
             exactly the divergence that goes live the round a drag exists"
        );
        assert_eq!(
            super::inspector_body_w(),
            inspector_rect().w - super::PAD * 2,
            "and it is one derivation from the pane, not a second number"
        );
        let canvas = canvas_rect();
        assert!(
            canvas.w < opening_canvas.w,
            "the canvas gave up exactly what the panel took"
        );
        assert_eq!(
            (super::toolbar_rect().x, super::toolbar_rect().w),
            (canvas.x, canvas.w),
            "and the toolbar still spans the canvas's column"
        );
    });
}

/// ★★★★★ R1889 — **the specification decides which panels resize, and the
/// screen is held to BOTH directions of that.**
///
/// The width twin of R1802's edge census, and it is an equality rather than a
/// spot check for the reason that round recorded: a gate that only asserts the
/// positive case passes a screen that offers grips on everything.
///
/// ⚠ The population floor is what keeps it from going vacuous. If a
/// specification edit ever made every pane fixed, every loop below would run
/// zero times and this test would pass while nothing on the screen resized.
#[test]
fn r1889_every_pane_that_declares_a_resize_offers_a_grip_and_no_other_does() {
    use pinion_core::edge_panel::Resize;

    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        // ★ R1909 — the census is over panels that are SHOWING. A folded panel
        // offering no grip is correct and is asserted next door; counted here
        // it would make this equality say "a pane that declares a resize offers
        // a grip unless it happens to be folded", which is not a rule anybody
        // could act on.
        let _open = super::WithTheInspectorOpen::now(&state);

        let mut draggable = 0usize;
        let mut fixed = 0usize;

        for which in [super::SidePanel::Palette, super::SidePanel::Inspector] {
            let declared = which.spec().policy.resize;
            let offers = super::side_panel_has_grip(&state, which);
            match declared {
                Resize::Between { min, max } => {
                    draggable += 1;
                    assert!(
                        offers,
                        "{} declares it resizes between {min} and {max} and the \
                         screen offers no grip",
                        which.word()
                    );
                    assert!(
                        min < max,
                        "a range whose ends meet is a fixed panel \
                                        spelled the long way"
                    );
                    // The opening width must be reachable, or the panel opens
                    // outside its own declared range and the first drag jumps.
                    let opening = which.at(&state).extent;
                    assert!(
                        (min..=max).contains(&opening),
                        "{} opens at {opening}, outside its own declared \
                         {min}..={max}",
                        which.word()
                    );
                }
                Resize::Fixed => {
                    fixed += 1;
                    assert!(
                        !offers,
                        "{} declares a fixed width and the screen offers a grip \
                         anyway — an affordance that cannot do anything",
                        which.word()
                    );
                }
            }
        }

        // ★ The floor R1802 taught: a census over a declared set is only worth
        // running while the set is not empty.
        assert!(
            draggable > 0,
            "no pane declares a resize, so every assertion above ran zero times"
        );
        let _ = fixed;

        // And the rail, which is neither of the two placeable panels, declares
        // `fixed()` — so the vocabulary reaches a pane that genuinely must not
        // move rather than only the two that may.
        assert!(
            !spec::PANES[0].policy.resize.is_draggable(),
            "the rail's width is the specification's"
        );
    });
}

/// ★★★★★ R1887 — **a fold is reversible by the person who did it.**
///
/// ⚠ The entry re-measurement sharpened the record rather than confirming it.
/// The debt said the strip was drawn and had no control in it; measured, the
/// fold was **geometric only** — nothing in the paint branched on `folded` at
/// all, so a folded panel kept its whole body and painted it into eighteen
/// pixels. Both halves are here now: the strip is what a folded panel paints,
/// and pressing it is what brings the panel back.
///
/// The floor has no fold — its nearest gesture removes the panel from the
/// layout, leaving a reader nothing to press.
#[test]
fn r1887_a_folded_panel_is_a_strip_a_press_brings_back() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();

        let open = palette_rect();
        let fold = super::side_panel_control(open, 1);
        let at = (open.x + fold.x + fold.w / 2, open.y + fold.y + fold.h / 2);
        super::move_cursor(&state, at.0, at.1);
        super::press(&state);
        super::release(&state);
        assert!(state.palette_at.get().folded, "the press folded it");

        let strip = palette_rect();
        assert_eq!(strip.w, PANEL_STRIP_W, "a fold leaves the strip");

        // ★ Anywhere in the strip: eighteen pixels is not room for a control,
        // so the strip IS the control. A reader who has to find a target inside
        // it has been given a fold they cannot undo.
        for (dx, dy) in [(1, 1), (strip.w / 2, strip.h / 2), (strip.w - 1, 40)] {
            assert_eq!(
                super::Hit::at(&state, strip.x + dx, strip.y + dy),
                super::Hit::Panel(super::SidePanel::Palette, super::PanelAct::Unfold),
                "every part of the strip brings the panel back"
            );
        }
        super::move_cursor(&state, strip.x + strip.w / 2, strip.y + strip.h / 2);
        super::press(&state);
        super::release(&state);
        assert!(!state.palette_at.get().folded, "and the press unfolded it");
        assert_eq!(
            palette_rect().w,
            open.w,
            "back to the width the reader had, not to a default — that is the \
             difference between folding and hiding"
        );
    });
}

/// ★★★★★ R1887 — **a placement the panel does not admit is refused, and the
/// refusal says what was asked AND what is allowed.**
///
/// This is the claim `pinion_core::edge_panel` was written to make, and until
/// this round nothing reached it through a channel a caller uses: the module's
/// own tests call the policy directly. Measured on the floor at R1801 —
/// restricting a panel to one edge and then asking for another puts it on the
/// other, with nothing thrown, nothing returned and no signal.
///
/// Driven through the WIRE because that is where a caller can ask for an edge
/// the header's control never offers: the control cycles the admitted set by
/// construction, so a screen-only test could not reach a refusal at all. ⇒ the
/// two channels are not redundant, they reach different parts of the rule.
#[test]
fn r1887_a_placement_the_panel_does_not_admit_is_refused_with_both_halves() {
    use pinion_core::external::{ExternalIntrospect, IntrospectValue};

    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = use_lab_state();
        let mut oracle = super::LabOracle::new();
        oracle.attach(std::rc::Rc::clone(&state));

        let before = state.palette_at.get();
        let refused = oracle
            .invoke("place", IntrospectValue::Text("palette,top".to_owned()))
            .expect_err("the palette declares left and right, so the top is refused");
        let said = format!("{refused:?}");
        for half in ["top", "left", "right", "edge-not-allowed"] {
            assert!(
                said.contains(half),
                "★ the refusal must carry {half:?} — a caller told only `no` \
                 cannot act on it: {said}"
            );
        }
        assert_eq!(
            state.palette_at.get(),
            before,
            "and nothing moved: a refusal that half-applies is worse than none"
        );

        // The other half of the same rule: an edge it DOES admit goes through,
        // and the answer names where the panel ended up.
        let done = oracle
            .invoke("place", IntrospectValue::Text("palette,right".to_owned()))
            .expect("the palette admits the right edge");
        // ★ R1889 — the WIDTH is in the answer now, and this assertion is what
        // made the change deliberate rather than incidental: R1887 wrote the
        // contract as `<panel> <edge>` and the round that gave the panel a
        // third changeable property had to come here and widen it. A verb whose
        // answer omits what it changed makes every caller re-query to find out
        // whether it did anything.
        assert_eq!(
            done,
            IntrospectValue::Text(format!("palette right {}", state.palette_at.get().extent)),
            "the verb answers where the panel is now, and how wide"
        );
        assert_eq!(
            state.palette_at.get().edge,
            pinion_core::style::ChromeEdge::Right
        );
    });
}

// ── R1834: a card shows the same rows at every zoom ─────────────────────────

/// ★★★★★ **A card's rows do not depend on the zoom** — because the behaviour
/// reference's do not.
///
/// This screen used to collapse every digest row below 67% zoom
/// (`scaled(FONT_TINY) >= 6`). Measured against the reference by extracting its
/// application logic: it applies ONE transform to the whole graph and contains
/// **zero** conditionals on zoom in 195 KB — no level of detail, at any zoom in
/// its 25%..=250% range. Our floor is the same 25%, so the collapse was hiding
/// rows the reference draws across 25%..=66%.
///
/// ⚠ The assertion is over the ZOOM RANGE and not at two chosen points, because
/// a threshold is exactly the shape a two-point test walks past: the old rule
/// tripped at 67%, which neither the minimum nor 100% would have caught on its
/// own.
///
/// The specification carries the reference's fact
/// (`REFERENCE_COLLAPSES_CARD_DETAIL_AT_LOW_ZOOM`) so this reads it rather than
/// restating it — R1651's shape, where a reference measurement lives in the
/// specification and the gate cross-checks the screen against it.
#[test]
fn r1834_a_cards_rows_do_not_depend_on_the_zoom() {
    // ★ The premise, asserted at COMPILE TIME. It was a runtime `assert!` and
    // clippy refused it: the expression is constant, so the check could never
    // fail while running and the "guard" was decoration. That is this tree's
    // own recorded rule — an invariant true by construction belongs to the
    // compiler — and it is stronger here: recording that the reference DOES
    // collapse now fails to BUILD this gate, which is the moment to notice
    // that its shape and the specification have parted.
    const _: () = assert!(!spec::REFERENCE_COLLAPSES_CARD_DETAIL_AT_LOW_ZOOM);

    let owner = Owner::new();
    owner.run(|| {
        let state = state();
        let node = state.cards()[0];
        let at_full = card_shape_at(&state, node, 100)
            .expect("a card of the opening graph has a shape at 100%")
            .rows
            .len();
        assert!(
            at_full > 0,
            "the opening graph's first card has digest rows, or this gate \
             asserts that zero equals zero at every zoom",
        );

        for zoom in (ZOOM_MIN..=ZOOM_MAX).step_by(5) {
            let rows = card_shape_at(&state, node, zoom)
                .expect("a card has a shape at every zoom the screen allows")
                .rows
                .len();
            assert_eq!(
                rows, at_full,
                "★ at {zoom}% the card shows {rows} row(s) where it shows \
                 {at_full} at 100% — the reference collapses at no zoom, so \
                 neither may this screen",
            );
        }
    });
}

/// ★★★ **A face shrinks with the diagram it sits in, all the way down.**
///
/// The cause the collapse was a symptom of. A floor at 6px stopped the face
/// shrinking while the diagram kept shrinking, so a card grew relative to its
/// neighbours until they overlapped — which is the defect R1656 measured and
/// repaired with a level of detail. The floor is 1 now, and this asserts the
/// relationship rather than the constant: halve the zoom and the face must not
/// stay where it was.
#[test]
fn r1834_a_canvas_face_keeps_shrinking_below_the_old_floor() {
    let big = super::canvas_font_by(spec::FONT_TINY_PX, 100);
    let small = super::canvas_font_by(spec::FONT_TINY_PX, ZOOM_MIN);
    assert!(
        small < big,
        "a face at {ZOOM_MIN}% ({small}) is not smaller than at 100% ({big})",
    );
    assert!(
        small >= 1,
        "a face of zero is not a small label, it is an invisible one",
    );
    // ★ The discrimination: under the OLD floor of 6 this would have been 6 at
    // every zoom at or below 66%, so the assertion above would have passed on
    // the broken build too. This one would not.
    assert!(
        small < 6,
        "★ the face is still floored at the old 6px, which is what stopped a \
         card shrinking with the diagram around it",
    );
}

// ── R1844 — checkpoints and assertions with a timeout ────────────────────────
//
// The census row `lab.t1.9`. R1789 gave this screen a clock and four acts that
// all COMMAND the graph; none of them asks whether it did what it was told, so
// a plan that starts a node at two seconds and kills it at eight could not say
// *and it should have been up in between*. These are that fifth word's gates.
//
// ⚠ And they are the scenario module's FIRST gates. Measured before writing
// them: no test in this crate named `scenario` at all — R1789's module was
// covered only by a demo, and a demo is not run by `cargo test`.

/// A scenario fixture: a state, and the name of a card that starts up.
fn scenario_state() -> (std::rc::Rc<LabState>, String) {
    let state = std::rc::Rc::new(state());
    let card = state
        .cards()
        .first()
        .map(|node| state.name_of(*node))
        .expect("the opening graph has a card");
    (state, card)
}

/// ★★★★★ R1844 — **the act that waits requires a deadline, and the acts that
/// do not refuse one.**
///
/// Both directions, because they fail differently. A `check` with no timeout is
/// a sample dressed as an assertion — true or false about one instant of
/// whatever step the caller advanced by. A `kill` carrying one is worse: a
/// number a reader sees in the plan that nothing will ever consult, which is a
/// lie the surface tells once and then keeps telling.
#[test]
fn r1844_only_the_act_that_waits_takes_a_timeout() {
    let owner = Owner::new();
    owner.run(|| {
        let (state, card) = scenario_state();
        assert!(
            scenario::schedule(&state, "main", 1.0, "check", &card, None).is_err(),
            "a check with no deadline asserts something about one instant only",
        );
        for act in ["start", "stop", "kill"] {
            assert!(
                scenario::schedule(&state, "main", 2.0, act, &card, Some(3.0)).is_err(),
                "{act} happens at its moment, so a timeout on it would never be read",
            );
        }
        assert!(scenario::schedule(&state, "main", 1.0, "check", &card, Some(3.0)).is_ok());
    });
}

/// ★★★★★ R1844 — **a checkpoint waits, and the waiting is what it is for.**
///
/// Three verdicts over one plan, because two would not distinguish the
/// mechanism from a sample: the card is DOWN when the checkpoint is crossed, so
/// an assertion without a deadline would already have failed. It waits, the
/// card comes up inside the window, and the verdict is `met`.
#[test]
fn r1844_a_checkpoint_waits_for_its_card_and_is_met() {
    let owner = Owner::new();
    owner.run(|| {
        let (state, card) = scenario_state();
        scenario::schedule(&state, "main", 0.0, "stop", &card, None).expect("stop places");
        scenario::schedule(&state, "main", 1.0, "check", &card, Some(4.0)).expect("check places");
        scenario::schedule(&state, "main", 3.0, "start", &card, None).expect("start places");

        // Crossed at 1s with the card down: waiting, not failed.
        scenario::advance(&state, 2.0).expect("the clock moves");
        assert_eq!(verdicts(&state), vec!["waiting".to_owned()]);

        // The card comes up at 3s, inside the window that ends at 5s.
        scenario::advance(&state, 2.0).expect("the clock moves");
        assert_eq!(verdicts(&state), vec!["met".to_owned()]);
    });
}

/// ★★★★★ R1844 — **and it fails when the deadline passes with the card still
/// down**, which is the half that makes the verdict worth reading.
#[test]
fn r1844_a_checkpoint_fails_when_its_deadline_passes() {
    let owner = Owner::new();
    owner.run(|| {
        let (state, card) = scenario_state();
        scenario::schedule(&state, "main", 0.0, "stop", &card, None).expect("stop places");
        scenario::schedule(&state, "main", 1.0, "check", &card, Some(2.0)).expect("check places");

        scenario::advance(&state, 2.0).expect("the clock moves");
        assert_eq!(
            verdicts(&state),
            vec!["waiting".to_owned()],
            "3s is the deadline"
        );

        scenario::advance(&state, 2.0).expect("the clock moves");
        assert_eq!(verdicts(&state), vec!["failed".to_owned()]);
    });
}

/// ★ R1844 — a check asserts and does not COMMAND, so it cannot contradict.
///
/// `conflicts` reports a moment where one card is told two opposite things.
/// Asking is not telling: a check beside a kill at one second is a plan that
/// asserts and then acts, which is exactly what a scenario is for.
#[test]
fn r1844_a_check_beside_a_command_is_not_a_conflict() {
    let owner = Owner::new();
    owner.run(|| {
        let (state, card) = scenario_state();
        scenario::schedule(&state, "main", 1.0, "check", &card, Some(1.0)).expect("check places");
        scenario::schedule(&state, "other", 1.0, "kill", &card, None).expect("kill places");
        assert!(
            scenario::conflicts(&state.scenario.borrow()).is_empty(),
            "a checkpoint commands nothing, so it contradicts nothing",
        );
    });
}

// ── R1848 — the traffic taxonomy ────────────────────────────────────────────

/// ★★★★★ R1848 — **only a traffic role carries traffic, and that emptiness is
/// the taxonomy's content.**
///
/// The census's `lab.t1.8` asks for traffic nodes carrying five named
/// parameters. Its verdict is `app`, which means the framework owns what a node
/// IS and this screen owns which parameters its domain's traffic has — so the
/// assignment below is a domain decision, and this is where the decision is
/// held to being one. An infrastructure role that declared a parameter would be
/// making a claim about somebody else's messages.
#[test]
fn r1848_only_traffic_roles_carry_traffic() {
    for role in spec::ROLES {
        let carries = !role.carries.is_empty();
        assert_eq!(
            carries,
            role.group == "traffic",
            "{} is in the {} group and {} traffic parameters",
            role.name,
            role.group,
            if carries { "carries" } else { "carries no" }
        );
    }
    // And the vocabulary is CLOSED: a role cannot name a parameter that is not
    // one, which is what makes this a taxonomy rather than a list of strings.
    for role in spec::ROLES {
        for parameter in role.carries {
            assert!(
                spec::TRAFFIC_PARAMETERS.contains(parameter),
                "{} carries {:?}, which is not in the vocabulary",
                role.name,
                parameter
            );
        }
    }
}

/// ★★★★★ R1848 — **the taxonomy DIVIDES the roles, rather than saying the same
/// thing about all of them.**
///
/// The check the round above would pass while being useless: four roles each
/// carrying all five parameters satisfies every assertion there and answers no
/// question a reader would ask. So the fixture is held to distinguishing —
/// somebody carries the whole vocabulary, somebody carries less, and no two
/// roles are told apart by nothing.
#[test]
fn r1848_the_taxonomy_tells_the_traffic_roles_apart() {
    let traffic: Vec<_> = spec::ROLES
        .iter()
        .filter(|r| r.group == "traffic")
        .collect();
    assert!(traffic.len() > 1, "a taxonomy over one role is a label");
    let widest = traffic.iter().map(|r| r.carries.len()).max().unwrap_or(0);
    assert_eq!(
        widest,
        spec::TRAFFIC_PARAMETERS.len(),
        "something originates messages, and it decides every parameter"
    );
    // ★★★★★ R1848 — PAIRWISE, and this is a repair the counterfactuals forced.
    // The first draft compared only the widest declaration with the narrowest,
    // which is satisfied by any spread at all: widening ONE role until it was
    // indistinguishable from another left this test green, and the
    // counterfactual that did exactly that came back PASSED. A taxonomy tells
    // its members apart when no two of them are told the same thing — that is
    // what the name of this test claims, and now what it checks.
    for (n, role) in traffic.iter().enumerate() {
        for other in &traffic[n + 1..] {
            assert_ne!(
                role.carries, other.carries,
                "{} and {} are told exactly the same thing, so the declaration \
                 does not distinguish them",
                role.name, other.name
            );
        }
    }
}

/// ★★★★★ R1848 — **what a card STATES is derived from its role's declaration
/// and its own rows, and the opening graph states less than it declares.**
///
/// This is the measurement the screen could not make about itself. A card's
/// rows are free text keyed by whatever the row was written with; the role said
/// nothing about which keys belong to it; so "does this node state its
/// priority?" had nowhere to be asked. It does now, and the answer for the
/// reference's own opening graph is that most of the vocabulary is unstated.
///
/// ⚠ NOT asserted as a defect. The opening graph is the reference's screen and
/// this tree reproduces it — the number is recorded so that a later round
/// changing it has to say so, which is the only thing a fixture measurement is
/// good for.
#[test]
fn r1848_a_card_states_a_subset_of_what_its_role_carries() {
    let mut stated_total = 0;
    let mut declared_total = 0;
    for node in spec::NODES {
        let stated = spec::stated_traffic(node);
        let unstated = spec::unstated_traffic(node);
        let role = spec::role_of(node).unwrap_or_else(|| panic!("{} has no role", node.id));
        assert_eq!(
            stated.len() + unstated.len(),
            role.carries.len(),
            "{}: stated and unstated must partition what {} carries",
            node.id,
            role.name
        );
        for parameter in &stated {
            assert!(
                node.rows.iter().any(|(key, _)| *key == parameter.key()),
                "{} is reported stating {} and has no such row",
                node.id,
                parameter.key()
            );
        }
        stated_total += stated.len();
        declared_total += role.carries.len();
    }
    assert!(
        declared_total > 0,
        "no node has a role that carries anything, so this measures nothing"
    );
    assert!(
        stated_total < declared_total,
        "the opening graph states every declared parameter, which is not what \
         the reference's screen shows — if that changed, this fixture is stale"
    );
    assert!(
        stated_total > 0,
        "no card states any declared parameter, so the keys and the vocabulary \
         do not meet and the join is decorative"
    );
}

/// Every checkpoint's verdict, in the order they were raised.
fn verdicts(state: &std::rc::Rc<LabState>) -> Vec<String> {
    state
        .checks
        .borrow()
        .iter()
        .map(|check| check.verdict().to_owned())
        .collect()
}

/// ★★★★★ R1866 — **a rewind is a restart**, asserted where `cargo test` can
/// see it.
///
/// The rule `advance` keeps is a PROPERTY — *a playhead at the start means this
/// run has not happened* — and it was first written as a CONDITION, *at the
/// beginning and going forward*. Those are different sentences: the condition
/// does not cover a reader who scrubs BACK to zero, who was therefore left
/// holding the previous run's checkpoint verdicts and the previous run's tape.
/// `record` would then keep a baseline for a run that, as far as the playhead
/// is concerned, never happened.
///
/// 🟥 This is asserted HERE as well as in the round's demo because **the demo
/// is not the population of `cargo test`**: the counterfactual that put the
/// condition back walked through three crates of green, so the only thing
/// standing between that rule and a silent regression was a script nothing in
/// the build runs. The demo proves it of the RUNNING binary; this proves it of
/// the rule.
///
/// Both halves are asserted because they are two stores cleared by one
/// sentence, and a repair that remembered one of them would pass a test that
/// only asked about the other.
#[test]
fn r1866_a_rewind_is_a_restart_for_both_the_tape_and_the_checks() {
    let owner = Owner::new();
    owner.run(|| {
        let (state, card) = scenario_state();
        scenario::schedule(&state, "main", 0.0, "stop", &card, None).expect("stop places");
        scenario::schedule(&state, "main", 1.0, "check", &card, Some(2.0)).expect("check places");

        // The run has to have happened before its restart means anything.
        scenario::advance(&state, 2.0).expect("the clock moves");
        assert!(
            !verdicts(&state).is_empty(),
            "the fixture crossed no checkpoint, so the assertion below would \
             hold for a reason that has nothing to do with the rewind",
        );
        scenario::record(&state).expect("a run with marks on it can be kept");

        // Back to the start, and NOTHING forward: the rewind alone is the act
        // whose consequence this is about.
        scenario::advance(&state, -1000.0).expect("the clock rewinds");
        assert!(
            verdicts(&state).is_empty(),
            "a rewound playhead left the previous run's verdicts standing: {:?}",
            verdicts(&state),
        );
        assert!(
            scenario::record(&state).is_err(),
            "a rewound playhead left the previous run's marks on the tape, so a \
             baseline could be kept for a run that never happened",
        );
    });
}

/// ★★★★★ R1885.3 — **the opening graph is heterogeneous AND every drawn wire
/// still negotiates**, the pair of facts that makes it a compatibility TEST
/// rather than a picture of one deployment.
///
/// # Why this exists, and it is not a flattering reason
///
/// `opening_implementation`'s doc named this test — by this exact name — and the
/// test did not exist. R1885's own closing audit grepped for it and found zero
/// definitions: a citation to a proof nobody wrote, in the one place a reader
/// would go to check the claim. This repository gates that class over the
/// atomic store's citations (`validate-code-refs`) and over the census's
/// `proven_by` (`--check-proofs`); over Rust doc prose it gates nothing, so
/// nothing said a word.
///
/// The same paragraph was wrong a second way. It said "the two non-reference
/// builds" and by then there was ONE: the spans were changed mid-round when the
/// walk found no refusal was reachable, and both non-reference cards ended up on
/// the same build. **The prose did not follow the fix it was describing.**
///
/// # What it asserts, and what it deliberately does not
///
/// Heterogeneity is a COUNT OF DISTINCT BUILDS, not a check that `P-03` and
/// `S-01` are the odd ones out: naming them would restate the table this is
/// meant to check, and would keep passing if that table were flattened to a
/// single build. Every link is asked of the document, so the rule under test is
/// the one the screen runs rather than a second copy written here.
///
/// # 🟥 The mutation that PASSED, and what it forced
///
/// This test was written with two assertions and one of them was hollow. Moving
/// both non-reference cards onto the legacy build — a graph that genuinely
/// cannot open — left it GREEN, because `seed_links` draws with
/// `Document::connect`, which since R1885 refuses an inadmissible pair. The two
/// bad wires were never made, so "every drawn wire negotiates" had nothing to
/// object to. ⇒ ★★★★★ **a population can shrink instead of an assertion
/// failing**, and the shrink is invisible to any check phrased over what is
/// present. The count against `spec::LINKS` is what closed it, and the same
/// mutation now reports `5 of the 7`.
#[test]
fn r1885_the_opening_graph_is_heterogeneous_and_still_negotiates() {
    let owner = Owner::new();
    owner.run(|| {
        let state = state();
        let doc = state.doc.borrow();
        let tree = doc.tree(ROOT).expect("the root tree");

        let builds: BTreeSet<&'static str> = tree
            .nodes()
            .filter_map(|n| match &n.body {
                NodeBody::Kind(kind) => Some(kind.implementation.stack.word()),
                _ => None,
            })
            .collect();
        assert!(
            builds.len() >= 2,
            "★ the opening graph runs a single build, so it cannot answer the \
             question the axis exists for: {builds:?}"
        );

        // 🟥🟥🟥 ★★★★★ R1885.3 — **the count, and it is the half that has teeth.**
        // `seed_links` draws with `Document::connect`, which since R1885 REFUSES
        // an inadmissible pair — so a build that cannot negotiate does not
        // produce a refused wire, it produces NO WIRE. Measured: with both
        // non-reference cards moved onto the legacy build, the assertion below
        // about drawn wires passed, because the two wires that would have been
        // refused were simply never made. ⇒ **a population can shrink instead of
        // an assertion failing**, and "every drawn wire is fine" cannot see a
        // wire that was never drawn. So the specification's own count is what
        // this compares against.
        let links = tree.links();
        assert_eq!(
            links.len(),
            spec::LINKS.len(),
            "★ the opening graph drew {} of the {} wires its specification \
             declares — a wire `connect` refused is a wire that is not here, \
             which is exactly what this count exists to catch",
            links.len(),
            spec::LINKS.len(),
        );
        let refused: Vec<String> = links
            .iter()
            .filter_map(|link| match doc.admission(ROOT, link.from, link.to) {
                Some(Admission::Refused(why)) => Some(why.because),
                _ => None,
            })
            .collect();
        assert_eq!(
            refused,
            Vec::<String>::new(),
            "★ the opening graph must be a compatibility test that PASSES — a \
             screen that opens blocked asserts a defect instead of offering a \
             test: {refused:?}"
        );
    });
}

/// ★★★★★ R1914 — **the published address vocabulary IS the taxonomy's member
/// list**, compared against the derivation's output rather than re-spelled.
///
/// `PIN_PARTS` and `PIN_ADDRESSES` are written out because
/// `NodeKind::composition` allocates and a `const` cannot project from it. That
/// makes them a second statement of a fact the taxonomy already owns, and a
/// second statement is a thing that drifts — so this compares them with what
/// the taxonomy actually answers. Adding a third member to a locator without
/// touching the declaration fails here.
#[test]
fn r1914_the_published_pin_addresses_are_the_taxonomys_members() {
    use crate::graph::{Endpoint, LabNode, Transport};
    use pinion_node_graph::{Composition, NodeKind};

    let Composition::Members(members) = LabNode::composition(&Endpoint::Locator(Transport::Tcp))
    else {
        panic!("a locator is composite -- that is what makes this screen splittable");
    };
    let named: Vec<String> = members.iter().map(|port| port.name.clone()).collect();
    assert_eq!(
        named,
        super::PIN_PARTS,
        "★ the words `split_pin` accepts must be the members the taxonomy \
         declares, in the same ORDER -- the address `accept.host` resolves by \
         position, so a reordering here would silently address the other half",
    );

    // ★ And the published address list is exactly the two pins plus every
    // member of each, which is what an agent enumerates.
    let mut want: Vec<String> = vec!["dial".to_owned(), "accept".to_owned()];
    for pin in ["dial", "accept"] {
        for part in &named {
            want.push(format!("{pin}.{part}"));
        }
    }
    let mut published: Vec<String> = super::PIN_ADDRESSES
        .iter()
        .map(|a| (*a).to_owned())
        .collect();
    published.sort();
    want.sort();
    assert_eq!(published, want);
}

/// ★★★★★ R1914 — **a pin on this screen comes apart and goes back**, through
/// the verb an agent actually calls.
///
/// The engine's four commands, met on the assembled tool's own card. What this
/// asserts beyond "it did something" is the three facts neither reference
/// publishes: the parent is hidden with a REASON, the member ports carry the
/// halves of the address the pin was accepting, and folding puts the address
/// back together from them.
#[test]
fn r1914_a_pin_on_a_card_comes_apart_into_host_and_service() {
    use pinion_node_graph::{Hidden, PortPath, Side};

    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        // ★★★★★ BOTH halves are required, and asking for only one is how an
        // earlier draft of this test came to pass while proving nothing: a pin
        // that splits but carries no address cannot show a value being shared
        // out, and every pin on the opening graph that carried one was WIRED —
        // which the reference refuses to split. The dial pin is the
        // intersection, and finding it by asking rather than by naming a card
        // is what keeps this true when the opening graph changes.
        let found = state
            .cards()
            .into_iter()
            .find_map(|node| {
                let doc = state.doc.borrow();
                doc.splittable(ROOT, node, Side::Output, 0).ok()?;
                let carries = doc
                    .port_value(ROOT, node, pinion_node_graph::PortRef::output(0))
                    .cloned()?;
                Some((node, carries))
            })
            .expect("★ some card has an unwired dial pin carrying an address");
        let (card, accepting) = found;
        let (_scheme, rest) = accepting.split_once('/').expect("a locator has a scheme");
        let (want_host, want_service) = rest.rsplit_once(':').expect("and an address");

        let said = super::split_pin(&state, card, "dial").expect("it splits");
        assert!(
            said.contains("apart into 2 pin(s)"),
            "★ a locator has two members, and the verb says how many: {said}",
        );

        let doc = state.doc.borrow();
        let seen = doc.visible_ports(ROOT, card).expect("the card is there");
        assert_eq!(
            seen.why_hidden(Side::Output, 0),
            Some(Hidden::Split),
            "★ the parent is HIDDEN and the reason is sayable -- the reference \
             sets the same flag and has no field to report it from",
        );
        assert_eq!(seen.split_outputs, [0]);

        let members: Vec<(String, Option<String>)> = doc
            .resolved_ports(ROOT, card, Side::Output)
            .into_iter()
            .filter(|(path, _)| path.depth() > 0)
            .map(|(path, port)| {
                let carried = doc
                    .index_of(ROOT, card, Side::Output, &path)
                    .and_then(|index| {
                        doc.port_value(ROOT, card, pinion_node_graph::PortRef::output(index))
                            .cloned()
                    })
                    .or_else(|| port.flow.default_value().cloned());
                (port.name, carried)
            })
            .collect();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].0, "host");
        assert_eq!(members[1].0, "service");

        // ★★★★★ The pieces are the ADDRESS this pin was carrying, taken
        // apart -- not two declared defaults a reader has to fill in again.
        assert_eq!(
            (members[0].1.as_deref(), members[1].1.as_deref()),
            (Some(want_host), Some(want_service)),
            "★ from {accepting:?}",
        );
        drop(doc);

        // ★ And back again, at the OTHER end: asked at a member, the fold
        // reaches the pin that member belongs to.
        let said = super::split_pin(&state, card, "-dial.service").expect("it folds");
        assert!(
            said.contains("from 1 split(s)"),
            "★ one split went away, and the verb says how many: {said}",
        );
        let doc = state.doc.borrow();
        assert!(
            doc.split_paths(ROOT, card, Side::Output).is_empty(),
            "nothing is split any more",
        );
        assert_eq!(
            doc.port_value(ROOT, card, pinion_node_graph::PortRef::output(0)),
            Some(&accepting),
            "★★★★★ the address came back together -- the half the reference \
             has for four named types and nothing else",
        );
        assert_eq!(
            doc.path_of(ROOT, card, Side::Output, 0),
            Some(PortPath::root(0))
        );
    });
}

/// ★★★★★ R1915 — **a member pin a split put on the frame answers a press**,
/// and it answers with the address that names it.
///
/// R1914 drew these pins and announced them and left them untouchable: the hit
/// carried a `bool`, so there was structurally nowhere to put which member, and
/// the tag parser read the last dotted segment as the side — so `…dial.host`
/// matched nothing and answered `Nothing`. Drawn, named, and unreachable.
///
/// This drives the SCREEN's own painter and then presses the rectangles it
/// drew, which is the only way to hold the two together: a press is resolved
/// from the paint, so a test that computed the rectangle itself would be
/// checking arithmetic rather than the frame.
#[test]
fn r1915_a_member_pin_answers_a_press_with_the_address_that_names_it() {
    use pinion_node_graph::{PortPath, Side};

    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();
        let card = state
            .cards()
            .into_iter()
            .find(|node| {
                state
                    .doc
                    .borrow()
                    .splittable(ROOT, *node, Side::Output, 0)
                    .is_ok()
            })
            .expect("★ some card has an unwired dial pin");
        super::split_pin(&state, card, "dial").expect("it splits");
        crate::painted::render_so_a_press_can_be_asked(&state);

        let name = state.name_of(card);
        let window = |x: u32, y: u32| {
            super::content_to_window(&state, i64::from(x), i64::from(y)).expect("on screen")
        };
        let box_of = card_rect(&state, card).expect("a card");

        for (ordinal, part) in super::PIN_PARTS.iter().enumerate() {
            let seat = super::member_pin_rect(&state, box_of, true, ordinal);
            let at = window(seat.x + seat.w / 2, seat.y + seat.h / 2);
            let want = PortPath::root(0).then(u32::try_from(ordinal).unwrap_or(0));
            assert_eq!(
                Hit::at(&state, at.0, at.1),
                Hit::Pin {
                    node: card,
                    side: Side::Output,
                    at: want.clone(),
                },
                "★ {name}'s {part} half is pressable where it is painted",
            );
            // ★★★★★ And the word the hit answers with is the word the VERB
            // takes — one spelling, so a client can press what it read and hand
            // it straight back.
            assert_eq!(super::pin_word(Side::Output, &want), format!("dial.{part}"),);
            assert_eq!(
                super::pin_address(&format!("dial.{part}")).expect("the verb takes it"),
                (Side::Output, want),
            );
        }
    });
}

/// ★★★★★ R1915 — **a wire lands on a member pin, and folding the pin it
/// belongs to cuts that wire and says so.**
///
/// The half R1914 could report and had never been made to do: `remap_ports`
/// severs a link whose port stops existing, `Recombined::severed` names what it
/// severed, and nothing had ever walked that path. The reference cannot: its
/// recombine destroys the sub-pins and their links go with them, and the
/// command answers `void`.
#[test]
fn r1915_a_wire_on_a_member_is_cut_by_the_fold_and_named() {
    use pinion_node_graph::{NodeId, PortPath, Side};

    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();
        // ★ The pair is found by ASKING rather than named: both pins have to be
        // splittable (so unwired), and the graph has to admit the wire — the
        // first pair tried closed a cycle, which is the document's own refusal
        // doing its job and not a fault to work around.
        let host = PortPath::root(0).then(0);
        let splits =
            |node: NodeId, side: Side| state.doc.borrow().splittable(ROOT, node, side, 0).is_ok();
        let cards = state.cards();
        // ⚠ MEASURED, and it changed how this test is built: no pair on the
        // OPENING GRAPH can take this wire. Its three unwired dial pins all
        // belong to cards its one unwired accept pin already reaches, so every
        // attempt is refused `that link would close a cycle` — the document's
        // own rule doing its job. So the test adds a card, which is a gesture
        // the screen has, rather than weakening the assertion until the opening
        // graph happens to satisfy it.
        let dialler = cards
            .iter()
            .copied()
            .find(|n| splits(*n, Side::Output))
            .expect("★ some card has an unwired dial pin");
        super::add_node(&state, crate::graph::Role::Store);
        let listener = *state
            .cards()
            .iter()
            .rev()
            .find(|n| !cards.contains(n))
            .expect("★ the palette put a card on the canvas");
        assert!(
            splits(listener, Side::Input),
            "★ a card nothing has dialled has an accept pin nothing is wired to",
        );

        super::split_pin(&state, dialler, "dial").expect("the dial splits");
        super::split_pin(&state, listener, "accept").expect("the accept splits");
        let said = super::connect_at(&state, dialler, &host, listener, &host)
            .expect("★ a host half reaches a host half — the taxonomy's own rule");
        assert!(
            said.contains("dial.host") && said.contains("accept.host"),
            "★ the link says which HALVES it joined, not which cards: {said}",
        );

        let landed = {
            let doc = state.doc.borrow();
            let into = doc
                .index_of(ROOT, listener, Side::Input, &host)
                .expect("the member is a port");
            doc.tree(ROOT)
                .expect("the tree")
                .links()
                .iter()
                .filter(|l| l.to.node == listener && l.to.port == into)
                .count()
        };
        assert_eq!(landed, 1, "★ the wire is on the MEMBER port, by index");

        // ★★★★★ A wire on a member is what makes the fold cost something, and
        // the fold says what it cost.
        let folded = state
            .doc
            .borrow_mut()
            .recombine_port(ROOT, listener, Side::Input, &host)
            .expect("the accept pin is split");
        assert_eq!(
            folded.severed.len(),
            1,
            "★★★★★ folding took the port the wire landed on away, and NAMED the \
             wire it had to cut — the reference's command answers `void`",
        );
        assert_eq!(folded.severed[0].to.node, listener);
        assert!(
            state
                .doc
                .borrow()
                .split_paths(ROOT, listener, Side::Input)
                .is_empty(),
            "and nothing is split on that side any more",
        );
    });
}

/// ★★★★★ R1916 — **resting on a pin shows a sentence about it**, on the frame
/// and in the accessibility tree.
///
/// The canon puts a `title` on 25 of its controls and the assembled tool
/// mounted NONE — measured at R1886, and the reason was structural rather than
/// forgetful: the framework's tooltip had been its own anchor since R695, whose
/// own module docs name "attach a tooltip to an arbitrary existing widget" as a
/// future axis. `pinion_core::describe` is that axis, and this is its first
/// consumer.
#[test]
fn r1916_resting_on_a_pin_shows_what_it_is_for() {
    use pinion_a11y::AriaRole;
    use pinion_node_graph::{PortPath, Side};

    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();
        crate::painted::render_so_a_press_can_be_asked(&state);

        // ★ The control: with the cursor nowhere near a pin, nothing is shown.
        // Without it this test would pass on a screen that shows a tooltip
        // always, which is not what a tooltip is.
        assert_eq!(
            super::pin_description_shown(&state),
            None,
            "★ a tooltip nobody is resting on is not shown",
        );
        assert_eq!(
            super::wire_access(&state)
                .iter()
                .filter(|n| n.role == AriaRole::Tooltip)
                .count(),
            0,
        );

        let card = state.cards()[0];
        let name = state.name_of(card);
        let box_of = card_rect(&state, card).expect("a card");
        let seat = pin_rect(&state, box_of, true);
        let at = super::content_to_window(
            &state,
            i64::from(seat.x + seat.w / 2),
            i64::from(seat.y + seat.h / 2),
        )
        .expect("on screen");
        // ★ Through the screen's own `move_cursor`, not by setting the signal:
        // a leave clears `pointer_inside` and only a move sets it again, so a
        // test that wrote the position directly would be testing a state the
        // application cannot reach.
        super::move_cursor(&state, at.0, at.1);

        let (tag, sentence) =
            super::pin_description_shown(&state).expect("★ resting on the dial pin shows one");
        assert_eq!(tag, format!("lab.pin.{name}.dial"));
        // ★★★★★ The sentence is the SUBSTRATE's composition — the type's half
        // and the port's own half — not a string this screen wrote. The
        // reference's base implementation hands the description straight back
        // and adds neither half to it.
        let tip = state
            .doc
            .borrow()
            .port_tooltip(ROOT, card, Side::Output, &PortPath::root(0))
            .expect("the pin is a port");
        assert_eq!(sentence, tip.sentence());
        assert!(sentence.contains("address"), "the TYPE's half: {sentence}");
        assert!(sentence.contains("hands on"), "the PORT's half: {sentence}");

        // ★ And it reaches a reader who does not look at pixels, through
        // `aria-describedby` rather than as a floating node nothing points at.
        let nodes = super::wire_access(&state);
        let tips: Vec<_> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Tooltip)
            .collect();
        assert_eq!(tips.len(), 1, "one description region, for the one shown");
        assert_eq!(tips[0].name.as_deref(), Some(sentence.as_str()));
        let anchor = nodes
            .iter()
            .find(|n| n.tag == format!("lab.pin.{name}.dial"))
            .expect("the pin is announced");
        assert_eq!(
            anchor.described_by.as_deref(),
            Some(tips[0].tag.as_str()),
            "★ the mark POINTS AT its description — a region nothing references \
             is a region an AT never reads out",
        );
    });
}

/// ★★★★★ R1914 — the verb's refusals, in the model's words.
#[test]
fn r1914_the_split_verb_says_why_it_will_not() {
    let owner = Owner::new();
    owner.run(|| {
        let state = std::rc::Rc::new(state());
        let card = state.cards()[0];

        let why = super::split_pin(&state, card, "handle")
            .expect_err("this card draws no pin called `handle`");
        assert!(
            format!("{why:?}").contains("dial"),
            "★ the refusal names what the card DOES draw: {why:?}",
        );

        let why = super::split_pin(&state, card, "accept.scheme")
            .expect_err("a locator has no member called `scheme`");
        assert!(
            format!("{why:?}").contains("host"),
            "★ and it names the members a locator IS made of: {why:?}",
        );

        let why = super::split_pin(&state, card, "-dial")
            .expect_err("nothing is split, so there is nothing to fold");
        assert!(
            format!("{why:?}").contains("split"),
            "★ nothing to fold is not the same as not allowed to fold: {why:?}",
        );
    });
}

/// ★★★★★ R1926 — **every socket type has a colour, and no two share one.**
///
/// The property `LabNode::type_colour`'s comment claims, performed rather than
/// asserted in prose — R1855's lesson, and R1925 met the same class one round
/// ago when a module promised an invariant nothing ran.
///
/// It is the property the screen's whole point rests on: if a host and a
/// service were drawn in one colour, splitting a locator would produce two pins
/// a reader cannot tell apart, which is exactly the state this round found the
/// canvas in — every member pin took the NODE's transport colour.
///
/// The population is `Endpoint::all()`, which is derived from `Transport::ALL`,
/// so a transport added later is checked here without anyone extending a list.
#[test]
fn r1926_the_socket_palette_is_injective() {
    use pinion_node_graph::{NodeKind, Tint};

    let every = crate::graph::Endpoint::all();
    assert!(
        every.len() > Transport::ALL.len(),
        "the roster is the transports AND the halves: {every:?}"
    );
    let inked: Vec<(crate::graph::Endpoint, Tint)> = every
        .iter()
        .map(|ty| {
            (
                *ty,
                <crate::graph::LabNode as NodeKind>::type_colour(ty).unwrap_or_else(|| {
                    panic!("{ty:?} has no colour, so a pin carrying it has none")
                }),
            )
        })
        .collect();
    for (a, ink_a) in &inked {
        for (b, ink_b) in &inked {
            assert!(
                a == b || ink_a != ink_b,
                "★ {a:?} and {b:?} are drawn in the same colour {ink_a:?}, so a \
                 reader cannot tell them apart"
            );
        }
    }
}

/// ★ R1926 — the border colour the scene gave the box with this tag.
///
/// A local walk rather than a reuse of `painted.rs`'s harness, because that one
/// keeps RECTANGLES and this question is about an edge — the exact asymmetry
/// R1919 measured. Six lines is cheaper than widening a harness the rest of the
/// module does not need widened.
/// ★ What the frame says about one tag's border — **three** answers, because
/// collapsing the first two would make a missing pin and a colourless one the
/// same failure, which is the ambiguity R1922 recorded the cost of.
#[derive(Debug, PartialEq)]
enum PaintedEdge {
    /// No box with that tag is on the frame at all.
    Absent,
    /// Drawn, and given no border.
    Bare,
    /// Drawn, in this colour.
    Ink(pinion_core::Color),
}

impl PaintedEdge {
    fn of(border: Option<&pinion_core::Border>) -> Self {
        border.map_or(Self::Bare, |edge| Self::Ink(edge.color))
    }

    const fn is_drawn(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

fn painted_border(scene: &pinion_core::Scene, tag: &str) -> PaintedEdge {
    use pinion_core::Scene;
    // ★ A `Container`, not a `Box`: this screen's `box_at` builds a childless
    // CONTAINER carrying the style, which is exactly the sort of thing a walk
    // written from the type name rather than from the producer gets wrong — and
    // the first draft of this one did, silently, by answering "not on the frame"
    // for every pin.
    match scene {
        Scene::Container(node) if node.tag.as_deref() == Some(tag) => {
            PaintedEdge::of(node.style.border.as_ref())
        }
        Scene::Box(node) if node.tag.as_deref() == Some(tag) => {
            PaintedEdge::of(node.style.border.as_ref())
        }
        Scene::Container(node) => node
            .children
            .iter()
            .map(|child| painted_border(child, tag))
            .find(PaintedEdge::is_drawn)
            .unwrap_or(PaintedEdge::Absent),
        Scene::Scroll(node) => painted_border(&node.content, tag),
        _ => PaintedEdge::Absent,
    }
}

/// ★★★★★ R1926 — and the canvas paints what the model says, member by member.
///
/// The seam this round closed: the colour used to be a table in the view, read
/// off the node's transport. Asserting the two agree is what stops it drifting
/// back — and the second half, that the two halves of a split differ, is what
/// makes the agreement worth having (a screen and a model that both answered
/// one colour would satisfy the first half alone).
#[test]
fn r1926_a_split_draws_its_halves_in_their_own_colours() {
    use pinion_node_graph::{NodeKind, Side};

    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();
        // The dial pin, because every card has one and it carries a locator —
        // the same composite the accept pin carries, without depending on which
        // roles listen.
        //
        // ⚠ And the card must be one this canvas actually DRAWS a pin for: the
        // opening state has a collapsed card, a collapsed card paints no pins,
        // and picking it made the first draft of this test fail for a reason
        // that had nothing to do with colour. So the population is *cards whose
        // dial pin is on the frame*, derived from the frame rather than assumed.
        let opening = crate::painted::painted_scene(&state);
        let card = state
            .cards()
            .into_iter()
            .find(|node| {
                painted_border(&opening, &format!("lab.pin.{}.dial", state.name_of(*node)))
                    .is_drawn()
                    && state
                        .doc
                        .borrow()
                        .splittable(ROOT, *node, Side::Output, 0)
                        .is_ok()
            })
            .expect("★ some card draws an unwired dial pin");
        super::split_pin(&state, card, "dial").expect("the dial pin is a locator");

        let members: Vec<_> = state
            .doc
            .borrow()
            .resolved_ports(ROOT, card, Side::Output)
            .into_iter()
            .filter(|(path, _)| path.depth() > 0)
            .collect();
        assert_eq!(members.len(), 2, "a locator is made of two");

        let inks: Vec<_> = members
            .iter()
            .map(|(_, port)| {
                pinion_node_graph::palette_of::<crate::graph::LabNode>(&port.flow).own()
            })
            .collect();
        assert!(
            inks.iter().all(Option::is_some),
            "each half carries a colour: {inks:?}"
        );
        assert_ne!(
            inks[0], inks[1],
            "★ and the two halves are NOT one colour — the defect this round found"
        );
        let whole = <crate::graph::LabNode as NodeKind>::type_colour(
            &crate::graph::Endpoint::Locator(crate::graph::Transport::Tcp),
        );
        assert!(
            inks.iter().all(|held| *held != whole),
            "★★ nor is either of them the colour the WHOLE was drawn in, which \
             is what the canvas used for both before this round"
        );

        // ★★★★★ And the PAINT agrees, which is the half a model test cannot
        // reach: the line this round changed is in the view, and a model that
        // answered correctly while the canvas kept its own table would satisfy
        // everything above. The border, because that is where a pin's identity
        // lives here (R1919).
        let scene = crate::painted::painted_scene(&state);
        let name = state.name_of(card);
        let mut painted = Vec::new();
        for (path, _) in &members {
            let tag = format!("lab.pin.{name}.{}", super::pin_word(Side::Output, path));
            painted.push(match painted_border(&scene, &tag) {
                PaintedEdge::Ink(colour) => colour,
                PaintedEdge::Bare => panic!("{tag} is drawn with no border"),
                PaintedEdge::Absent => panic!("{tag} is not on the frame at all"),
            });
        }
        assert_eq!(painted.len(), 2);
        for (drawn, model) in painted.iter().zip(&inks) {
            let tint = model.expect("each half carries a colour");
            assert_eq!(
                (drawn.r, drawn.g, drawn.b),
                (tint.r, tint.g, tint.b),
                "★★★★★ the canvas draws the colour the MODEL says, not one of \
                 its own: painted {drawn:?} against {tint:?}"
            );
        }
        assert_ne!(
            (painted[0].r, painted[0].g, painted[0].b),
            (painted[1].r, painted[1].g, painted[1].b),
            "★★★★★ and the two halves are painted in two colours — the defect \
             this round found, asserted where it lived"
        );
    });
}

/// ★★★★★ R1961 close-audit — **choosing a card's transport moves its ADDRESS,
/// so the choice survives the next edit.**
///
/// The second author this round nearly shipped. `set_pin_transport` wrote
/// `LabNode::transport` through the crate's type swap, and R1961 made that
/// field a derivation — so the verb and the derivation both owned one fact, and
/// the derivation wins whenever anything settles. A person would have chosen a
/// transport, edited an unrelated field, and watched the choice vanish.
///
/// ⚠ The assertion is deliberately *after an unrelated edit*, because that is
/// the only shape that can tell the two designs apart: writing the field
/// directly passes every check made immediately afterwards.
#[test]
fn r1961_a_chosen_transport_outlives_an_unrelated_edit() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();
        let subject = state.node_of("R-01").expect("the specification's router");
        let elsewhere = state.node_of("P-02").expect("the specification's P-02");

        super::set_pin_transport(&state, subject, "dial", "udp").expect("a card that listens may");
        assert_eq!(
            spoken_by(&state, subject),
            Some(Transport::Udp),
            "the card speaks what was chosen",
        );
        assert!(
            super::endpoints_of(&state, subject)
                .iter()
                .all(|one| one.starts_with("udp/")),
            "★★★★★ and the ADDRESS moved with it — {:?}",
            super::endpoints_of(&state, subject),
        );

        // An edit that has nothing to do with the choice, on another card.
        state
            .forms
            .borrow_mut()
            .get_mut(&elsewhere)
            .expect("a form")
            .set("listen.endpoints", "tcp/0.0.0.0:7449")
            .expect("held");
        super::sync_node(&state, elsewhere);
        assert_eq!(
            spoken_by(&state, subject),
            Some(Transport::Udp),
            "★★★★★ and the choice is still there — a verb that wrote the derived \
             field instead of the address would have lost it here",
        );

        // ★ And the card that has no address of its own is REFUSED rather than
        // silently doing nothing, which is what the escape hatch used to hide.
        let learner = state
            .cards()
            .into_iter()
            .find(|node| spoken_by(&state, *node).is_none())
            .expect("★ some card speaks nothing");
        let why = super::set_pin_transport(&state, learner, "dial", "udp")
            .expect_err("★ a card with no address of its own cannot be given one");
        assert!(
            format!("{why:?}").contains("listens nowhere"),
            "★ and the refusal says why: {why:?}",
        );
    });
}

/// Every string in a published value, wherever in its shape it sits.
///
/// Written over the whole value rather than over the keys a register is known
/// to use: which key carries a socket type is exactly what the check below must
/// not have to know, since the defect it exists for is a register nobody
/// remembered.
fn strings_in(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                strings_in(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for held in map.values() {
                strings_in(held, out);
            }
        }
        _ => {}
    }
}

/// ★★★★★ R1961 close-audit — **one socket type, one published spelling.**
///
/// `Endpoint::wire_word`'s own doc calls itself *the one spelling a client
/// reads this type under*. Measured while auditing this round: it was false.
/// Three registers spelled a socket type with `Debug` (`choosable`, `drawn`
/// inside `tints`, and `containers`) while three others used `wire_word`
/// (`takes`, `admits`, `ports`), so one type reached a client as both
/// `Locator(Tcp)` and `locator/tcp` — and an agent joining the registers on the
/// token cannot.
///
/// ★ The population is DERIVED twice over: every path the screen publishes
/// (from its own schema, not a list here) crossed with every `Debug` spelling
/// the taxonomy has (from [`Endpoint::all`]). A type added later is watched
/// without anyone editing this, which is the property the fix needs — R1961
/// added `Unspoken`, whose two spellings were `locator` and `Unspoken`, sharing
/// not even a stem.
///
/// ⚠ Coarse in the same way the R1960 ratchet is: it forbids the exact `Debug`
/// text anywhere in a register, so a register legitimately publishing the word
/// `Host` on its own would be reported. That is the failure direction worth
/// having — it names the register and the token, and a reader decides.
#[test]
fn r1961_one_socket_type_has_one_published_spelling() {
    use pinion_core::external::ExternalIntrospect;

    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();
        crate::painted::render_so_a_press_can_be_asked(&state);
        let mut oracle = super::LabOracle::new();
        oracle.attach(state);

        let debug_forms: Vec<String> = crate::graph::Endpoint::all()
            .into_iter()
            .map(|ty| format!("{ty:?}"))
            .collect();
        let words: BTreeSet<String> = crate::graph::Endpoint::all()
            .into_iter()
            .map(crate::graph::Endpoint::wire_word)
            .collect();
        assert_eq!(
            words.len(),
            debug_forms.len(),
            "★ two socket types share a published word, so the spelling does not \
             identify the type: {words:?}",
        );

        let mut asked = 0_usize;
        for field in oracle.schema().fields {
            let Ok(read) = oracle.query(field.path) else {
                continue;
            };
            asked += 1;
            let mut found = Vec::new();
            match &read {
                pinion_core::external::IntrospectValue::Json(value) => {
                    strings_in(value, &mut found);
                }
                pinion_core::external::IntrospectValue::Text(text) => found.push(text.clone()),
                _ => {}
            }
            for token in found {
                assert!(
                    !debug_forms.contains(&token),
                    "★★★★★ `{}` publishes the socket type as `{token}`, which is \
                     its `Debug` text — the taxonomy's one spelling is \
                     `wire_word` and this register reached past it",
                    field.path,
                );
            }
        }
        assert!(
            asked > 1,
            "★ the registers have to be REACHED: {asked} answered, so this gate \
             is asking an empty screen",
        );
    });
}

/// ★★★★★ R1962 — **a card listening on one transport dials a peer that speaks
/// another, and the opening graph does exactly that.**
///
/// The blocker R1961 wrote down as prose and never measured: with ONE transport
/// per node, `LabNode::conversion` propagated equality along every wire, so
/// `spec::LINKS` forced all eight cards of the opening graph into one value and
/// the fixture could not carry a second. The domain has no such rule — a router
/// listening on tcp may dial a quic peer — and this asserts the model no longer
/// invents one.
///
/// ⚠ Asked of the OPENING graph rather than of a state the test builds, because
/// the fixture is where the debt lives: P-01 listens on quic and dials the tcp
/// router, and both halves are read from the document rather than assumed.
#[test]
fn r1962_a_card_listens_on_one_transport_and_dials_another() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();
        let split = state.node_of("P-01").expect("the specification's P-01");
        let (card, dials) = two_transports_of(&state, split);
        assert_eq!(
            card,
            Some(Transport::Quic),
            // R1966 — *speaks*, not *is drawn as*: the card body moved to the
            // kind axis, so this is a fact about the node and its pins.
            "★ P-01 speaks the address it LISTENS on",
        );
        assert_eq!(
            dials,
            Some(Transport::Tcp),
            "★★★★★ and its dial pin carries the address it DIALS — one card, \
             two transports, which one field could not hold",
        );

        // The wire that could not have existed: the document holds it, and the
        // graph is sound with it.
        let router = state.node_of("R-01").expect("the specification's router");
        let joined = state.doc.borrow().tree(ROOT).is_some_and(|tree| {
            tree.links()
                .iter()
                .any(|l| l.from.node == split && l.to.node == router)
        });
        assert!(
            joined,
            "★★★★★ the quic peer really is wired to the tcp router — before the \
             split this link was refused, `node carries Locator(Quic), node \
             expects Locator(Tcp)`",
        );

        // ★★★★★ R1969 — what stood here asserted the REFUSAL as *the rule is
        // intact where it belongs: two ends that disagree are still refused*,
        // and the canon has no such rule at all (measured — see
        // `graph::LabNode::conversion`). It was the guard R1962 put on its own
        // change, and it guarded the wrong thing: it made the split look like a
        // narrow exemption from a general law when the general law was the
        // invention. So the assertion is INVERTED rather than deleted, because
        // this test's subject — a card that listens on one scheme and dials
        // another — is exactly the pair that used to be refused.
        let crossed = <crate::graph::LabNode as pinion_node_graph::NodeKind>::conversion(
            &crate::graph::Endpoint::Locator(Transport::Quic),
            &crate::graph::Endpoint::Locator(Transport::Tcp),
        );
        assert!(
            !matches!(crossed, pinion_node_graph::Conversion::Refused),
            "★★★★★ a quic dial cannot land on a tcp listen, which is the \
             refusal that took `r1651_the_node_lab_matches_the_reference` and \
             `r1688_where_the_canvas_is_pointed` red — and which the canon \
             does not have",
        );
    });
}

/// ★ R1961 — the FILL the scene gave the box with this tag.
///
/// The sibling of [`painted_border`], and separate for the reason that one
/// records: `painted.rs`'s harness keeps rectangles, and a card's transport
/// lives in its ground rather than in its edge. Answers `None` when no box
/// carries the tag, which the callers below turn into a named failure rather
/// than a missing colour.
fn painted_fill(scene: &pinion_core::Scene, tag: &str) -> Option<pinion_core::Color> {
    use pinion_core::Scene;
    match scene {
        Scene::Container(node) if node.tag.as_deref() == Some(tag) => Some(node.style.fill),
        Scene::Box(node) if node.tag.as_deref() == Some(tag) => Some(node.style.fill),
        Scene::Container(node) => node
            .children
            .iter()
            .find_map(|child| painted_fill(child, tag)),
        Scene::Scroll(node) => painted_fill(&node.content, tag),
        _ => None,
    }
}

/// The transport the document says this node speaks: its own listen address,
/// else the one it dials.
///
/// ⚠ R1966 — this doc said *the CARD's, which is the one its colour is drawn
/// from*, and the second clause is no longer true: the card body is drawn on
/// the KIND axis now, as the canon draws it, and what a node speaks reaches the
/// screen through its PINS. The value is unchanged and still worth asserting —
/// only the claim about where it is seen was wrong, and a comment three rounds
/// away from the line it describes is the shape that goes stale.
fn spoken_by(state: &LabState, node: pinion_node_graph::NodeId) -> Option<Transport> {
    two_transports_of(state, node).0
}

/// ★ R1962 — both halves the document holds for this card: what its colour and
/// accept pins read (`listens_over ?? dials_over` and `listens_over`), and what
/// its dial pin reads.
fn two_transports_of(
    state: &LabState,
    node: pinion_node_graph::NodeId,
) -> (Option<Transport>, Option<Transport>) {
    state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.node(node))
        .and_then(|n| match &n.body {
            NodeBody::Kind(kind) => Some((kind.listens_over.or(kind.dials_over), kind.dials_over)),
            _ => None,
        })
        .expect("a kind node")
}

/// ★★★★★ R1961 — **every card on the opening canvas speaks an address that is
/// actually on it, and the canvas does not draw them all the same.**
///
/// Two halves, and the second is the one
/// `debt-every-card-on-the-opening-graph-speaks-one-transport` asked for by
/// name: *the walk must assert the POPULATION*, because a screen whose eight
/// cards are one colour cannot demonstrate that the colour is derived — "read
/// off the transport" and "constant" are indistinguishable there, which is the
/// class R1845 / R1926 / R1927 named.
///
/// The first half is the derivation itself, re-run from scratch against what
/// the document stores. It is what makes the second half mean something: a
/// canvas could be given two colours by writing two colours.
#[test]
fn r1961_the_opening_canvas_speaks_the_addresses_it_carries() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();

        // ── the derivation, re-run ──────────────────────────────────────────
        let mut listens = 0_usize;
        let mut from_the_wire = 0_usize;
        let mut unspoken: Vec<String> = Vec::new();
        for node in state.cards() {
            let name = state.name_of(node);
            let listen = state
                .forms
                .borrow()
                .get(&node)
                .and_then(|form| form.field("listen.endpoints"))
                .map_or(String::new(), |f| f.value().into_owned());
            let dialled = super::dialled_endpoint(&state.doc.borrow(), node);
            let (listens_over, dials_over) = super::transports_spoken(&listen, dialled.as_deref());
            let want = listens_over.or(dials_over);
            assert_eq!(
                two_transports_of(&state, node),
                (want, dials_over),
                "★ {name} stores transports the derivation does not give it — \
                 listen {listen:?}, dialled {dialled:?}",
            );
            match (Transport::of_locator(&listen), want) {
                (Some(_), _) => listens += 1,
                (None, Some(_)) => {
                    // ★★★★★ The repair itself: this card has NO address of its
                    // own, and the transport it speaks came off the wire it
                    // dials. Before R1961 these took the escape hatch — a
                    // classification nobody made.
                    from_the_wire += 1;
                    assert!(
                        dialled.is_some(),
                        "★ {name} speaks {want:?} and dials nothing, so nothing \
                         said it — that is the escape hatch back",
                    );
                }
                (None, None) => unspoken.push(name),
            }
        }
        assert_eq!(
            listens + from_the_wire + unspoken.len(),
            state.cards().len(),
            "every card is accounted for by one of the three: {listens} listen, \
             {from_the_wire} read the wire, {unspoken:?} speak nothing",
        );
        assert!(
            from_the_wire > 0,
            "★★★★★ the derivation this round exists for has to be REACHED: no \
             card takes its transport from the wire it dials, so the whole arm \
             is untested by this graph",
        );
        assert!(
            !unspoken.is_empty(),
            "★★★★★ and the unclassified state has to be reachable too. A graph \
             in which every card has an address cannot tell a derivation from a \
             default — which is the thing R1921 forbade and the escape hatch \
             hid for five rounds",
        );

        r1966_the_canvas_draws_kinds_and_the_pins_draw_wires(&state);
    });
}

/// ★★★★★ R1966 — **the frame half of the check above**: a card's body says what
/// it IS and its pin says what it SPEAKS.
///
/// Its own function for R1909.2's reason — the walk's job is the population and
/// the claims are a separate question about one frame — and because clippy
/// refused the two together at 106 lines, which is the same judgement said by a
/// linter.
fn r1966_the_canvas_draws_kinds_and_the_pins_draw_wires(state: &std::rc::Rc<LabState>) {
    {
        let scene = crate::painted::painted_scene(state);
        let mut fills: BTreeSet<(u8, u8, u8)> = BTreeSet::new();
        for node in state.cards() {
            let tag = format!("lab.node.{}", state.name_of(node));
            if let Some(fill) = painted_fill(&scene, &tag) {
                fills.insert((fill.r, fill.g, fill.b));
            }
        }
        assert!(
            fills.len() > 1,
            "★ the canvas paints one colour over all eight cards: {fills:?}",
        );
        // ★★★★★ R1966 — the population is the KIND palette, and it is asserted
        // by NAMING TWO KINDS rather than by counting colours.
        //
        // ⚠⚠ R1962's form of this counted how many of the painted fills were
        // `Transport::ALL` tints and demanded more than one. Measured at R1966
        // AFTER the card body moved onto the kind axis, that check still
        // reported THREE transport colours — over a canvas that paints none.
        // `peer` and `tcp` are both `#2D6CDF`, `sto` and `tls` both `#1F8A4C`,
        // `qry` and `udp` both `#C77800`: the canon's two colour systems share
        // VALUES, so a check that counts colours cannot tell the axes apart and
        // was green through the whole time the axis was wrong.
        //
        // Naming two kinds is what a count cannot do. `Router` and `Peer` are
        // the pair the canon separates most loudly — `#9A004F` against
        // `#2D6CDF` — and they are the pair a person looking at the two windows
        // would compare first.
        let fill_of = |name: &str| -> Option<(u8, u8, u8)> {
            let node = state.node_of(name)?;
            painted_fill(&scene, &format!("lab.node.{}", state.name_of(node)))
                .map(|c| (c.r, c.g, c.b))
        };
        let router = fill_of("R-01").expect("the router is painted");
        let peer = fill_of("P-01").expect("a peer is painted");
        assert_ne!(
            router, peer,
            "★★★★★ the router and a peer are painted the same colour, so the \
             canvas is not drawing cards by KIND — which is the axis the canon \
             draws them on, and the defect a person reported by comparing the \
             two windows",
        );
        for (name, role) in [("R-01", Role::Router), ("P-01", Role::Peer)] {
            let want = role.tint();
            assert_eq!(
                fill_of(name),
                Some((want.r, want.g, want.b)),
                "★ {name} is not painted the colour its kind declares — the one \
                 declaration is `Role::tint`, and a card reading anything else \
                 is a second vocabulary",
            );
        }
        // ★ And the pins still wear the WIRE, because the transport palette was
        // moved rather than deleted: that is where the canon keeps it, and the
        // legend this screen draws reads it there.
        let ring = match painted_border(&scene, "lab.pin.P-01.accept") {
            PaintedEdge::Ink(colour) => (colour.r, colour.g, colour.b),
            other => panic!("the peer's accept pin carries no colour: {other:?}"),
        };
        let quic = Transport::Quic.tint();
        assert_eq!(
            ring,
            (quic.r, quic.g, quic.b),
            "★★★★★ the accept pin no longer wears the transport it listens on, \
             so moving the card body to the kind axis took the wire's colour \
             with it instead of leaving it where the canon puts it",
        );
        assert_ne!(
            ring, peer,
            "★ and the two systems are visibly apart on one card: its body says \
             what it IS and its pin says what it SPEAKS",
        );
    }
}

/// ★★★★★ R1961 — **a card learns what it speaks from the wire it draws, and
/// unlearns it when the wire goes.**
///
/// The live half. The check above reads one opening state; this one drives the
/// screen's own verbs — `delete_link`, a form commit, `connect` — and asserts
/// the derivation after each, which is what holds every site that has to call
/// `settle_transports`. A site that forgets it leaves a stale colour, and a
/// stale colour is exactly what this screen's debt is about.
///
/// ⚠ The peer is given a **quic** address first, so the transport the wire
/// teaches is one no default could have produced. Asserting that a card became
/// TCP would pass against the escape hatch this round removed.
#[test]
fn r1961_a_card_learns_what_it_speaks_from_the_wire_it_draws() {
    let owner = Owner::new();
    owner.run(|| {
        super::reset_lab_state();
        let state = super::use_lab_state();
        let learner = state
            .cards()
            .into_iter()
            .find(|node| spoken_by(&state, *node).is_none())
            .expect("★ some card on the opening canvas speaks nothing");
        let peer = state.node_of("P-03").expect("the specification's P-03");

        // Free the peer's only endpoint: this screen refuses a second dialler
        // on an address one card already took, which is the reference's rule.
        let held = state
            .doc
            .borrow()
            .tree(ROOT)
            .map(|t| {
                t.links()
                    .iter()
                    .filter(|l| l.to.node == peer)
                    .map(|l| l.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for link in held {
            super::delete_link(&state, link).expect("a drawn link can be removed");
        }
        state
            .forms
            .borrow_mut()
            .get_mut(&peer)
            .expect("a form")
            .set("listen.endpoints", "quic/0.0.0.0:7451")
            .expect("held");
        super::sync_node(&state, peer);
        assert_eq!(
            spoken_by(&state, peer),
            Some(Transport::Quic),
            "the peer speaks the address it was given",
        );

        super::connect(&state, learner, peer).expect("★ a card that speaks nothing may dial one");
        assert_eq!(
            spoken_by(&state, learner),
            Some(Transport::Quic),
            "★★★★★ and it now speaks what the wire dials — not the TCP a \
             default would have handed it",
        );

        let drawn = state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|t| {
                t.links()
                    .iter()
                    .find(|l| l.from.node == learner)
                    .map(|l| l.id)
            })
            .expect("the wire just drawn");
        super::delete_link(&state, drawn).expect("and it can be taken away again");
        assert_eq!(
            spoken_by(&state, learner),
            None,
            "★★★★★ and it stops speaking it — a derived fact that only ever \
             grew would be a stored one wearing a derivation's name",
        );
    });
}
