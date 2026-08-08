//! R1602 — a census verdict is proven by a test.
//!
//! # Why this file exists
//!
//! R1601 made the reference census a *tool*: `tools/reference_census.py`
//! enumerates what Blender and Unreal register and `docs/reference-census.json`
//! carries one verdict per operator. What that tool proves is **completeness** —
//! no reference operator is silently absorbed into a percentage. What it does
//! not prove, and cannot, is that any verdict is **true**.
//!
//! The asymmetry is what makes that dangerous. A wrong `absent` leaves a fake
//! item on the gap list and the next round discovers it by trying to close it,
//! so it self-corrects. A wrong `have` **inflates the coverage number and
//! nobody trips over it** — and the error R1601 itself corrected was exactly
//! that direction, 93% down to 78%.
//!
//! So `covered_by` naming an API is a hand claim, and the ones most worth
//! doubting are the claims that a capability falls out of a *composition*
//! ("call `expose` again", "there is nothing to sync"). A composition is a
//! **hypothesis until it is run** — R1571 already learned that by building
//! R1555's written prescription and finding it harmful.
//!
//! This file is the answer: every `have` verdict this crate is responsible for
//! names a test here, the test exercises the capability **through the public
//! API only** (the position an application is in), and the pin and the proofs
//! are asserted to be in bijection. The coverage number is then something
//! `cargo test` re-derives rather than something a session remembered.
//!
//! # What a proof has to do
//!
//! Reach the capability the reference operator names, and assert an outcome
//! that would differ if the capability were missing. A proof that only calls a
//! method and unwraps it proves the method compiles.
//!
//! Three verdicts are **not** here and are not missing: `NODE_OT_select_box`,
//! `NODE_OT_select_circle` and `NODE_OT_select_lasso` are region tests over
//! *drawn* geometry, which `select.rs` measured as belonging to the scene layer
//! rather than to a node model. Their proofs live where the capability does, in
//! `pinion-core`, and the pin says so — which is why a proof is addressed
//! `<crate>::<test>` rather than by a bare name.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use pinion_node_graph::{
    Appearance, Crossings, Definitions, Document, EditPath, Fragment, Grow, InterfaceSide, LinkId,
    NodeBody, NodeId, NodeKind, Port, PortRef, ROOT, Reach, Sharing, Socket, TreeId,
};

// ---------------------------------------------------------------- taxonomy

/// Two socket types, so type disagreement is reachable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Ty {
    Number,
    Text,
}

/// The application's own taxonomy — the half this crate deliberately does not
/// own. Small, and every member is here because some proof needs its shape.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Op {
    /// A constant. No inputs, so it is a source.
    Num(i64),
    /// A constant of the other type, so a refused wire is reachable.
    Word(String),
    /// `(Augend: Number = 0, Addend: Number = 1) -> Out: Number`. Both inputs
    /// carry a default, which is what makes a cleared authored value
    /// observable as a *value* rather than as an absence.
    Add,
    /// `(Augend: Number = 2, Factor: Number) -> Out: Number`. `Augend` matches
    /// `Add`'s by name and `Factor` does not, and `Factor` has **no default** —
    /// the shape Unreal's "hide pins with no connection and no default" needs.
    Mul,
    /// `(Value: Number) -> Out: Number`. One in, one out: the shape a dissolve
    /// and a bypass are about.
    Double,
    /// `(Phrase: Text) -> Out: Text`.
    Shout,
    /// `(Result: Number) -> ()`. A sink, so a graph has an end.
    Sink,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Val {
    Number(i64),
    Text(String),
}

impl Val {
    fn number(&self) -> Option<i64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Text(_) => None,
        }
    }
}

impl NodeKind for Op {
    type Type = Ty;
    type Value = Val;

    fn name(&self) -> String {
        match self {
            Self::Num(_) => "Num",
            Self::Word(_) => "Word",
            Self::Add => "Add",
            Self::Mul => "Mul",
            Self::Double => "Double",
            Self::Shout => "Shout",
            Self::Sink => "Sink",
        }
        .to_owned()
    }

    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Num(_) | Self::Word(_) => Vec::new(),
            Self::Add => vec![
                Port::new("Augend", Ty::Number).with_default(Val::Number(0)),
                Port::new("Addend", Ty::Number).with_default(Val::Number(1)),
            ],
            Self::Mul => vec![
                Port::new("Augend", Ty::Number).with_default(Val::Number(2)),
                Port::new("Factor", Ty::Number),
            ],
            Self::Double => vec![Port::new("Value", Ty::Number)],
            Self::Shout => vec![Port::new("Phrase", Ty::Text)],
            Self::Sink => vec![Port::new("Result", Ty::Number)],
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Num(_) | Self::Add | Self::Mul | Self::Double => {
                vec![Port::new("Out", Ty::Number)]
            }
            Self::Word(_) | Self::Shout => vec![Port::new("Out", Ty::Text)],
            Self::Sink => Vec::new(),
        }
    }

    fn evaluate(&self, inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        let number = |i: usize| inputs.get(i).and_then(Option::as_ref).and_then(Val::number);
        match self {
            Self::Num(n) => vec![Some(Val::Number(*n))],
            Self::Word(w) => vec![Some(Val::Text(w.clone()))],
            Self::Add => vec![number(0).zip(number(1)).map(|(a, b)| Val::Number(a + b))],
            Self::Mul => vec![number(0).zip(number(1)).map(|(a, b)| Val::Number(a * b))],
            Self::Double => vec![number(0).map(|v| Val::Number(v * 2))],
            Self::Shout => vec![inputs.first().and_then(Option::as_ref).map(|v| match v {
                Val::Text(t) => Val::Text(t.to_uppercase()),
                other @ Val::Number(_) => other.clone(),
            })],
            Self::Sink => Vec::new(),
        }
    }
}

// ----------------------------------------------------------------- helpers

fn num(document: &mut Document<Op>, n: i64) -> NodeId {
    document
        .add_node(ROOT, NodeBody::Kind(Op::Num(n)), 0, 0)
        .unwrap()
}

fn node(document: &mut Document<Op>, op: Op) -> NodeId {
    document.add_node(ROOT, NodeBody::Kind(op), 0, 0).unwrap()
}

fn wire(document: &mut Document<Op>, from: NodeId, out: u32, to: NodeId, input: u32) {
    document
        .connect(ROOT, Socket::new(from, out), Socket::new(to, input))
        .unwrap();
}

/// `two -> add.0`, `three -> add.1`, `add -> sink.0`. The one chain almost every
/// proof needs, so no proof spends its lines building it.
struct Chain {
    document: Document<Op>,
    two: NodeId,
    three: NodeId,
    add: NodeId,
    sink: NodeId,
}

fn chain() -> Chain {
    let mut document = Document::new("root");
    let two = num(&mut document, 2);
    let three = num(&mut document, 3);
    let add = node(&mut document, Op::Add);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, add, 0);
    wire(&mut document, three, 0, add, 1);
    wire(&mut document, add, 0, sink, 0);
    Chain {
        document,
        two,
        three,
        add,
        sink,
    }
}

/// What actually arrives at a socket, evaluated end to end.
///
/// The chain ends in a sink, which has no outputs — so "the graph still
/// computes what it computed" is a statement about what reaches the sink's
/// input rather than about what it answers, and this is the accessor that makes
/// that assertable.
fn arrives(document: &Document<Op>, socket: Socket) -> Option<Val> {
    let mut evaluator = document.evaluator();
    evaluator.input(ROOT, socket)
}

/// Every link id touching `node`, either end. What a "break all links on this
/// node" gesture names, computed the way an application would.
fn links_touching(document: &Document<Op>, tree: TreeId, target: NodeId) -> Vec<LinkId> {
    document
        .tree(tree)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.from.node == target || link.to.node == target)
        .map(|link| link.id)
        .collect()
}

// -------------------------------------------------------------- the pin

/// The committed judgement, read at test time so the two cannot drift.
const PIN: &str = include_str!("../../../docs/reference-census.json");

