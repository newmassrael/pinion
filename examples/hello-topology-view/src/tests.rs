//! ★★★★★ R1947 — what this section claims about itself, asserted rather than
//! written down.

use super::{Hit, address, spec, use_view_state};

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
        let by_tag = Hit::of_tag(&address::node(node.id));
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

/// Every module of this crate, for the address gate to read.
fn crate_sources() -> [(&'static str, &'static str); 6] {
    [
        ("address.rs", include_str!("address.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("spec.rs", include_str!("spec.rs")),
        ("judge.rs", include_str!("judge.rs")),
        ("painted.rs", include_str!("painted.rs")),
        ("tests.rs", include_str!("tests.rs")),
    ]
}

/// ★★★★★ R2122 — **every module this crate declares is one the address gate
/// reads.**
///
/// Derived from `lib.rs`'s own `mod` lines rather than compared with a second
/// list. Without it the roster above is hand-written, and a module added
/// tomorrow could spell a hundred addresses while the namespace gate reported
/// zero — the shape R2053 measured on the node lab, where the gate's own
/// population was six modules of eleven and the one nobody had looked at had
/// been composing addresses all along.
#[test]
fn r2122_every_module_is_read() {
    const LIB: &str = include_str!("lib.rs");
    let mut declared: Vec<String> = LIB
        .lines()
        .filter_map(|line| {
            let name = line
                .trim()
                .strip_prefix("pub mod ")
                .or_else(|| line.trim().strip_prefix("mod "))?
                .strip_suffix(';')?;
            (!name.contains(' ')).then(|| format!("{name}.rs"))
        })
        .collect();
    declared.push("lib.rs".to_owned());
    declared.sort_unstable();
    declared.dedup();
    let mut read: Vec<String> = crate_sources()
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    read.sort_unstable();
    assert_eq!(
        declared, read,
        "★★★★★ the address gate reads {read:?} and this crate declares \
         {declared:?} — a module outside that roster can spell any address it \
         likes and the namespace gate below will report zero"
    );
}

/// ★★★★★ R2122 — **nobody but the declaration spells this screen's
/// namespace.**
///
/// The end state of the paint-address campaign for this section, and an
/// assertion of ZERO rather than a ratchet because zero is where this crate
/// actually stands: measured at entry, **ninety-four literals** over
/// ninety-four lines, every one of them inside these six files — the shell
/// spells none and the walks spell none, which is what let one round reach the
/// end state the capture viewer needed seven instalments for.
///
/// ⚠ The census charges eighty-three of those ninety-four; see `address.rs`'s
/// header for why the two numbers are both right. THIS gate counts literals,
/// not charged sites, because what it refuses is a second SPELLING — and the
/// bare stem, quoted with no separator after it, is one even though no census
/// would bill it.
///
/// ⚠⚠ And that sentence cannot NAME the thing it is about, which is this gate
/// working rather than a limitation: the needle is plain text over the whole
/// file, comments included, so a doc comment quoting the stem is a speller. The
/// first draft of the paragraph above did exactly that and this gate refused
/// it, `[("tests.rs", 1)]`.
///
/// ⚠ The needle is the BARE stem rather than the namespace, so a module
/// spelling the stem with no separator — `paint_stems` wanted exactly that
/// until this round — is caught too. The namespace on its own would have let
/// that one literal stand.
///
/// ⚠⚠ The vacuity guard comes FIRST. A needle that matched nothing would clear
/// every module below and read exactly like success, which is the failure this
/// family of gates is most exposed to: the emptiness IS the claim, so an empty
/// needle proves it by accident.
#[test]
fn r2122_no_module_but_the_declaration_spells_this_screens_namespace() {
    let anchor = format!("\"{}", address::STEM);
    let sources = crate_sources();
    let declaration = sources
        .iter()
        .find(|(name, _)| *name == "address.rs")
        .expect("the declaring module is in the roster");
    let declared = declaration.1.matches(anchor.as_str()).count();
    assert!(
        declared >= 15,
        "★ `address.rs` spells the stem {declared} time(s), which is not what a \
         module declaring three panes, a root, a tip, a highlight field and \
         five keyed families looks like — the anchor is wrong and the emptiness \
         below means nothing"
    );
    let mut spellers: Vec<(&str, usize)> = Vec::new();
    for (name, body) in &sources {
        if *name == "address.rs" {
            continue;
        }
        let count = body.matches(anchor.as_str()).count();
        if count > 0 {
            spellers.push((name, count));
        }
    }
    assert!(
        spellers.is_empty(),
        "★★★★★ every painted address of this screen begins `{}` and is \
         declared in `address.rs`; these (file, times) spell one themselves: \
         {spellers:?}. A literal here is a second copy of a composition the \
         declaration already publishes, and one wrong letter in it paints a \
         mark no reader can find.",
        address::NAMESPACE
    );
}

/// ★★★★★ R2122 — **the declaration agrees with itself and with the
/// specification, and its inverses are disjoint.**
///
/// The second half of the gate above, and its own test rather than a clause of
/// it: *nobody re-spells the address* and *the composition is right* are two
/// claims, and the first is satisfied by a declaration that composes nonsense.
#[test]
fn r2122_every_topology_address_is_derived() {
    // The two spellings of the stem agree, so a reader matching on the bare
    // form cannot drift from one composing on the separator form.
    assert_eq!(address::NAMESPACE, format!("{}.", address::STEM));
    // The two forms of each pane's prefix agree, for the same reason.
    assert_eq!(address::FILTERS_SEAT, format!("{}.", address::FILTERS));
    assert_eq!(address::GRAPH_SEAT, format!("{}.", address::GRAPH));
    assert_eq!(address::INSPECTOR_SEAT, format!("{}.", address::INSPECTOR));
    // ★ The four parts a caller needs as a `&'static str` ARE the derivation.
    assert_eq!(address::CANVAS, address::graph("canvas"));
    assert_eq!(address::FIT, address::graph("fit"));
    assert_eq!(address::ZOOM_IN, address::graph("zoom_in"));
    assert_eq!(address::ZOOM_OUT, address::graph("zoom_out"));
    // ★★ Every part the SPECIFICATION names round-trips through its own pane's
    // inverse and is refused by the other two — and the pane is most of the
    // answer, because all three tables name a part `title`.
    let mut checked = 0;
    for (pane, table) in [
        (0_usize, spec::FILTERS),
        (1, spec::GRAPH),
        (2, spec::INSPECTOR),
    ] {
        for part in table {
            let tag = match pane {
                0 => address::filters(part.key),
                1 => address::graph(part.key),
                _ => address::inspector(part.key),
            };
            let recovered = [
                address::filters_part(&tag),
                address::graph_part(&tag),
                address::inspector_part(&tag),
            ];
            assert_eq!(
                recovered
                    .iter()
                    .filter(|answer| **answer == Some(part.key))
                    .count(),
                1,
                "★ `{tag}` is claimed by {recovered:?} — exactly one pane's \
                 inverse may answer for a part, or a reader looking a key up \
                 finds the wrong pane's row"
            );
            assert_eq!(recovered[pane], Some(part.key));
            checked += 1;
        }
    }
    assert!(
        checked >= 25,
        "{checked} part(s) were round-tripped, and a specification whose \
         tables had emptied would pass every assertion above by describing \
         nothing"
    );
    // ★★★ A pane's own tag is not one of its parts — container and content,
    // and a prefix that swallowed the separator would resolve a reader asking
    // about the PANE to whichever part sorted first.
    assert_eq!(address::filters_part(address::FILTERS), None);
    assert_eq!(address::graph_part(address::GRAPH), None);
    assert_eq!(address::inspector_part(address::INSPECTOR), None);
    // ★★★★ Everything this module composes is IN the namespace, and the
    // membership test says so. Without this the gate above could be satisfied
    // by a declaration that stopped composing `tv.` at all.
    for (_, tag) in address::parts() {
        assert!(
            address::ours(&tag),
            "`{tag}` is not in this screen's namespace"
        );
    }
    for lone in [
        address::ROOT,
        address::FILTERS,
        address::GRAPH,
        address::INSPECTOR,
        address::TIP,
        address::HIGHLIGHT,
        address::CANVAS,
        address::FIT,
        address::ZOOM_IN,
        address::ZOOM_OUT,
    ] {
        assert!(
            address::ours(lone),
            "`{lone}` is not in this screen's namespace"
        );
    }
}

/// ★★★★★ R2122 — **what is INSIDE a mark is not that mark, in every family
/// whose stem carries two vocabularies.**
///
/// Split from the gate above at the line budget's insistence, and a better seam
/// than it looks — the same seam R2121 was pushed into: *the panes compose
/// their parts correctly* and *a ring is not a node* fail for different reasons
/// and should say which. This half fails in the direction that reads as sense:
/// a reader handed `badge.box` as a part key looks it up in the specification
/// and finds nothing, quietly.
#[test]
fn r2122_what_is_inside_a_mark_is_not_that_mark() {
    let badge = address::inspector("badge");
    let badge_box = address::child(&badge, "box");
    assert_eq!(address::inspector_part(&badge_box), None);
    assert_eq!(address::child_of(&badge_box), Some((badge.as_str(), "box")));
    assert_eq!(
        address::entry_of(&badge_box),
        None,
        "★ `{badge_box}` is a part's child and the entry inverse claimed it — \
         an ordinal and a child word are two different things hanging off one \
         part, and a reader told `box` is entry 0 files it under the band"
    );
    assert_eq!(address::inspector_part(&address::entry("keys", 1)), None);
    assert_eq!(
        address::entry_of(&address::entry("keys", 1)),
        Some(("keys", 1))
    );
    assert_eq!(address::entry_of(&badge), None);
    // ★★★★★ The two keyed families whose members carry a child, which is the
    // same ambiguity a level down.
    for node in spec::NODES {
        let tag = address::node(node.id);
        assert_eq!(address::node_id(&tag), Some(node.id));
        let ring = address::child(&tag, "ring");
        assert_eq!(
            address::node_id(&ring),
            None,
            "★ `{ring}` is a ring and the node inverse claimed it — a press on \
             it would then pick whatever that spelling happened to parse as"
        );
        assert_eq!(address::child_of(&ring), Some((tag.as_str(), "ring")));
    }
    for toggle in spec::TOGGLES {
        let tag = address::toggle(toggle.key);
        assert_eq!(address::toggle_key(&tag), Some(toggle.key));
        let knob = address::child(&tag, "knob");
        assert_eq!(address::toggle_key(&knob), None);
        assert_eq!(address::child_of(&knob), Some((tag.as_str(), "knob")));
    }
    // ★★★★★★ The ordinal families, and that no other family's inverse answers
    // for one of them.
    for n in 0..spec::LAYOUTS.len() {
        let tag = address::layout(n);
        assert_eq!(address::layout_index(&tag), Some(n));
        assert_eq!(address::chip_index(&tag), None);
        assert_eq!(address::node_id(&tag), None);
    }
    for n in 0..spec::HIGHLIGHT_CHIPS.len() {
        let tag = address::chip(n);
        assert_eq!(address::chip_index(&tag), Some(n));
        assert_eq!(address::layout_index(&tag), None);
    }
    for n in 0..spec::LINKS.len() {
        let tag = address::link_label(n);
        assert_eq!(address::link_label_index(&tag), Some(n));
        assert_eq!(
            address::link_label_index(&format!("{}{n}", address::LINK_SEAT)),
            None,
            "★ this screen paints no bare link mark, and an inverse that \
             answered for one would hand a reader an address to go looking for"
        );
    }
    // ★★★★★★★ And a fifth child word cannot be addressed without being
    // declared, which is what keeps the part inverses able to say where a
    // mark's address ends.
    assert!(
        std::panic::catch_unwind(|| address::child(&badge, "halo")).is_err(),
        "★ `halo` is not one of this screen's child words and the declaration \
         composed an address for it anyway — an open suffix here makes every \
         inverse above unable to tell a mark from what is inside it"
    );
}
