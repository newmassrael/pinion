//! R1575 §5.3 §5.52 — the derivation's own tests.
//!
//! The demo (`tools/demos/r1575_graph_states_its_layers.py`) drives the live
//! window over RPC; these pin the parts that are pure functions, where a unit
//! test is the cheaper and sharper instrument.

use super::{
    AUTHORED, Dash, DiffState, NODES, anchor, build, layer_stroke, link_id, observation, parse_pair,
};
use pinion_core::reactive::Owner;
use pinion_node_graph::{Discovery, LinkLayer, ROOT};

/// Build the state outside a live shell. `DiffState::new` needs no Owner; the
/// `use_diff_state` hook does, and that is the shell's path rather than this
/// one's.
fn state() -> DiffState {
    DiffState::new()
}

/// R1575 — the three kinds really are a function of set membership.
///
/// The discriminating half is `converged`: the SAME derivation over a different
/// observation must produce an EMPTY difference. Without it this test would
/// also pass against a `diff` that hard-coded the `partial` answer.
#[test]
fn r1575_the_kind_of_a_link_is_which_sets_hold_it() {
    let state = state();

    // `partial`: one authored link never observed, one observed link never
    // authored, and the other four in both.
    assert_eq!(state.count(LinkLayer::Matched), 4, "matched under partial");
    assert_eq!(state.count(LinkLayer::Missing), 1, "missing under partial");
    assert_eq!(state.count(LinkLayer::Drift), 1, "drift under partial");
    assert_eq!(state.ids(LinkLayer::Missing), "leaf-3>peer-b");
    assert_eq!(state.ids(LinkLayer::Drift), "leaf-2>hub");

    // The same code over the converged observation.
    assert!(
        observation("converged").is_some(),
        "the converged observation is declared"
    );
    state.document.set(build("converged"));
    assert_eq!(
        state.count(LinkLayer::Matched),
        AUTHORED.len(),
        "every authored link is observed under converged",
    );
    assert_eq!(state.count(LinkLayer::Missing), 0, "nothing missing");
    assert_eq!(state.count(LinkLayer::Drift), 0, "nothing drifted");
}

/// R1575 — adopting is one assignment, and every derived fact follows from it.
///
/// This is the property that makes the derivation worth having: nothing walks
/// the links to update a stored kind, so there is no second place for the
/// answer to be wrong.
#[test]
fn r1575_adopting_the_observation_empties_the_difference() {
    let state = state();
    assert!(
        state.count(LinkLayer::Missing) + state.count(LinkLayer::Drift) > 0,
        "premise"
    );

    // R1645 — adopting is now `Document::adopt` per reported link plus a
    // retraction of what was drawn and never seen, so the AUTHORING rules apply
    // to it. Done here the way the binding does it.
    let mut document = state.document.get();
    for seen in document.layers(ROOT).drift().to_vec() {
        document
            .adopt(ROOT, seen.from, seen.to)
            .expect("the partial scenario's drift is authorable");
    }
    for id in document.layers(ROOT).missing().to_vec() {
        document
            .disconnect(ROOT, id)
            .expect("a drawn link retracts");
    }
    let reported = document.observations(ROOT).len();
    state.document.set(document);

    assert_eq!(state.count(LinkLayer::Missing), 0);
    assert_eq!(state.count(LinkLayer::Drift), 0);
    assert_eq!(
        state.count(LinkLayer::Matched),
        reported,
        "after adopting, every reported link is matched",
    );
}

/// R1645 — the scenario the crate makes reachable: the world reports a link
/// this model cannot hold, and adopting it is REFUSED by name.
///
/// A binding keeping two sets of name pairs could not produce this at all —
/// nothing in a set of pairs knows what a cycle is, so `authored = observed`
/// would have "adopted" a graph the model forbids.
#[test]
fn r1645_a_reported_link_the_model_cannot_hold_is_named() {
    let state = state();
    state.document.set(build("impossible"));
    assert_eq!(state.count(LinkLayer::Drift), 1, "one undrawn report");

    let mut document = state.document.get();
    let seen = document.layers(ROOT).drift()[0];
    let refusal = document
        .adopt(ROOT, seen.from, seen.to)
        .expect_err("it closes a cycle, and this model does not hold one");
    assert!(format!("{refusal}").contains("cannot hold it"));
    assert_eq!(
        document.layers(ROOT).drift().len(),
        1,
        "so it stays visible as drift rather than being forgotten"
    );

    // And the drawing is known to be partial while it stands — the standing is
    // derived from the drift, not from the switch.
    assert_eq!(document.discovery(), Discovery::Off);
    assert!(!document.standing(ROOT).is_certain());
    assert!(
        document.validate().is_empty(),
        "and the graph is still valid"
    );
}