/// Which crate's census this file is.
const CRATE: &str = "pinion-node-graph";

#[derive(Deserialize)]
struct Row {
    verdict: String,
    #[serde(default)]
    proven_by: String,
}

/// The proof for each `have` verdict this crate answers for.
///
/// The third element is the proof itself, so the compiler — not a grep — is
/// what checks it exists. Its *name* is derived from the operator by
/// [`proof_name`], so a row and its proof cannot end up describing different
/// capabilities without the bijection test saying so.
/// One `have` verdict and the proof that runs it.
struct Proof {
    tree: &'static str,
    operator: &'static str,
    /// The proof's own name, taken from the **compiler**.
    name: &'static str,
    run: Box<dyn Fn()>,
}

/// Bind a verdict to its proof, with the proof's name read off the function
/// itself rather than transcribed beside it.
///
/// ★ This shape was chosen by a counterfactual. The first draft stored a bare
/// `fn()` pointer, and renaming a proof was **not caught**: the bijection
/// compared operator names on both sides and the derived name against the pin,
/// so the function's own identifier was never in the comparison at all and
/// `proven_by` could name a test that no longer existed. `F` here is the
/// function's *item* type, so `type_name_of_val` answers with its path — the
/// identifier is written once, at the call site, and the string comes from the
/// compiler.
fn proof<F: Fn() + 'static>(tree: &'static str, operator: &'static str, run: F) -> Proof {
    let path = std::any::type_name_of_val(&run);
    Proof {
        tree,
        operator,
        name: path.rsplit("::").next().unwrap_or(path),
        run: Box::new(run),
    }
}

/// The proof for each `have` verdict this crate answers for.
///
/// Split by reference only because one list of sixty-one is past the length
/// this project lets a function have; the two are one table.
fn proofs() -> Vec<Proof> {
    let mut all = blender_proofs();
    all.extend(unreal_proofs());
    all
}

fn blender_proofs() -> Vec<Proof> {
    vec![
        proof(
            "blender",
            "NODE_OT_add_empty_group",
            blender_add_empty_group,
        ),
        proof("blender", "NODE_OT_add_group", blender_add_group),
        proof(
            "blender",
            "NODE_OT_add_group_input_node",
            blender_add_group_input_node,
        ),
        proof("blender", "NODE_OT_attach", blender_attach),
        proof("blender", "NODE_OT_clipboard_copy", blender_clipboard_copy),
        proof(
            "blender",
            "NODE_OT_clipboard_paste",
            blender_clipboard_paste,
        ),
        proof(
            "blender",
            "NODE_OT_collapse_hide_unused_toggle",
            blender_collapse_hide_unused_toggle,
        ),
        proof("blender", "NODE_OT_delete", blender_delete),
        proof(
            "blender",
            "NODE_OT_delete_reconnect",
            blender_delete_reconnect,
        ),
        proof("blender", "NODE_OT_detach", blender_detach),
        proof("blender", "NODE_OT_duplicate", blender_duplicate),
        proof("blender", "NODE_OT_group_edit", blender_group_edit),
        proof(
            "blender",
            "NODE_OT_group_enter_exit",
            blender_group_enter_exit,
        ),
        proof("blender", "NODE_OT_group_insert", blender_group_insert),
        proof("blender", "NODE_OT_group_make", blender_group_make),
        proof("blender", "NODE_OT_group_separate", blender_group_separate),
        proof("blender", "NODE_OT_group_ungroup", blender_group_ungroup),
        proof("blender", "NODE_OT_hide_toggle", blender_hide_toggle),
        proof(
            "blender",
            "NODE_OT_interface_item_duplicate",
            blender_interface_item_duplicate,
        ),
        proof(
            "blender",
            "NODE_OT_interface_item_new",
            blender_interface_item_new,
        ),
        proof(
            "blender",
            "NODE_OT_interface_item_remove",
            blender_interface_item_remove,
        ),
        proof("blender", "NODE_OT_join", blender_join),
        proof("blender", "NODE_OT_join_nodes", blender_join_nodes),
        proof("blender", "NODE_OT_link", blender_link),
        proof("blender", "NODE_OT_link_make", blender_link_make),
        proof("blender", "NODE_OT_links_cut", blender_links_cut),
        proof("blender", "NODE_OT_links_detach", blender_links_detach),
        proof("blender", "NODE_OT_links_mute", blender_links_mute),
        proof("blender", "NODE_OT_mute_toggle", blender_mute_toggle),
        proof("blender", "NODE_OT_new_node_tree", blender_new_node_tree),
        proof("blender", "NODE_OT_options_toggle", blender_options_toggle),
        proof("blender", "NODE_OT_parent_set", blender_parent_set),
        proof("blender", "NODE_OT_preview_toggle", blender_preview_toggle),
        proof("blender", "NODE_OT_resize", blender_resize),
        proof("blender", "NODE_OT_select_grouped", blender_select_grouped),
        proof(
            "blender",
            "NODE_OT_select_linked_from",
            blender_select_linked_from,
        ),
        proof(
            "blender",
            "NODE_OT_select_linked_to",
            blender_select_linked_to,
        ),
        proof(
            "blender",
            "NODE_OT_select_same_type_step",
            blender_select_same_type_step,
        ),
        proof("blender", "NODE_OT_sockets_sync", blender_sockets_sync),
        proof("blender", "NODE_OT_swap_node", blender_swap_node),
        proof(
            "blender",
            "NODE_OT_tree_path_parent",
            blender_tree_path_parent,
        ),
    ]
}

fn unreal_proofs() -> Vec<Proof> {
    vec![
        proof("unreal", "BreakNodeLinks", unreal_break_node_links),
        proof("unreal", "BreakPinLinks", unreal_break_pin_links),
        proof("unreal", "BreakThisLink", unreal_break_this_link),
        proof("unreal", "CollapseNodes", unreal_collapse_nodes),
        proof(
            "unreal",
            "CollapseSelectionToFunction",
            unreal_collapse_selection_to_function,
        ),
        proof(
            "unreal",
            "CollapseSelectionToMacro",
            unreal_collapse_selection_to_macro,
        ),
        proof("unreal", "CreateComment", unreal_create_comment),
        proof(
            "unreal",
            "DeleteAndReconnectNodes",
            unreal_delete_and_reconnect_nodes,
        ),
        proof("unreal", "DisableNodes", unreal_disable_nodes),
        proof("unreal", "EnableNodes", unreal_enable_nodes),
        proof("unreal", "ExpandNodes", unreal_expand_nodes),
        proof(
            "unreal",
            "HideNoConnectionNoDefaultPins",
            unreal_hide_no_connection_no_default_pins,
        ),
        proof(
            "unreal",
            "HideNoConnectionPins",
            unreal_hide_no_connection_pins,
        ),
        proof(
            "unreal",
            "PromoteSelectionToFunction",
            unreal_promote_selection_to_function,
        ),
        proof(
            "unreal",
            "PromoteSelectionToMacro",
            unreal_promote_selection_to_macro,
        ),
        proof("unreal", "ReconstructNodes", unreal_reconstruct_nodes),
        proof(
            "unreal",
            "ResetPinToDefaultValue",
            unreal_reset_pin_to_default_value,
        ),
        proof(
            "unreal",
            "SelectAllInputNodes",
            unreal_select_all_input_nodes,
        ),
        proof(
            "unreal",
            "SelectAllOutputNodes",
            unreal_select_all_output_nodes,
        ),
        proof("unreal", "ShowAllPins", unreal_show_all_pins),
    ]
}
/// The proof name an operator must carry, so the two are one decision.
///
/// Blender's operators are already snake case under a fixed prefix; Unreal's
/// are Pascal case, so the same rule reads them by case boundary.
fn proof_name(tree: &str, operator: &str) -> String {
    let stem = if tree == "blender" {
        operator.trim_start_matches("NODE_OT_").to_owned()
    } else {
        let mut out = String::new();
        for (index, character) in operator.char_indices() {
            if character.is_ascii_uppercase() && index > 0 {
                out.push('_');
            }
            out.push(character.to_ascii_lowercase());
        }
        out
    };
    format!("{tree}_{stem}")
}

