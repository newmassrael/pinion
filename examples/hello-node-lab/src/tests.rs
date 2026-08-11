//! R1651 — the screen against its own specification, without a window.
//!
//! The demo drives the real application through the wire and a real pointer;
//! these are the claims that do not need a process, and the ones that would be
//! expensive to notice only from a running screen: that the opening state IS
//! the specification, that the gate is derived, and that the two halves of the
//! screen agree about what a node is.

use pinion_core::reactive::Owner;
use pinion_core::widgets::config_form::Applies;

use super::{Hit, LabState, canvas_rect, card_rect, form_for, inspector_rect, pin_rect, spec};
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
        for node in state.cards() {
            let card = card_rect(&state, node).expect("a declared node has a card");
            let centre = (card.x + card.w / 2, card.y + card.h / 2);
            assert_eq!(
                Hit::at(&state, centre.0, centre.1),
                Hit::Node(node),
                "{} is pressable at the centre of its card",
                state.name_of(node)
            );
            let dial = pin_rect(card, true);
            assert_eq!(
                Hit::at(&state, dial.x + dial.w / 2, dial.y + dial.h / 2),
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
