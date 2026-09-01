//! ★★★★★ R1947 — what this section claims about itself, asserted rather than
//! written down.

use super::{Hit, spec, use_view_state};

/// The state, reset — the module-level cache is shared across tests in one
/// thread, so a test that changed the selection would otherwise decide what the
/// next one sees.
fn fresh() -> std::rc::Rc<super::ViewState> {
    let state = use_view_state();
    state.layout.set(0);
    state.selected.set(spec::OPENS_ON.to_owned());
    state.zoom.set(spec::ZOOM_FIT);
    state
        .toggles
        .set(spec::TOGGLES.iter().map(|t| t.opens_on).collect());
    state
}

/// ★★★★★ R1947 — **every standing is a word the graded vocabulary holds.**
///
/// The inspector prints a standing beside a colour, and the colour is the only
/// thing that says how bad it is. A standing whose severity the scale does not
/// hold could be drawn in any colour at all and nothing would notice — which is
/// exactly the property the reference does NOT have: its own alarm control
/// offers a word its rows do not carry.
#[test]
fn r1947_every_standing_is_graded_by_the_scale_this_application_uses() {
    for node in spec::NODES {
        assert!(
            spec::SEVERITY.rank(node.standing.severity()).is_some(),
            "{} is {:?}, graded {:?}, which the vocabulary does not hold",
            node.id,
            node.standing.label(),
            node.standing.severity(),
        );
    }
    // ★ Can this population be empty? No — a plot with no nodes is refused by
    // the assertion below, which is what keeps the loop above from passing
    // vacuously.
    assert!(
        spec::NODES.len() > 1,
        "a topology of one node is not a topology",
    );
}

/// ★★★★★ R1947 — **every declared standing is actually reached by the
/// population.**
///
/// The mirror of the test above, and the half that catches the likelier error:
/// a vocabulary can be correct and unused. If `Down` were declared and no node
/// were down, the plot would never draw the failure colour and no reader would
/// ever see what a failed peer looks like — which is a screen that is right
/// about a state it cannot demonstrate.
#[test]
fn r1947_every_standing_the_vocabulary_declares_is_drawn_by_some_node() {
    for standing in [
        spec::Standing::Active,
        spec::Standing::Serving,
        spec::Standing::Reconnecting,
        spec::Standing::Down,
    ] {
        assert!(
            spec::NODES.iter().any(|n| n.standing == standing),
            "no node is {:?}, so the plot never draws that state",
            standing.label(),
        );
    }
}

/// ★★★★★ R1947 — **every link names two nodes this plot has.**
///
/// A link to a node that is not in the population would be drawn from a place
/// the plot does not have — silently, at the origin, because a missing lookup
/// has to answer something. The reference cannot make this mistake because it
/// derives its links from a live capture; this build declares them, so this is
/// the assertion that declaration buys.
#[test]
fn r1947_every_link_joins_two_declared_nodes() {
    for link in spec::LINKS {
        assert!(
            spec::node(link.from).is_some(),
            "a link is drawn from {}, which is not a node",
            link.from
        );
        assert!(
            spec::node(link.to).is_some(),
            "a link is drawn to {}, which is not a node",
            link.to
        );
        assert_ne!(link.from, link.to, "a link joins a node to itself");
    }
}

/// ★★★★★ R1947 — **no two nodes are drawn in the same place, in either
/// layout.**
///
/// A plot's whole claim is that a reader can tell two peers apart by looking.
/// Two nodes at one point is not a rendering artifact — it is a placement table
/// that lost an entry, and it is invisible in a screenshot because the second
/// node is exactly under the first.
#[test]
fn r1947_no_two_nodes_share_a_place_in_either_layout() {
    for layout in spec::LAYOUTS {
        for (n, one) in spec::NODES.iter().enumerate() {
            for other in &spec::NODES[n + 1..] {
                assert_ne!(
                    one.at(layout),
                    other.at(layout),
                    "{} and {} are at one place in the {} layout",
                    one.id,
                    other.id,
                    layout.in_force(),
                );
            }
        }
    }
}