/// The pin's `have` rows addressed to this crate, and the proofs in this file,
/// are the same set — and each row names the proof that runs it.
///
/// This is the check that makes a verdict cost something. A `have` added to the
/// pin with no proof fails here; a proof deleted or renamed fails here; and a
/// row whose `proven_by` names a different capability than the operator it sits
/// under fails here, because the name is *derived* rather than transcribed.
#[test]
fn the_pin_and_the_proofs_are_in_bijection() {
    let pin: BTreeMap<String, BTreeMap<String, Row>> =
        serde_json::from_str(PIN).expect("the census pin parses");

    let mut claimed: BTreeMap<(String, String), String> = BTreeMap::new();
    for (tree, rows) in &pin {
        for (operator, row) in rows {
            if row.verdict != "have" {
                assert!(
                    row.proven_by.is_empty(),
                    "{tree}/{operator} is {:?} and still names a proof — a verdict \
                     that is out of the numerator must not carry evidence for one",
                    row.verdict
                );
                continue;
            }
            let (crate_name, proof) = row
                .proven_by
                .split_once("::")
                .unwrap_or_else(|| panic!("{tree}/{operator}: proven_by is not <crate>::<test>"));
            if crate_name == CRATE {
                claimed.insert((tree.clone(), operator.clone()), proof.to_owned());
            }
        }
    }

    let table = proofs();
    let mine: BTreeSet<(String, String)> = table
        .iter()
        .map(|entry| (entry.tree.to_owned(), entry.operator.to_owned()))
        .collect();
    let pinned: BTreeSet<(String, String)> = claimed.keys().cloned().collect();

    let unproven: Vec<_> = pinned.difference(&mine).collect();
    assert!(
        unproven.is_empty(),
        "the pin claims {CRATE} proves these and this file does not: {unproven:?}"
    );
    let unclaimed: Vec<_> = mine.difference(&pinned).collect();
    assert!(
        unclaimed.is_empty(),
        "this file proves these and the pin does not say so: {unclaimed:?}"
    );

    for entry in &table {
        let key = (entry.tree.to_owned(), entry.operator.to_owned());
        assert_eq!(
            entry.name,
            proof_name(entry.tree, entry.operator),
            "{}/{} is proven by a function whose name is not derived from it",
            entry.tree,
            entry.operator
        );
        assert_eq!(
            claimed[&key], entry.name,
            "{}/{} names a proof the compiler says is called something else",
            entry.tree, entry.operator
        );
    }
    assert_eq!(table.len(), claimed.len());
}

/// Every proof in the table runs.
///
/// The individual `#[test]` attributes already run them one by one, which is
/// what names a failure. This asserts the *table* is live: an entry whose
/// function was quietly replaced by a stub still has to do the work.
#[test]
fn every_proof_in_the_table_runs() {
    for entry in proofs() {
        (entry.run)();
    }
}

// ==================================================== groups and definitions

/// Blender adds an empty group and drops you inside it. The two halves here are
/// a definition with nothing in it and an instance standing for it — and the
/// instance's signature is **derived**, so an empty definition gives an
/// instance with no ports at all rather than a node with unresolved sockets.
#[test]
fn blender_add_empty_group() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("Empty");
    let instance = document.instantiate(ROOT, definition, 0, 0).unwrap();

    let signature = document.signature(ROOT, instance).unwrap();
    assert!(signature.inputs.is_empty() && signature.outputs.is_empty());
    assert_eq!(document.tree(definition).unwrap().node_count(), 0);
    assert!(document.evaluate(ROOT, instance).is_empty());
    assert!(document.validate().is_empty());
}

/// A second instance of a definition that already exists. What makes this more
/// than a copy is that the two instances share the definition and **not** the
/// value: fed differently they answer differently, which is the memo being
/// keyed by instance.
#[test]
fn blender_add_group() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    assert_eq!(chain.document.instance_count(made.definition), 1);

    let again = chain
        .document
        .instantiate(ROOT, made.definition, 0, 300)
        .unwrap();
    assert_eq!(chain.document.instance_count(made.definition), 2);

    let ten = num(&mut chain.document, 10);
    wire(&mut chain.document, ten, 0, again, 0);
    wire(&mut chain.document, ten, 0, again, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, again),
        vec![Some(Val::Number(20))]
    );
    assert_eq!(
        chain.document.evaluate(ROOT, made.node),
        vec![Some(Val::Number(5))],
        "the first instance is undisturbed by the second"
    );
}

/// Blender's Group Input node. The interface is the definition's, and the node
/// is how the graph *inside* reaches it — so an `Input` interface node's ports
/// are OUTPUTS, which is the part that is easy to get backwards.
#[test]
fn blender_add_group_input_node() {
    let mut document: Document<Op> = Document::new("root");
    let definition = document.add_definition("Def");
    let index = document
        .expose(
            definition,
            InterfaceSide::Input,
            Port::new("Seed", Ty::Number),
        )
        .unwrap();
    assert_eq!(index, 0);

    let inside = document
        .add_node(definition, NodeBody::Interface(InterfaceSide::Input), 0, 0)
        .unwrap();
    assert_eq!(
        document
            .tree(definition)
            .unwrap()
            .interface_node(InterfaceSide::Input)
            .map(|n| n.id),
        Some(inside)
    );
    let signature = document.signature(definition, inside).unwrap();
    assert!(signature.inputs.is_empty());
    assert_eq!(signature.outputs.len(), 1);
    assert_eq!(signature.outputs[0].name, "Seed");
}

/// Collapse a selection into a re-usable definition. The interface is not
/// authored: it is derived from what crossed the boundary, and the graph goes
/// on computing what it computed.
#[test]
fn blender_group_make() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    assert_eq!(before, Some(Val::Number(5)));
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let definition = chain.document.tree(made.definition).unwrap();
    assert_eq!(definition.interface().inputs().len(), 2);
    assert_eq!(definition.interface().outputs().len(), 1);
    assert!(chain.document.tree(ROOT).unwrap().node(chain.add).is_none());
    assert_eq!(arrives(&chain.document, Socket::new(chain.sink, 0)), before);
    assert!(chain.document.validate().is_empty());
}

/// Inline a group back. The nodes come out, the definition is reported as
/// having no instances left, and the value is unchanged.
#[test]
fn blender_group_ungroup() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let out = chain.document.ungroup(ROOT, made.node).unwrap();
    assert_eq!(out.nodes.len(), 1);
    assert!(out.definition_unused);
    assert_eq!(arrives(&chain.document, Socket::new(chain.sink, 0)), before);
}

/// Move a node from the host INTO the group through its instance. Blender
/// leaves the interface alone; here it is re-derived, so the value that used to
/// cross keeps crossing and nothing is left describing a link that is gone.
#[test]
fn blender_group_insert() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let moved = chain
        .document
        .group_insert(ROOT, made.node, &[chain.two], Sharing::Shared)
        .unwrap();
    assert_eq!(
        moved.moved.len(),
        1,
        "the node is named by the id it has in the definition, which is another tree"
    );
    assert!(chain.document.tree(ROOT).unwrap().node(chain.two).is_none());
    assert_eq!(
        chain
            .document
            .tree(made.definition)
            .unwrap()
            .interface()
            .inputs()
            .len(),
        1,
        "the port `two` used to feed is gone; the one `three` feeds remains"
    );
    assert_eq!(arrives(&chain.document, Socket::new(chain.sink, 0)), before);
}