/// R1575 — the diff's order is canonical, so two reads of one model are equal.
#[test]
fn r1575_the_diff_reads_the_same_twice() {
    let state = state();
    assert_eq!(state.diff(), state.diff(), "the derivation is a function");
    let kinds: Vec<LinkLayer> = state.diff().into_iter().map(|(_, k)| k).collect();
    let mut sorted = kinds.clone();
    sorted.sort();
    assert_eq!(kinds, sorted, "kinds group in the legend's reading order");
}

/// R1575 — a dashed kind carries a dash and the matched kind does not.
///
/// The paint's own claim, asserted against the kind rather than against a
/// screenshot: `Matched` must be `None` (solid is not a rhythm) and both others
/// must be `Some`, with **different** rhythms so they read apart at one width.
#[test]
fn r1575_only_the_unmatched_kinds_carry_a_dash() {
    let theme = pinion_core::theme::Theme::default();
    let matched = layer_stroke(LinkLayer::Matched, &theme);
    let missing = layer_stroke(LinkLayer::Missing, &theme);
    let drift = layer_stroke(LinkLayer::Drift, &theme);

    assert!(matched.dash.is_none(), "a confirmed link is drawn solid");
    let missing_dash = missing.dash.expect("a missing link is drawn dashed");
    let drift_dash = drift.dash.expect("a drifted link is drawn dotted");
    assert_ne!(
        missing_dash, drift_dash,
        "the two unmatched kinds must be distinguishable from each other, not \
         only from the matched one",
    );
    assert_ne!(
        missing.color, drift.color,
        "and they differ in ink as well as rhythm, so the distinction survives \
         a reader who cannot resolve the dash at this width",
    );
}

/// R1575 — the flow offset is canonical: one full period is the identity.
#[test]
fn r1575_a_full_period_of_flow_is_the_identity() {
    let period = Dash::DASHED.period();
    assert_eq!(
        Dash::DASHED.with_offset(period),
        Dash::DASHED,
        "advancing by exactly one period returns the same value, so a marching \
         animation draws from a finite set",
    );
    assert_eq!(Dash::DASHED.with_offset(period + 3).offset, 3);
    assert_eq!(
        Dash::DASHED.advanced_by(period),
        Dash::DASHED,
        "and the animation step agrees with the absolute one",
    );
}

/// R1575 — every declared link names nodes that exist, so no link is drawn to
/// nowhere. A premise the paint silently depends on (`link_scene` returns
/// `None` for an unknown endpoint, which would drop the link without a word).
#[test]
fn r1575_every_link_endpoint_is_a_declared_node() {
    let state = state();
    for ((from, to), kind) in state.diff() {
        assert!(
            anchor(&from).is_some(),
            "{} names an unknown source in the {} layer",
            link_id(&from, &to),
            kind.name(),
        );
        assert!(
            anchor(&to).is_some(),
            "{} names an unknown target in the {} layer",
            link_id(&from, &to),
            kind.name(),
        );
    }
    assert_eq!(NODES.len(), 6, "the fixture's node count is what it says");
}

/// R1575 — the argument parser accepts the shape the schema advertises and
/// rejects what it does not.
#[test]
fn r1575_a_pair_argument_is_parsed_or_refused() {
    assert_eq!(
        parse_pair(" leaf-3 , peer-b "),
        Some(("leaf-3".to_string(), "peer-b".to_string())),
        "surrounding space is the caller's formatting, not part of a name",
    );
    assert_eq!(parse_pair("leaf-3"), None, "a pair needs two halves");
}

/// R1575 — the Owner hook is what the shell uses, and it caches.
#[test]
fn r1575_the_state_hook_returns_one_model_per_owner() {
    let owner = Owner::new();
    let a: std::rc::Rc<DiffState> = owner.cache(super::STATE_KEY, DiffState::new);
    let b: std::rc::Rc<DiffState> = owner.cache(super::STATE_KEY, DiffState::new);
    assert!(
        std::rc::Rc::ptr_eq(&a, &b),
        "two reads under one Owner are one model — otherwise the view and the \
         oracle would diff different graphs",
    );
}