/// ★★★★★ R1947 — **the two layouts are actually different arrangements.**
///
/// The header states which layout is in force, and the segmented control
/// switches between them. If both tables held the same places, every one of
/// those affordances would work, say the right thing, and change nothing a
/// reader can see — a control that is right about a difference it does not
/// make.
#[test]
fn r1947_choosing_the_other_layout_moves_the_plot() {
    let moved = spec::NODES
        .iter()
        .filter(|n| n.at(spec::Layout::Force) != n.at(spec::Layout::Hierarchical))
        .count();
    assert_eq!(
        moved,
        spec::NODES.len(),
        "only {moved} of {} nodes move between the two layouts",
        spec::NODES.len(),
    );
}

/// ★★★★★ R1947 — **a press by point and a press by tag are one behaviour.**
///
/// Two addresses into one hit test, which is what lets the wire drive exactly
/// what a pointer reaches. A screen with two resolvers has two behaviours and
/// only one of them is the one a person gets.
#[test]
fn r1947_a_hit_resolves_the_same_from_a_point_and_from_a_tag() {
    let state = fresh();
    for node in spec::NODES {
        let rect = super::node_rect(node, state.layout(), state.zoom.get());
        let by_point = Hit::at(&state, rect.x + rect.w / 2, rect.y + rect.h / 2);
        let by_tag = Hit::of_tag(&format!("tv.node.{}", node.id));
        assert_eq!(
            by_point, by_tag,
            "{} resolves differently by point and by tag",
            node.id
        );
        assert!(
            by_tag.word().is_some(),
            "{} answers no word on the wire",
            node.id
        );
    }
}

/// ★★★★★ R1947 — **a toggle hides a class of link and says which.**
///
/// The measurement is on the count the wire publishes rather than on the flag,
/// because a flag that flips while the plot draws the same lines is the defect
/// this is for.
#[test]
fn r1947_turning_a_link_class_off_draws_fewer_links() {
    let state = fresh();
    let drawn = |state: &std::rc::Rc<super::ViewState>| {
        spec::LINKS.iter().filter(|l| state.draws(l.kind)).count()
    };
    let all = drawn(&state);
    assert_eq!(
        all,
        spec::LINKS.len(),
        "the section opens showing every link"
    );
    for (n, toggle) in spec::TOGGLES.iter().enumerate() {
        if toggle.group != "links" {
            continue;
        }
        super::flip_toggle(&state, n);
        let fewer = drawn(&state);
        assert!(
            fewer < all,
            "turning {} off left {fewer} links drawn, the same as {all}",
            toggle.title
        );
        super::flip_toggle(&state, n);
        assert_eq!(drawn(&state), all, "turning it back on restored the plot");
    }
}

/// ★★★★★ R1947 — **the zoom is bounded at both ends, and `Fit` returns.**
///
/// A zoom that runs away is a plot a reader cannot get back to. Asserted by
/// pressing past the bound rather than by reading the constant, so a clamp that
/// is written and not applied fails here.
#[test]
fn r1947_the_zoom_is_bounded_and_fit_returns_to_the_opening_zoom() {
    let state = fresh();
    for _ in 0..12 {
        super::zoom_by(&state, true);
    }
    // ★ The ceiling is the DERIVED one — the closest this plot can be drawn and
    // still hold every node — not the declared `ZOOM_MAX`, which is only the
    // larger of the two bounds. Comparing against the derivation is what R1882
    // asks for: a gate that re-spells the rule can disagree with it.
    assert_eq!(
        state.zoom.get(),
        super::zoom_ceiling(),
        "zooming in is bounded by what the plot can hold"
    );
    assert!(
        super::zoom_ceiling() <= spec::ZOOM_MAX,
        "the derived ceiling cannot exceed the declared one"
    );
    assert!(
        super::zoom_ceiling() > spec::ZOOM_FIT,
        "a ceiling at or below the opening zoom would make zooming in do nothing"
    );
    for _ in 0..24 {
        super::zoom_by(&state, false);
    }
    assert_eq!(state.zoom.get(), spec::ZOOM_MIN, "zooming out is bounded");
    super::fit(&state);
    assert_eq!(
        state.zoom.get(),
        spec::ZOOM_FIT,
        "fit returns the plot to the zoom it opened at"
    );
}