/// The other direction: move a node out of the definition into the host. The
/// value that used to cross the boundary is **reconnected**, which is where
/// Blender loses it.
#[test]
fn blender_group_separate() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let made = chain
        .document
        .group(ROOT, &[chain.two, chain.add], "Sum")
        .unwrap();

    let inside: Vec<NodeId> = chain
        .document
        .tree(made.definition)
        .unwrap()
        .nodes()
        .filter(|n| matches!(&n.body, NodeBody::Kind(Op::Num(_))))
        .map(|n| n.id)
        .collect();
    assert_eq!(inside.len(), 1);

    let moved = chain
        .document
        .group_separate(ROOT, made.node, &inside, Sharing::Shared)
        .unwrap();
    assert_eq!(moved.moved.len(), 1);
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        before,
        "the value that used to cross the boundary was reconnected"
    );
}

/// A definition is a tree of its own, addable without any node being collapsed
/// into it. Blender's "New Node Tree".
#[test]
fn blender_new_node_tree() {
    let mut document: Document<Op> = Document::new("root");
    let before = document.tree_count();
    let definition = document.add_definition("Fresh");

    assert_eq!(document.tree_count(), before + 1);
    assert!(document.definitions().any(|t| t.id == definition));
    assert_eq!(document.instance_count(definition), 0);
    assert!(document.tree(definition).unwrap().interface().is_empty());
}

/// Descend into a definition and come back. The path is a value, so "where am
/// I" is answerable without the editor keeping its own copy.
#[test]
fn blender_group_edit() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let mut path = EditPath::root();
    assert_eq!(path.current(), ROOT);
    let entered = path.enter(&chain.document, made.node).unwrap();
    assert_eq!(entered, made.definition);
    assert_eq!(path.current(), made.definition);
    assert_eq!(path.depth(), 1);
    assert_eq!(path.breadcrumb(&chain.document).len(), 2);
}

/// The same pair used as Blender's toggle: entering and leaving returns the
/// editor exactly where it was, which is the property a toggle needs and a
/// remembered tree id cannot promise.
#[test]
fn blender_group_enter_exit() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();

    let mut path = EditPath::root();
    path.enter(&chain.document, made.node).unwrap();
    let back = path.exit().unwrap();
    assert_eq!(back, ROOT);
    assert_eq!(path.current(), ROOT);
    assert_eq!(path.depth(), 0);
    assert!(path.exit().is_err(), "there is nothing above the root");
}

/// Blender's "go to parent tree" — the same exit, reached from a nesting two
/// deep so that "parent" is not a synonym for "root".
#[test]
fn blender_tree_path_parent() {
    let mut chain = chain();
    let inner = chain.document.group(ROOT, &[chain.add], "Inner").unwrap();
    let outer = chain.document.group(ROOT, &[inner.node], "Outer").unwrap();

    let mut path = EditPath::root();
    path.enter(&chain.document, outer.node).unwrap();
    let inner_instance = chain
        .document
        .tree(outer.definition)
        .unwrap()
        .nodes()
        .find(|n| matches!(n.body, NodeBody::Group(_)))
        .map(|n| n.id)
        .unwrap();
    path.enter(&chain.document, inner_instance).unwrap();
    assert_eq!(path.depth(), 2);

    assert_eq!(path.exit().unwrap(), outer.definition);
    assert_eq!(path.depth(), 1);
}

// ============================================================== the interface

/// Add a port to a definition's interface. Every instance gains the socket at
/// once, because an instance's signature IS the interface.
#[test]
fn blender_interface_item_new() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let before = chain
        .document
        .signature(ROOT, made.node)
        .unwrap()
        .inputs
        .len();

    let index = chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Extra", Ty::Number),
        )
        .unwrap();
    assert_eq!(index as usize, before);
    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        before + 1
    );
}

/// Remove one. This is the direction that can invalidate indices, so the links
/// that had to go are **named with the tree they were in** — including the ones
/// at instances, which live in another tree entirely.
#[test]
fn blender_interface_item_remove() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let arity = chain
        .document
        .signature(ROOT, made.node)
        .unwrap()
        .inputs
        .len();

    let dropped = chain
        .document
        .unexpose(made.definition, InterfaceSide::Input, 0)
        .unwrap();
    assert!(
        dropped.iter().any(|d| d.tree == ROOT),
        "a link at the instance is in the ROOT tree, not the definition's"
    );
    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        arity - 1
    );
    assert!(chain.document.validate().is_empty());
}

/// ★ A **composition claim**, which is why it is worth a test: the pin says
/// duplicating an interface item is `expose` called again with the same port.
/// Running it is what shows the claim holds — the copy is a distinct port with
/// its own index, the original's wiring is untouched, and every instance grows.
#[test]
fn blender_interface_item_duplicate() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let original = chain
        .document
        .tree(made.definition)
        .unwrap()
        .interface()
        .inputs()[0]
        .clone();
    let wired = chain
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(made.node, 0))
        .is_some();
    assert!(wired);

    let copy = chain
        .document
        .expose(made.definition, InterfaceSide::Input, original.clone())
        .unwrap();

    let interface = chain.document.tree(made.definition).unwrap().interface();
    assert_ne!(copy, 0);
    assert_eq!(interface.inputs()[copy as usize].name, original.name);
    assert_eq!(
        interface.inputs()[copy as usize].value_type(),
        original.value_type()
    );
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(made.node, 0))
            .is_some(),
        "duplicating a port does not disturb what the original one carries"
    );
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(made.node, copy))
            .is_none(),
        "and the copy arrives unwired"
    );
}

/// ★ The pin's reason for this one is that there is **nothing to sync** — the
/// signature is derived rather than stored. That is a claim about a mechanism
/// that does not exist, so the only honest proof is to change the interface and
/// observe the instance follow with no call in between.
#[test]
fn blender_sockets_sync() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let inside = chain
        .document
        .tree(made.definition)
        .unwrap()
        .interface_node(InterfaceSide::Input)
        .map(|n| n.id)
        .unwrap();

    let before_instance = chain
        .document
        .signature(ROOT, made.node)
        .unwrap()
        .inputs
        .len();
    let before_inside = chain
        .document
        .signature(made.definition, inside)
        .unwrap()
        .outputs
        .len();

    chain
        .document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Late", Ty::Number),
        )
        .unwrap();

    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        before_instance + 1,
        "the instance answered with the new arity without being told"
    );
    assert_eq!(
        chain
            .document
            .signature(made.definition, inside)
            .unwrap()
            .outputs
            .len(),
        before_inside + 1,
        "and so did the interface node inside the definition"
    );
}

// ================================================================ structure

/// Wire two sockets. A link that could not carry a value is refused rather than
/// drawn, and the refusal names why.
#[test]
fn blender_link() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let double = node(&mut document, Op::Double);
    document
        .connect(ROOT, Socket::new(two, 0), Socket::new(double, 0))
        .unwrap();
    assert_eq!(document.evaluate(ROOT, double), vec![Some(Val::Number(4))]);

    let word = node(&mut document, Op::Word("hi".into()));
    let refused = document.connect(ROOT, Socket::new(word, 0), Socket::new(double, 0));
    assert!(refused.is_err(), "Text does not cross into a Number socket");
}

/// The same verb read as Blender's "Make Links": a value input takes exactly
/// one producer, so a second wire onto it **displaces** the first and says so,
/// rather than leaving the node with two feeds.
#[test]
fn blender_link_make() {
    let mut chain = chain();
    let ten = num(&mut chain.document, 10);

    let second = chain
        .document
        .connect(ROOT, Socket::new(ten, 0), Socket::new(chain.add, 0))
        .unwrap();
    let displaced = second.displaced.expect("the first wire was named");
    assert_eq!(displaced.from.node, chain.two);
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(13))]
    );
}

/// ★ A **composition claim**: Blender's link-cut drags a stroke across the
/// canvas and removes every wire it crossed. What an application has is the set
/// of ids that stroke named, so the proof is that cutting a *set* is a loop over
/// `disconnect` and that each cut hands back the link it removed — which is what
/// an undo needs and what Blender's operator does not return.
#[test]
fn blender_links_cut() {
    let mut chain = chain();
    let crossed: Vec<_> = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.to.node == chain.add)
        .map(|link| link.id)
        .collect();
    assert_eq!(crossed.len(), 2);

    let mut removed = Vec::new();
    for link in crossed {
        removed.push(chain.document.disconnect(ROOT, link).unwrap());
    }
    assert_eq!(removed.len(), 2);
    assert_eq!(chain.document.tree(ROOT).unwrap().links().len(), 1);

    for link in removed {
        chain.document.connect(ROOT, link.from, link.to).unwrap();
    }
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        Some(Val::Number(5)),
        "each cut handed back what it removed, so the cut is undoable"
    );
}

