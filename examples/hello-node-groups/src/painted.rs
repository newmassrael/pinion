//! ★★★★★ R2174 §5.2 §5.38 — **what this screen paints, held to its VALUES.**
//!
//! # ⚠ This screen had no paint-side test at all
//!
//! `tests.rs` next door asserts the MODEL — that grouping derives an interface,
//! that a bypass cycle is refused, that a fragment round-trips. Every one of
//! those runs on a [`Document`](pinion_node_graph::Document) and none of them
//! calls [`view`](super::view). So the addresses this screen paints were
//! compared with nothing in this crate, and the only readers holding them to a
//! value were the ten literals in three walks that R2174 is converting.
//!
//! That is the campaign's own rule — PIN BEFORE CONVERTING — and it is why this
//! module exists before the walks change: converting the last speller of a
//! family nothing pins *removes* its check rather than repaying it, which
//! `tools/painted_addresses.py --check` refuses by name.
//!
//! # ★ The sweep is over STATES, not sizes
//!
//! The sibling screens sweep states x sizes because their layout is solved.
//! This one lays every mark out at an absolute canvas coordinate under a fixed
//! [`WIN_W`](super::WIN_W) x [`WIN_H`](super::WIN_H), so a second size would
//! paint the identical set and the pin would claim a coverage it does not have.
//! What varies here is what the DOCUMENT holds, so that is what is swept.

use std::rc::Rc;

use pinion_core::reactive::Owner;
use pinion_core::test_fixtures::address_pin;
use pinion_node_graph::{NodeBody, NodeId, ROOT};

use super::{GroupsState, use_groups_state, view};

/// The seeded mix — the node with two colours and an amount crossing into it.
const MIX: NodeId = NodeId(3);

/// The seeded fade, which is what gets collapsed into a definition below.
const FADE: NodeId = NodeId(4);

/// One swept state: what to call it, and the edit that reaches it.
type SweptState = (&'static str, fn(&Rc<GroupsState>));

/// The states this screen is swept in.
///
/// ★ Cumulative, as the sibling screens' sweeps are: the last case is a
/// document that has been bypassed, fenced, collapsed and descended into rather
/// than one that has had exactly one thing done to it.
///
/// ★★ Each case is here because it paints a family the case before it does not.
/// The opening draws cards, titles, pins, pin labels and wires; a bypass is the
/// only thing that draws a `through`; a fence is the only thing that draws a
/// `frame` and its caption; and descending is what puts the breadcrumb
/// somewhere other than the root.
const STATES: [SweptState; 5] = [
    ("opening", |_| {}),
    ("the mix bypassed", |state| {
        state
            .edit(|document| document.set_bypassed(ROOT, MIX, true).map(drop))
            .expect("the seeded mix routes colour to colour, so it may bypass");
    }),
    ("the swatches fenced", |state| {
        let fenced = [NodeId(0), NodeId(1)];
        state.selection.set(fenced.to_vec());
        state
            .edit(|document| {
                document
                    .enframe(ROOT, &fenced, Some("Sources".to_owned()))
                    .map(drop)
            })
            .expect("two nodes of one tree can be fenced");
    }),
    ("the fade collapsed", |state| {
        state.selection.set(vec![FADE]);
        state
            .edit(|document| document.group(ROOT, &[FADE], "Finish").map(drop))
            .expect("one node with one input and one output collapses");
    }),
    ("inside the definition", |state| {
        let document = state.document.get();
        let host = state.path.get().current();
        let instance = document
            .tree(host)
            .expect("the host tree is the one being edited")
            .nodes()
            .find(|node| matches!(node.body, NodeBody::Group(_)))
            .expect("the case before this one left an instance behind")
            .id;
        let mut path = state.path.get();
        path.enter(&document, instance)
            .expect("a group instance is what an edit path descends into");
        state.path.set(path);
        state.selection.set(Vec::new());
    }),
];

/// Every tag this screen paints across [`STATES`].
///
/// ⚠ A fresh [`Owner`] per case and no `reset`: an owner's cache is its own, so
/// [`use_groups_state`] hands back a newly seeded document each time. That is
/// an assumption about the substrate rather than about this screen, so it is
/// ASSERTED below instead of trusted — a leaked cache would otherwise show up
/// as a pin that churns, which is the failure that teaches nothing.
fn swept_tags() -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for n in 0..STATES.len() {
        let owner = Owner::new();
        owner.run(|| {
            let state = use_groups_state();
            assert!(
                state.path.get().current() == ROOT && state.selection.get().is_empty(),
                "case {:?} opened on a document the case before it left behind",
                STATES[n].0
            );
            for (_, edit) in &STATES[..=n] {
                edit(&state);
            }
            tags.extend(view().tags());
        });
    }
    tags
}

/// Where this screen's pinned address set lives, for the regeneration path.
const PIN_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/painted_addresses.pin");

/// ★★★★★ R2174 — **what this screen addresses is pinned by its VALUE.**
///
/// R2137.3 is the measurement underneath: the declaration in `address.rs` means
/// paint, wire and walks all derive one address from one constant, so they all
/// move together when it moves. That improves the LIKELIHOOD of a typo and
/// creates no DETECTION. This is the detection.
///
/// Regenerate with
/// `PINION_REGEN_ADDRESS_PIN=1 cargo test -p hello-node-groups`.
#[test]
fn r2174_every_address_this_screen_paints_is_pinned_by_value() {
    let tags = swept_tags();
    let seen = address_pin::fold(tags.iter().map(String::as_str));
    address_pin::check(
        &seen,
        include_str!("painted_addresses.pin"),
        PIN_PATH,
        // ⚠ A floor, not decoration: near it means the sweep stopped reaching
        // the screen rather than that the screen shrank, and a pin compared
        // against almost nothing is green for having asked nothing.
        10,
    );
}
