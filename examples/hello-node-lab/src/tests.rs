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
    Hit, LabState, canvas_rect, card_rect, content_to_window, form_for, inspector_rect, pin_rect,
    spec,
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
            .iter()
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

// ★ The scrolled-seat check lives in `painted.rs`, not here: it has to take
// the rectangle from the PAINT and ask the hit test about it, and a version
// written here would have had only `node_act_seat` to get a rectangle from —
// which is the function under test. The first draft did exactly that, aimed
// the press at the centre of what that function answered, and passed with the
// scroll offset removed from it. See
// `r1682_a_node_life_seat_is_pressable_where_it_is_painted`.