/// Blender's "Detach Links": the node stays, its wires do not, and what the
/// graph loses is reported rather than discovered.
#[test]
fn blender_links_detach() {
    let mut chain = chain();
    let rewired = chain.document.detach(ROOT, chain.add).unwrap();

    assert!(chain.document.tree(ROOT).unwrap().node(chain.add).is_some());
    assert!(links_touching(&chain.document, ROOT, chain.add).is_empty());
    assert_eq!(rewired.removed.len() + rewired.severed.len(), 3);
}

/// A muted link stops the value and keeps the wire. It is a different word from
/// a bypassed node because it is the opposite behaviour, and Blender spells both
/// "mute".
#[test]
fn blender_links_mute() {
    let mut chain = chain();
    let link = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .find(|l| l.from.node == chain.two)
        .map(|l| l.id)
        .unwrap();

    assert!(!chain.document.set_link_muted(ROOT, link, true).unwrap());
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(3))],
        "the muted feed falls back to the port's declared default of 0"
    );
    assert_eq!(
        chain.document.tree(ROOT).unwrap().links().len(),
        3,
        "and the wire is still there to be drawn"
    );
}

/// Delete. The links that had to go with the node are named, so an editor never
/// has to scan for what it just broke.
#[test]
fn blender_delete() {
    let mut chain = chain();
    let removed = chain.document.remove_node(ROOT, chain.add).unwrap();

    assert_eq!(removed.links.len(), 3);
    assert!(chain.document.tree(ROOT).unwrap().node(chain.add).is_none());
    assert!(chain.document.validate().is_empty());
}

/// Delete and reconnect. Blender's own description is "remove nodes and
/// reconnect nodes **as if deletion was muted**", so the reconnection is the
/// bypass derivation applied to the structure — one rule, not two.
#[test]
fn blender_delete_reconnect() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let double = node(&mut document, Op::Double);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, double, 0);
    wire(&mut document, double, 0, sink, 0);

    let rewired = document.dissolve(ROOT, double).unwrap();
    assert_eq!(rewired.bridged.len(), 1);
    assert!(document.tree(ROOT).unwrap().node(double).is_none());
    assert_eq!(document.evaluate(ROOT, sink), Vec::new());
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(sink, 0))
            .map(|l| l.from.node),
        Some(two),
        "the upstream took the deleted node's place"
    );
}

/// Bypass: the node stays and the values pass through it. The route is derived
/// from the signature alone, so unplugging a different port cannot change which
/// value leaves by which output — the property Blender's wiring-sensitive
/// scoring does not have.
#[test]
fn blender_mute_toggle() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let double = node(&mut document, Op::Double);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, double, 0);
    wire(&mut document, double, 0, sink, 0);
    assert_eq!(document.evaluate(ROOT, double), vec![Some(Val::Number(4))]);

    assert!(!document.set_bypassed(ROOT, double, true).unwrap());
    let route = document.passthrough(ROOT, double).unwrap();
    assert!(route.is_identity());
    assert!(route.dropped_outputs().is_empty());
    assert_eq!(
        document.evaluate(ROOT, double),
        vec![Some(Val::Number(2))],
        "the input arrives at the output unchanged"
    );
    assert!(document.tree(ROOT).unwrap().node(double).is_some());
}

/// Change what a node IS without changing which node it is. Blender creates a
/// new node and deletes the old one, so every reference to it dies; here the id,
/// the position and the frame membership all survive and what did not fit is
/// reported.
#[test]
fn blender_swap_node() {
    let mut chain = chain();
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .label = Some("keep me".into());

    let swapped = chain.document.set_kind(ROOT, chain.add, Op::Mul).unwrap();

    let after = chain.document.tree(ROOT).unwrap().node(chain.add).unwrap();
    assert_eq!(after.id, chain.add);
    assert_eq!(after.label.as_deref(), Some("keep me"));
    assert!(
        swapped
            .carried
            .iter()
            .any(|c| c.by_name && c.from == PortRef::input(0)),
        "`Augend` is on both kinds, so the author's own name carried the wire"
    );
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(6))]
    );
}

// ================================================================ appearance

/// Hide the ports nothing is wired to. The answer is a *derivation* over the
/// declaration and the wiring together, which only the document can make.
#[test]
fn blender_collapse_hide_unused_toggle() {
    let mut chain = chain();
    let ports = chain.document.visible_ports(ROOT, chain.add).unwrap();
    assert_eq!(ports.hidden_count(), 0);

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .appearance
        .hide_unused_ports = true;

    let ports = chain.document.visible_ports(ROOT, chain.add).unwrap();
    assert_eq!(
        ports.inputs,
        vec![0, 1],
        "both inputs are wired, so both stay"
    );

    let lonely = node(&mut chain.document, Op::Add);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .hide_unused_ports = true;
    let ports = chain.document.visible_ports(ROOT, lonely).unwrap();
    assert!(ports.inputs.is_empty());
    assert_eq!(ports.hidden_inputs, vec![0, 1]);
}

/// Collapse: drawn small, and the same request about unused ports. Two
/// booleans rather than one state, so un-collapsing restores what the node was
/// already saying instead of a default.
#[test]
fn blender_hide_toggle() {
    let mut chain = chain();
    let lonely = node(&mut chain.document, Op::Add);
    {
        let looks = &mut chain
            .document
            .tree_mut(ROOT)
            .and_then(|t| t.node_mut(lonely))
            .unwrap()
            .appearance;
        looks.hide_unused_ports = true;
        looks.collapsed = true;
    }
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .hidden_count(),
        3
    );

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .collapsed = false;
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .hidden_count(),
        3,
        "un-collapsing restored the node's own answer about unused ports"
    );
}

/// Whether the node's own controls are drawn. What a control *is* belongs to
/// the application; whether it is on screen travels with the node — and the
/// evaluator may not read it.
#[test]
fn blender_options_toggle() {
    let mut chain = chain();
    let before = chain.document.evaluate(ROOT, chain.add);
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance
            .show_options
    );

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .appearance
        .show_options = false;

    assert!(
        !chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance
            .show_options
    );
    assert_eq!(chain.document.evaluate(ROOT, chain.add), before);
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, chain.add)
            .unwrap()
            .hidden_count(),
        0
    );
}

/// Whether the node's preview is drawn. Same axis, separate memory.
#[test]
fn blender_preview_toggle() {
    let mut chain = chain();
    assert!(
        !chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance
            .show_preview
    );
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(chain.add))
        .unwrap()
        .appearance
        .show_preview = true;

    let looks = &chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(chain.add)
        .unwrap()
        .appearance;
    assert!(looks.show_preview);
    assert!(looks.show_options, "the two are independent");
}

/// An authored size. A width is `Option` because an application has its own
/// default; a **height** is `Option` for a different reason — an ordinary node's
/// height is a function of its ports and a frame's is not.
#[test]
fn blender_resize() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;
    {
        let looks = &mut chain
            .document
            .tree_mut(ROOT)
            .and_then(|t| t.node_mut(frame))
            .unwrap()
            .appearance;
        looks.width = Some(320);
        looks.height = Some(200);
    }
    let looks = &chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(frame)
        .unwrap()
        .appearance;
    assert_eq!((looks.width, looks.height), (Some(320), Some(200)));
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance,
        Appearance::default(),
        "an ordinary node derives its height and is left alone"
    );
}

// ==================================================================== frames