/// ★★★★★ R1947 — **an action that cannot act says which requirement books it.**
///
/// Drawn and refused, which is this tree's treatment of a reference control
/// leading out of a section nothing has built. What is asserted is the
/// SENTENCE, because a refusal a reader cannot read is the same as a button
/// that does nothing.
#[test]
fn r1947_a_refused_action_names_the_requirement_that_books_it() {
    let state = fresh();
    for (n, action) in spec::ACTIONS.iter().enumerate() {
        super::refuse_action(&state, n);
        let said = state.said_sentence();
        assert!(
            said.contains(action.reserved_for),
            "{} refused with {said:?}, which names no requirement",
            action.title
        );
        assert!(
            said.contains(action.title),
            "{} refused with {said:?}, which does not say what refused",
            action.title
        );
    }
}

/// ★★★★★ R1947 — **the keyboard reaches every node, and comes back round.**
///
/// The reference has no keyboard path into its plot at all — measured, its
/// nodes are click-only — so this is second-stage work rather than
/// reproduction, and it is kept for the reason the owner's ordering rule gives:
/// what this build has and the reference does not is not removed.
#[test]
fn r1947_the_arrows_reach_every_node_and_wrap() {
    let state = fresh();
    let mut seen = vec![state.selected.get()];
    for _ in 1..spec::NODES.len() {
        assert!(
            super::key_at(&state, "ArrowRight"),
            "the arrow moved nothing"
        );
        seen.push(state.selected.get());
    }
    for node in spec::NODES {
        assert!(
            seen.iter().any(|id| id == node.id),
            "{} is not reachable from the keyboard",
            node.id
        );
    }
    assert!(super::key_at(&state, "ArrowRight"), "the walk wraps");
    assert_eq!(
        state.selected.get(),
        spec::OPENS_ON,
        "and wrapping returns to where it started"
    );
}

/// ★★★★★ R1947 — **every described mark is one this screen can actually be
/// resting on.**
///
/// A description for a tag nothing draws is a sentence no reader will ever see,
/// and it is the error direction that inflates a register silently. Checked
/// through the same resolver the pointer uses.
#[test]
fn r1947_every_description_belongs_to_a_mark_the_pointer_can_rest_on() {
    let described = super::descriptions();
    let tags: Vec<String> = described.tags().map(str::to_owned).collect();
    assert!(!tags.is_empty(), "the register describes nothing");
    for tag in &tags {
        assert_ne!(
            Hit::of_tag(tag),
            Hit::Nothing,
            "{tag} is described and nothing resolves it",
        );
    }
}

/// ★★★★★ R1947 — **the pin and this build name the same parts, in the same
/// order.**
///
/// The tables in `spec` are what the painter draws from and what `judge` titles
/// from; the pin is what the section is judged against. Two lists that must
/// agree, asserted here rather than at the paint, so a mismatch is reported as
/// itself instead of as a conformance failure whose cause is a typo.
#[test]
fn r1947_the_specified_parts_are_the_parts_this_build_tables() {
    let document = spec::document();
    for (surface, table) in [
        ("filters", spec::FILTERS),
        ("graph", spec::GRAPH),
        ("inspector", spec::INSPECTOR),
    ] {
        let canon = document
            .canon(surface)
            .unwrap_or_else(|| panic!("the pin declares no surface {surface}"));
        let specified: Vec<&str> = canon.parts().iter().map(|p| p.key.as_ref()).collect();
        let tabled: Vec<&str> = table.iter().map(|p| p.key).collect();
        assert_eq!(
            tabled, specified,
            "{surface}'s tabled parts are not the pin's, in order",
        );
        for part in canon.parts() {
            let ours = table
                .iter()
                .find(|p| p.key == part.key)
                .expect("the keys were just compared");
            assert_eq!(
                ours.title, part.title,
                "{surface}.{} is called two things",
                part.key
            );
        }
    }
}
