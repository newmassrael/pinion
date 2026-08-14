//! R1651 — the screen against its own specification, without a window.
//!
//! The demo drives the real application through the wire and a real pointer;
//! these are the claims that do not need a process, and the ones that would be
//! expensive to notice only from a running screen: that the opening state IS
//! the specification, that the gate is derived, and that the two halves of the
//! screen agree about what a node is.

use pinion_core::reactive::Owner;
use pinion_core::widgets::config_form::Applies;

use super::{
    Hit, INSP_W, LabState, MIN_W, PALETTE_W, RAIL_W, TOOLBAR_LEFT_CLUSTER, TOOLBAR_RIGHT_CLUSTER,
    canvas_rect, card_rect, content_to_window, deploy, form_for, inspector_rect, pin_rect, spec,
};
use crate::graph::Role;

fn state() -> LabState {
    LabState::opening()
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
            state.selected.get().map(|n| state.name_of(n)).as_deref(),
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
        let form = form_for(spec::SELECTED_NODE, Role::Router);
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
        let warned = |state: &LabState| {
            state
                .gate_lines()
                .into_iter()
                .any(|(_, s)| s.starts_with("S-01") && s.contains("listening"))
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
        let transport = state
            .doc
            .borrow()
            .tree(super::ROOT)
            .and_then(|t| t.node(store))
            .and_then(|n| match &n.body {
                super::NodeBody::Kind(kind) => Some(kind.transport),
                _ => None,
            })
            .expect("a kind node");
        assert_eq!(
            transport,
            crate::graph::Transport::Quic,
            "★ and the pin's COLOUR follows the locator, because the colour is \
             the type the taxonomy refuses a mismatched link on"
        );
    });
}

#[test]
fn r1651_the_specification_and_the_taxonomy_agree_about_every_role() {
    // Both directions. The palette table is the reference's statement of what
    // the tool offers; `Role` is what the application can actually build. A
    // role in one and not the other is the drift this whole module exists to
    // make impossible.
    let declared: Vec<&str> = spec::ROLES.iter().map(|r| r.name).collect();
    let built: Vec<&str> = Role::ALL.into_iter().map(Role::name).collect();
    assert_eq!(declared, built, "same roles, same order");
    for want in spec::ROLES {
        let role = Role::from_name(want.name).expect("declared");
        assert_eq!(role.group(), want.group, "{}", want.name);
        assert_eq!(role.gist(), want.gist, "{}", want.name);
        assert_eq!(
            role.accepts(),
            want.accepts,
            "★ {} — whether a role can be dialled decides whether the canvas \
             draws it an accept pin AND whether a link to it can exist at all",
            want.name
        );
    }
}

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
        let extra = state.node_of("T-01").expect("on the canvas");
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
        let state = state();
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
                Hit::Pin { node, dial: true },
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
        let state = std::rc::Rc::new(state());
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
        let state = std::rc::Rc::new(state());
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
        state.selected.set(Some(node));

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
            state.selected.get(),
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
            state.selected.get(),
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
        assert!(!plan.nodes.is_empty(), "the opening graph has cards");

        let document = deploy::as_document(&plan);
        let script = deploy::as_script(&plan);

        let ordered: Vec<&str> = plan.nodes.iter().map(|e| e.name.as_str()).collect();
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
        for entry in &plan.nodes {
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
            before.nodes.iter().any(|e| e.name == "P-03"),
            "it starts in the plan"
        );

        super::disable_card(&state, node).expect("the card is there");
        let after = state.plan();
        assert!(
            !after.nodes.iter().any(|e| e.name == "P-03"),
            "a card that produces nothing is not started"
        );
        assert_eq!(
            after.nodes.len() + 1,
            before.nodes.len(),
            "and nothing else moved"
        );
        let script = deploy::as_script(&after);
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
        let unexpressed = plan.unexpressed();
        assert_eq!(unexpressed.len(), 1, "one row, named");
        assert_eq!(unexpressed[0].0, spec::SELECTED_NODE);
        assert_eq!(unexpressed[0].1.key, "transport.link.tx.batch_size");

        // ★ The rest of that node's configuration still ships. A refusal that
        // took the other rows with it would make one bad value cost the file.
        let entry = plan
            .nodes
            .iter()
            .find(|e| e.name == spec::SELECTED_NODE)
            .expect("still in the plan");
        assert!(
            entry.config.as_object().is_some_and(|o| !o.is_empty()),
            "the node still has a configuration: {}",
            entry.config
        );
        assert!(
            entry
                .config
                .pointer("/transport/link/tx/batch_size")
                .is_none(),
            "and the refused row is not silently in it"
        );

        let document = deploy::as_document(&plan);
        assert_eq!(
            document["unexpressed"].as_array().map(Vec::len),
            Some(1),
            "the artifact carries it"
        );
        assert!(
            deploy::as_script(&plan).contains("not in any file above"),
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
        let bar = super::toolbar_rect();
        assert_eq!(
            bar.w,
            TOOLBAR_RIGHT_CLUSTER + TOOLBAR_LEFT_CLUSTER,
            "at the floor the toolbar pane is exactly the two clusters"
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
        assert!(
            seats.len() >= 8,
            "the toolbar's roster: {:?}",
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
        assert!(
            furthest <= i64::from(super::TOOLBAR_RIGHT_CLUSTER),
            "the right-anchored cluster reaches {furthest} px in and \
             TOOLBAR_RIGHT_CLUSTER declares {}. Raise the constant — the \
             window's floor is derived from it, so a seat that outgrows it is \
             painted off the pane at the minimum size.",
            super::TOOLBAR_RIGHT_CLUSTER
        );
        assert!(
            i64::from(super::TOOLBAR_RIGHT_CLUSTER) - furthest <= 24,
            "and it declares {} for a cluster that needs {furthest} — a floor \
             reserving space nothing uses makes the window bigger than the \
             screen requires",
            super::TOOLBAR_RIGHT_CLUSTER
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
        assert_eq!(
            MIN_W,
            RAIL_W + PALETTE_W + TOOLBAR_RIGHT_CLUSTER + TOOLBAR_LEFT_CLUSTER + INSP_W,
            "★ and the floor is DERIVED from them — R1687 grew it from 1316 to \
             1442 by adding one button, which is a real cost (a 1366-wide \
             laptop no longer shows this screen unclipped) and has to be a \
             decision somebody makes rather than arithmetic nobody sees"
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
        assert_eq!(min, (MIN_W, super::MIN_H));
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
        assert!(
            (before - after).abs() <= 2,
            "the graph sits {before} px from the left of the canvas and \
             {after} px from the right"
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
            state.selected.get(),
            "and it is not the card the screen opens on"
        );

        let said = super::go_to_problem(&state);
        assert_eq!(state.selected.get(), Some(target));
        assert_eq!(said, first.sentence);
        assert_eq!(
            state.toast.get(),
            first.sentence,
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
        let seats = super::toolbar_seats(&state);
        assert!(seats.len() >= 8);
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