/// Fence a selection into a frame. The frame is a node, so it can be moved,
/// saved and asked about like any other — and it has no ports, so nothing links
/// to it and evaluation never reaches it.
#[test]
fn blender_join() {
    let mut chain = chain();
    let enframed = chain
        .document
        .enframe(ROOT, &[chain.two, chain.add], Some("decode".into()))
        .unwrap();

    assert_eq!(enframed.members.len(), 2);
    assert_eq!(chain.document.members(ROOT, enframed.frame).len(), 2);
    let frame = chain
        .document
        .tree(ROOT)
        .unwrap()
        .node(enframed.frame)
        .unwrap();
    assert!(frame.is_frame());
    assert_eq!(frame.label.as_deref(), Some("decode"));
    let signature = chain.document.signature(ROOT, enframed.frame).unwrap();
    assert!(signature.inputs.is_empty() && signature.outputs.is_empty());
}

/// The same verb applied inside an existing frame. The new frame lands **inside
/// whatever already contained all of the selection**, so framing part of a
/// pipeline does not lift that part out of the pipeline — and the nodes that
/// were not selected stay where they were.
#[test]
fn blender_join_nodes() {
    let mut chain = chain();
    let outer = chain
        .document
        .enframe(
            ROOT,
            &[chain.two, chain.three, chain.add],
            Some("pipeline".into()),
        )
        .unwrap();

    let inner = chain
        .document
        .enframe(ROOT, &[chain.two, chain.three], Some("sources".into()))
        .unwrap();

    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(inner.frame)
            .unwrap()
            .parent,
        Some(outer.frame),
        "the new frame joined the frame that already held all of the selection"
    );
    assert_eq!(
        chain.document.ancestry(ROOT, chain.two),
        vec![outer.frame, inner.frame],
        "the containment chain reads outermost first, the way a breadcrumb does"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .parent,
        Some(outer.frame),
        "and the member that was not selected did not move"
    );
}

/// Attach a node to a frame directly.
#[test]
fn blender_attach() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;

    let previous = chain
        .document
        .set_parent(ROOT, chain.two, Some(frame))
        .unwrap();
    assert_eq!(previous, None);
    assert!(chain.document.members(ROOT, frame).contains(&chain.two));
}

/// Detach one. Blender's operator clears the parent outright, so only the
/// all-the-way form is reachable there; it is reachable here too, and the node
/// lands on the canvas rather than in limbo.
#[test]
fn blender_detach() {
    let mut chain = chain();
    let frame = chain
        .document
        .enframe(ROOT, &[chain.add, chain.two], None)
        .unwrap()
        .frame;

    let left = chain.document.unframe(ROOT, &[chain.two]).unwrap();
    assert_eq!(left, vec![chain.two]);
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.two)
            .unwrap()
            .parent,
        None
    );
    assert_eq!(chain.document.members(ROOT, frame), vec![chain.add]);

    assert_eq!(
        chain.document.set_parent(ROOT, chain.add, None).unwrap(),
        Some(frame)
    );
    assert!(chain.document.members(ROOT, frame).is_empty());
}

/// Set a node's parent, and the two rules that makes it a forest: a container
/// must be a frame, and nothing may contain itself. Blender states both as
/// assertions its shipped build compiles out, and its own operator detaches
/// before it attaches so the cycle guard cannot fire even in a debug build.
#[test]
fn blender_parent_set() {
    let mut chain = chain();
    let outer = chain
        .document
        .enframe(ROOT, &[chain.add], None)
        .unwrap()
        .frame;
    let inner = chain
        .document
        .enframe(ROOT, &[chain.two], None)
        .unwrap()
        .frame;
    chain.document.set_parent(ROOT, inner, Some(outer)).unwrap();

    assert!(
        chain
            .document
            .set_parent(ROOT, chain.three, Some(chain.add))
            .is_err(),
        "an ordinary node is not a container"
    );
    let cycle = chain.document.set_parent(ROOT, outer, Some(inner));
    assert!(
        cycle.is_err(),
        "and a frame may not contain its own container"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(inner)
            .unwrap()
            .parent,
        Some(outer),
        "a refused parenting leaves the forest untouched"
    );
}

// ============================================================ fragments

/// Copy. A fragment is a **value**: it carries the nodes, the definitions they
/// depend on and the boundary it was cut from, and it serializes — which is what
/// a clipboard between two documents actually needs.
#[test]
fn blender_clipboard_copy() {
    let chain = chain();
    let fragment = chain.document.extract(ROOT, &[chain.add]).unwrap();

    assert_eq!(fragment.node_count(), 1);
    assert_eq!(fragment.inbound().len(), 2, "the two feeds it was cut from");
    assert_eq!(fragment.outbound().len(), 1);
    assert_eq!(fragment.source_tree(), ROOT);

    let wire_form = serde_json::to_string(&fragment).unwrap();
    let back: Fragment<Op> = serde_json::from_str(&wire_form).unwrap();
    assert_eq!(back.node_count(), 1);
    assert!(back.validate().is_empty());
}

/// Paste. The fragment goes anywhere, re-using the definitions that are already
/// there rather than forking them.
#[test]
fn blender_clipboard_paste() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let fragment = chain.document.extract(ROOT, &[made.node]).unwrap();

    let pasted = chain
        .document
        .insert(
            ROOT,
            &fragment,
            (900, 900),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(pasted.nodes.len(), 1);
    assert!(pasted.definitions_added.is_empty());
    assert_eq!(pasted.definitions_reused, vec![made.definition]);
    assert_eq!(chain.document.instance_count(made.definition), 2);

    let landed = pasted.nodes[0];
    assert_ne!(landed, made.node);
    assert_eq!(
        (
            chain.document.tree(ROOT).unwrap().node(landed).unwrap().x,
            chain.document.tree(ROOT).unwrap().node(landed).unwrap().y
        ),
        (900, 900)
    );
}

/// Duplicate — extract and insert in one call, offset from the original.
#[test]
fn blender_duplicate() {
    let mut chain = chain();
    let copy = chain
        .document
        .duplicate(
            ROOT,
            &[chain.two, chain.add],
            (40, 40),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();

    assert_eq!(copy.nodes.len(), 2);
    assert!(
        copy.nodes
            .iter()
            .all(|n| *n != chain.two && *n != chain.add)
    );
    assert_eq!(
        copy.links.len(),
        1,
        "the wire inside the selection came along"
    );
    assert!(chain.document.validate().is_empty());
}

// ============================================================== selection

/// What feeds this. Blender walks one hop per keypress with nothing telling you
/// when the picture has stopped changing; the reach is a parameter here, and
/// `added` is what says the transitive walk is done.
#[test]
fn blender_select_linked_from() {
    let chain = chain();
    let direct = chain
        .document
        .grow(ROOT, &[chain.sink], Grow::Upstream(Reach::Direct))
        .unwrap();
    assert_eq!(direct.added, vec![chain.add]);

    let all = chain
        .document
        .grow(ROOT, &[chain.sink], Grow::Upstream(Reach::Transitive))
        .unwrap();
    assert_eq!(all.added, vec![chain.two, chain.three, chain.add]);
    let again = chain
        .document
        .grow(ROOT, &all.selection, Grow::Upstream(Reach::Transitive))
        .unwrap();
    assert!(
        !again.changed(),
        "the walk reports that it has reached the end"
    );
}

/// What this feeds — the other direction of the same relation, read the same
/// way.
#[test]
fn blender_select_linked_to() {
    let chain = chain();
    let all = chain
        .document
        .grow(ROOT, &[chain.two], Grow::Downstream(Reach::Transitive))
        .unwrap();
    assert_eq!(all.added, vec![chain.add, chain.sink]);
    assert!(
        !chain
            .document
            .grow(ROOT, &[chain.two], Grow::Downstream(Reach::Direct))
            .unwrap()
            .added
            .contains(&chain.sink),
        "one hop stops at the adder"
    );
}

/// Blender's "select grouped by type". Keyed on the whole selection rather than
/// on an active node, because a selection belongs to the editor and this crate
/// has no notion of which of them is active.
#[test]
fn blender_select_grouped() {
    let chain = chain();
    let same = chain
        .document
        .grow(ROOT, &[chain.two], Grow::SameKind)
        .unwrap();
    assert_eq!(same.added, vec![chain.three]);
    assert!(!same.selection.contains(&chain.add));
}

/// Blender steps the selection to the *next* node of the same kind. The run is
/// the answer that step walks, produced once.
#[test]
fn blender_select_same_type_step() {
    let mut chain = chain();
    let four = num(&mut chain.document, 4);
    let run = chain.document.same_kind_run(ROOT, chain.two).unwrap();

    assert_eq!(run, vec![chain.two, chain.three, four]);
    assert_eq!(
        chain.document.same_kind_run(ROOT, chain.sink).unwrap(),
        vec![chain.sink],
        "a kind with one member is a run of one, not an absence"
    );
}

// ============================================================ Unreal Engine

/// Break every link on a node, leaving the node. ★ A **composition claim**: the
/// crate's `disconnect` takes one link, so the proof is that an application can
/// name the set and that the node survives — which is the whole difference from
/// deleting it.
#[test]
fn unreal_break_node_links() {
    let mut chain = chain();
    let touching = links_touching(&chain.document, ROOT, chain.add);
    assert_eq!(touching.len(), 3);

    for link in touching {
        chain.document.disconnect(ROOT, link).unwrap();
    }
    assert!(links_touching(&chain.document, ROOT, chain.add).is_empty());
    assert!(chain.document.tree(ROOT).unwrap().node(chain.add).is_some());
    assert!(chain.document.validate().is_empty());
}

/// Break every link on one **pin**. The narrower reading of the same
/// composition, and the one that shows what a pin is here: a [`Socket`] is a
/// port index on a node and says nothing about which side it is, because the
/// side is decided by **which end of the link it sits on**. So an input pin's
/// links are `link_into`, an output pin's are the links leaving it, and
/// `Socket::new(add, 0)` names both — which is why a filter that ignores the
/// end matches twice.
#[test]
fn unreal_break_pin_links() {
    let mut chain = chain();
    let pin = Socket::new(chain.add, 0);
    let either_end = chain
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|link| link.to == pin || link.from == pin)
        .count();
    assert_eq!(
        either_end, 2,
        "input 0 and output 0 of one node are the same Socket value"
    );

    let incoming = chain
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(pin)
        .unwrap()
        .id;
    chain.document.disconnect(ROOT, incoming).unwrap();
    assert!(chain.document.tree(ROOT).unwrap().link_into(pin).is_none());
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(chain.add, 1))
            .is_some(),
        "the node's other input pin is untouched"
    );
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .links()
            .iter()
            .any(|l| l.from == pin),
        "and so is the output pin that shares the index"
    );
}

/// Break one named link, and hand it back so it can be put back.
#[test]
fn unreal_break_this_link() {
    let mut chain = chain();
    let id = chain.document.tree(ROOT).unwrap().links()[0].id;
    let removed = chain.document.disconnect(ROOT, id).unwrap();

    assert_eq!(removed.id, id);
    assert_eq!(chain.document.tree(ROOT).unwrap().links().len(), 2);
    chain
        .document
        .connect(ROOT, removed.from, removed.to)
        .unwrap();
    assert_eq!(chain.document.tree(ROOT).unwrap().links().len(), 3);
}

/// Collapse a selection into a subgraph node.
#[test]
fn unreal_collapse_nodes() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.add], "Collapsed")
        .unwrap();

    assert!(matches!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(made.node)
            .unwrap()
            .body,
        NodeBody::Group(_)
    ));
    assert_eq!(chain.document.evaluate(ROOT, chain.sink), Vec::new());
    assert_eq!(
        chain.document.evaluate(ROOT, made.node),
        vec![Some(Val::Number(5))]
    );
}

/// Unreal's "collapse to function": what separates a function from a one-off
/// subgraph is that it is **callable again**, so the proof instantiates it a
/// second time and checks the two occurrences do not share a value.
#[test]
fn unreal_collapse_selection_to_function() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.add], "Function")
        .unwrap();
    let second = chain
        .document
        .instantiate(ROOT, made.definition, 0, 400)
        .unwrap();

    let four = num(&mut chain.document, 4);
    wire(&mut chain.document, four, 0, second, 0);
    wire(&mut chain.document, four, 0, second, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, second),
        vec![Some(Val::Number(8))]
    );
    assert_eq!(
        chain.document.evaluate(ROOT, made.node),
        vec![Some(Val::Number(5))]
    );
    assert_eq!(chain.document.instance_count(made.definition), 2);
}

/// Unreal's "collapse to macro": a macro is the reading of the same boundary
/// that gets **expanded back into the caller**, so the proof is that the
/// collapse is reversible into the host with the value unchanged.
#[test]
fn unreal_collapse_selection_to_macro() {
    let mut chain = chain();
    let before = chain.document.evaluate(ROOT, chain.sink);
    let made = chain.document.group(ROOT, &[chain.add], "Macro").unwrap();

    let expanded = chain.document.ungroup(ROOT, made.node).unwrap();
    assert_eq!(expanded.nodes.len(), 1);
    assert_eq!(chain.document.evaluate(ROOT, chain.sink), before);
    assert!(chain.document.tree(ROOT).unwrap().node(made.node).is_none());
}

/// Unreal's comment box. Structurally a frame: it holds a region of canvas, its
/// members compute exactly as before, and the boundary means nothing to the
/// evaluator.
#[test]
fn unreal_create_comment() {
    let mut chain = chain();
    let before = arrives(&chain.document, Socket::new(chain.sink, 0));
    let comment = chain
        .document
        .enframe(ROOT, &[chain.two, chain.three], Some("inputs".into()))
        .unwrap();

    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(comment.frame)
            .unwrap()
            .is_frame()
    );
    assert_eq!(arrives(&chain.document, Socket::new(chain.sink, 0)), before);

    let moved = chain
        .document
        .translate(ROOT, comment.frame, 100, 0)
        .unwrap();
    assert_eq!(
        moved,
        vec![comment.frame, chain.two, chain.three],
        "dragging a comment carries what it contains, the frame named first"
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .x,
        0
    );
}

/// Unreal's delete-and-reconnect, over a selection rather than one node — the
/// reading that shows the derivation composes.
#[test]
fn unreal_delete_and_reconnect_nodes() {
    let mut document: Document<Op> = Document::new("root");
    let two = num(&mut document, 2);
    let first = node(&mut document, Op::Double);
    let second = node(&mut document, Op::Double);
    let sink = node(&mut document, Op::Sink);
    wire(&mut document, two, 0, first, 0);
    wire(&mut document, first, 0, second, 0);
    wire(&mut document, second, 0, sink, 0);

    for target in [first, second] {
        document.dissolve(ROOT, target).unwrap();
    }
    assert_eq!(
        document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(sink, 0))
            .map(|l| l.from.node),
        Some(two)
    );
    assert_eq!(document.tree(ROOT).unwrap().node_count(), 2);
}

/// Unreal's Disable Nodes. Its own semantics are the pass-through this crate
/// derives, and the outputs no input can feed are **named** rather than being
/// discovered as a missing wire.
#[test]
fn unreal_disable_nodes() {
    let mut chain = chain();
    assert!(!chain.document.set_bypassed(ROOT, chain.add, true).unwrap());
    assert!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .bypassed
    );

    let route = chain.document.passthrough(ROOT, chain.add).unwrap();
    assert_eq!(route.source_of(0), Some(0));
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(2))],
        "the first feed passes straight through"
    );
}

/// And back on. The flag is the graph's *meaning* rather than its looks, so it
/// is a field of the node and not of `Appearance`.
#[test]
fn unreal_enable_nodes() {
    let mut chain = chain();
    chain.document.set_bypassed(ROOT, chain.add, true).unwrap();
    assert!(chain.document.set_bypassed(ROOT, chain.add, false).unwrap());

    assert!(
        !chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .bypassed
    );
    assert_eq!(
        chain.document.evaluate(ROOT, chain.add),
        vec![Some(Val::Number(5))]
    );
    assert_eq!(
        chain
            .document
            .tree(ROOT)
            .unwrap()
            .node(chain.add)
            .unwrap()
            .appearance,
        Appearance::default(),
        "enabling a node changed nothing about how it is drawn"
    );
}

/// Unreal's Expand Node — the inverse of a collapse, back into the caller.
#[test]
fn unreal_expand_nodes() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.two, chain.add], "Sub")
        .unwrap();
    let out = chain.document.ungroup(ROOT, made.node).unwrap();

    assert_eq!(out.nodes.len(), 2);
    assert!(out.definition_unused);
    assert_eq!(
        arrives(&chain.document, Socket::new(chain.sink, 0)),
        Some(Val::Number(5))
    );
    assert_eq!(chain.document.tree(ROOT).unwrap().node_count(), 4);
}

/// Unreal hides unconnected pins.
#[test]
fn unreal_hide_no_connection_pins() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    wire(&mut chain.document, chain.two, 0, mul, 1);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(mul))
        .unwrap()
        .appearance
        .hide_unused_ports = true;

    let ports = chain.document.visible_ports(ROOT, mul).unwrap();
    assert_eq!(ports.inputs, vec![1]);
    assert_eq!(ports.hidden_inputs, vec![0]);
}

/// ★ Unreal's *other* hide command keeps a pin that has a **default**, because
/// a defaulted pin still carries a value the reader wants to see. A
/// **composition claim**: this crate publishes the hidden set and the signature
/// says which ports have defaults, so the narrower rule is a filter an
/// application writes — proven here by writing it.
#[test]
fn unreal_hide_no_connection_no_default_pins() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(mul))
        .unwrap()
        .appearance
        .hide_unused_ports = true;

    let ports = chain.document.visible_ports(ROOT, mul).unwrap();
    let signature = chain.document.signature(ROOT, mul).unwrap();
    assert_eq!(ports.hidden_inputs, vec![0, 1], "neither is wired");

    let no_default: Vec<u32> = ports
        .hidden_inputs
        .iter()
        .copied()
        .filter(|index| signature.inputs[*index as usize].default_value().is_none())
        .collect();
    assert_eq!(
        no_default,
        vec![1],
        "`Augend` has a default and survives the narrower rule; `Factor` does not"
    );
}

/// Unreal's Show All Pins, which is the same declaration read the other way.
#[test]
fn unreal_show_all_pins() {
    let mut chain = chain();
    let lonely = node(&mut chain.document, Op::Add);
    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .hide_unused_ports = true;
    assert_eq!(
        chain
            .document
            .visible_ports(ROOT, lonely)
            .unwrap()
            .hidden_count(),
        3
    );

    chain
        .document
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(lonely))
        .unwrap()
        .appearance
        .hide_unused_ports = false;
    let ports = chain.document.visible_ports(ROOT, lonely).unwrap();
    assert_eq!(ports.hidden_count(), 0);
    assert_eq!(ports.inputs, vec![0, 1]);
    assert_eq!(ports.outputs, vec![0]);
}

/// Unreal promotes a selection to a re-usable function on the Blueprint. Same
/// boundary, read as a definition that outlives the place it came from: the
/// proof deletes the original instance and instantiates the definition again.
#[test]
fn unreal_promote_selection_to_function() {
    let mut chain = chain();
    let made = chain
        .document
        .group(ROOT, &[chain.add], "Promoted")
        .unwrap();
    chain.document.remove_node(ROOT, made.node).unwrap();
    assert_eq!(chain.document.instance_count(made.definition), 0);

    let fresh = chain
        .document
        .instantiate(ROOT, made.definition, 10, 10)
        .unwrap();
    let two = num(&mut chain.document, 6);
    wire(&mut chain.document, two, 0, fresh, 0);
    wire(&mut chain.document, two, 0, fresh, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, fresh),
        vec![Some(Val::Number(12))]
    );
}

/// And to a macro, whose reading is expansion — proven by round-tripping the
/// boundary and asserting the graph means the same thing afterwards.
#[test]
fn unreal_promote_selection_to_macro() {
    let mut chain = chain();
    let before = chain.document.evaluate(ROOT, chain.sink);
    let made = chain
        .document
        .group(ROOT, &[chain.two, chain.add], "Promoted")
        .unwrap();
    let expanded = chain.document.ungroup(ROOT, made.node).unwrap();

    assert_eq!(expanded.nodes.len(), 2);
    assert_eq!(chain.document.evaluate(ROOT, chain.sink), before);
    assert!(chain.document.validate().is_empty());
}

/// ★ Unreal's Reconstruct Node re-reads a node against a signature that has
/// changed underneath it. The pin's reason is that a signature here is
/// **derived**, so there is nothing to reconstruct — a claim about an absence,
/// proven by changing the interface and observing the instance follow with the
/// links that no longer fit **named**.
#[test]
fn unreal_reconstruct_nodes() {
    let mut chain = chain();
    let made = chain.document.group(ROOT, &[chain.add], "Sum").unwrap();
    let before = chain
        .document
        .signature(ROOT, made.node)
        .unwrap()
        .inputs
        .len();

    let dropped = chain
        .document
        .unexpose(made.definition, InterfaceSide::Input, 0)
        .unwrap();

    assert_eq!(
        chain
            .document
            .signature(ROOT, made.node)
            .unwrap()
            .inputs
            .len(),
        before - 1,
        "the instance answers with the new arity, with no reconstruct step"
    );
    assert_eq!(
        dropped.len(),
        2,
        "the wire inside and the wire at the instance"
    );
    assert!(chain.document.validate().is_empty());
}

/// Unreal resets a pin to the value its declaration gives it. The authored
/// value is what a node carries when nothing else supplies one, so clearing it
/// has to leave **no** authored value — asserted apart from the value that then
/// arrives, because "removed" and "overwritten with the default" look identical
/// if only the second is checked.
#[test]
fn unreal_reset_pin_to_default_value() {
    let mut chain = chain();
    let mul = node(&mut chain.document, Op::Mul);
    wire(&mut chain.document, chain.two, 0, mul, 1);
    assert_eq!(
        chain.document.evaluate(ROOT, mul),
        vec![Some(Val::Number(4))]
    );

    chain
        .document
        .set_port_value(ROOT, mul, PortRef::input(0), Val::Number(10))
        .unwrap();
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::input(0)),
        Some(&Val::Number(10))
    );
    assert_eq!(
        chain.document.evaluate(ROOT, mul),
        vec![Some(Val::Number(20))]
    );

    let cleared = chain
        .document
        .clear_port_value(ROOT, mul, PortRef::input(0))
        .unwrap();
    assert_eq!(
        cleared,
        Some(Val::Number(10)),
        "and it says what it removed"
    );
    assert_eq!(
        chain.document.port_value(ROOT, mul, PortRef::input(0)),
        None,
        "the node carries no authored value at all, rather than a copy of the default"
    );
    assert_eq!(
        chain.document.evaluate(ROOT, mul),
        vec![Some(Val::Number(4))]
    );
}

/// Unreal selects everything feeding the selection — the transitive question,
/// which is the one a person actually has.
#[test]
fn unreal_select_all_input_nodes() {
    let chain = chain();
    let grown = chain
        .document
        .grow(ROOT, &[chain.sink], Grow::Upstream(Reach::Transitive))
        .unwrap();
    assert_eq!(
        grown.selection,
        vec![chain.two, chain.three, chain.add, chain.sink]
    );
}

/// And everything the selection feeds.
#[test]
fn unreal_select_all_output_nodes() {
    let chain = chain();
    let grown = chain
        .document
        .grow(
            ROOT,
            &[chain.two, chain.three],
            Grow::Downstream(Reach::Transitive),
        )
        .unwrap();
    assert_eq!(grown.added, vec![chain.add, chain.sink]);
}
