//! R1577 — the crate's own adversarial fixtures.
//!
//! Every one of these is a hand-written document: no renderer, no window, no
//! pointer. That is the property the crate exists to have.

use serde::{Deserialize, Serialize};

use crate::{
    ConnectError, Document, EditPath, GroupError, InterfaceSide, NestError, NodeBody, NodeId,
    NodeKind, PathError, Port, ROOT, Socket, TreeId, UngroupError, Violation,
};

/// The test taxonomy: two socket types, so type disagreement is reachable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Ty {
    Number,
    Text,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Op {
    Num(i64),
    Word(String),
    Add,
    /// Two numbers in, two numbers out — so multi-output is exercised.
    Split,
    Shout,
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
            Self::Split => "Split",
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
                Port::new("Addend", Ty::Number).with_default(Val::Number(0)),
            ],
            Self::Split => vec![Port::new("Value", Ty::Number)],
            Self::Shout => vec![Port::new("Phrase", Ty::Text)],
            Self::Sink => vec![Port::new("Result", Ty::Number)],
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Num(_) | Self::Add => vec![Port::new("Out", Ty::Number)],
            Self::Word(_) | Self::Shout => vec![Port::new("Out", Ty::Text)],
            Self::Split => vec![Port::new("Half", Ty::Number), Port::new("Rest", Ty::Number)],
            Self::Sink => Vec::new(),
        }
    }

    fn evaluate(&self, inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        let number = |i: usize| inputs.get(i).and_then(Option::as_ref).and_then(Val::number);
        match self {
            Self::Num(n) => vec![Some(Val::Number(*n))],
            Self::Word(w) => vec![Some(Val::Text(w.clone()))],
            Self::Add => vec![number(0).zip(number(1)).map(|(a, b)| Val::Number(a + b))],
            Self::Split => {
                let value = number(0);
                vec![
                    value.map(|v| Val::Number(v / 2)),
                    value.map(|v| Val::Number(v - v / 2)),
                ]
            }
            Self::Shout => vec![inputs.first().and_then(Option::as_ref).map(|v| match v {
                Val::Text(t) => Val::Text(t.to_uppercase()),
                other @ Val::Number(_) => other.clone(),
            })],
            Self::Sink => Vec::new(),
        }
    }
}

/// `two -> add.0`, `three -> add.1`, `add -> sink.0`.
struct Fixture {
    document: Document<Op>,
    two: NodeId,
    three: NodeId,
    add: NodeId,
    sink: NodeId,
}

fn fixture() -> Fixture {
    let mut document = Document::new("root");
    let two = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(2)), 0, 0)
        .unwrap();
    let three = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(3)), 0, 80)
        .unwrap();
    let add = document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 200, 40)
        .unwrap();
    let sink = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 400, 40)
        .unwrap();
    document
        .connect(ROOT, Socket::new(two, 0), Socket::new(add, 0))
        .unwrap();
    document
        .connect(ROOT, Socket::new(three, 0), Socket::new(add, 1))
        .unwrap();
    document
        .connect(ROOT, Socket::new(add, 0), Socket::new(sink, 0))
        .unwrap();
    Fixture {
        document,
        two,
        three,
        add,
        sink,
    }
}

/// Compile-time witness that `T` parses from owned data.
fn owned<T: serde::de::DeserializeOwned>() {}

fn number(value: &[Option<Val>]) -> Option<i64> {
    value.first().and_then(Option::as_ref).and_then(Val::number)
}

// ---------------------------------------------------------------- connecting

#[test]
fn a_refused_wire_names_both_ends_and_both_types() {
    let mut f = fixture();
    let word = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Word("hi".to_owned())), 0, 200)
        .unwrap();
    let error = f
        .document
        .connect(ROOT, Socket::new(word, 0), Socket::new(f.add, 0))
        .unwrap_err();
    assert_eq!(
        error,
        ConnectError::TypeMismatch {
            from: Socket::new(word, 0),
            from_type: Ty::Text,
            to: Socket::new(f.add, 0),
            to_type: Ty::Number,
        }
    );
    assert!(format!("{error}").contains("Text"));
}

#[test]
fn a_wire_that_would_close_a_cycle_names_the_path() {
    let mut f = fixture();
    let split = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Split), 600, 40)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.add, 0), Socket::new(split, 0))
        .unwrap();
    // split -> add would close add -> split -> add.
    let error = f
        .document
        .connect(ROOT, Socket::new(split, 0), Socket::new(f.add, 0))
        .unwrap_err();
    assert_eq!(
        error,
        ConnectError::WouldCycle {
            path: vec![f.add, split]
        }
    );
}

#[test]
fn wiring_an_occupied_input_reports_what_it_displaced() {
    let mut f = fixture();
    let five = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(5)), 0, 160)
        .unwrap();
    let outcome = f
        .document
        .connect(ROOT, Socket::new(five, 0), Socket::new(f.add, 0))
        .unwrap();
    let displaced = outcome.displaced.expect("input 0 was already fed");
    assert_eq!(displaced.from, Socket::new(f.two, 0));
    // One link in, one link out: the input still takes exactly one.
    assert_eq!(
        f.document
            .tree(ROOT)
            .unwrap()
            .links()
            .iter()
            .filter(|l| l.to == Socket::new(f.add, 0))
            .count(),
        1
    );
    assert_eq!(number(&f.document.evaluate(ROOT, f.add)), Some(8));
}

#[test]
fn a_node_cannot_feed_itself() {
    let mut f = fixture();
    let error = f
        .document
        .connect(ROOT, Socket::new(f.add, 0), Socket::new(f.add, 0))
        .unwrap_err();
    assert_eq!(error, ConnectError::SelfLink(f.add));
}

#[test]
fn removing_a_node_reports_the_links_that_went_with_it() {
    let mut f = fixture();
    let dropped = f.document.remove_node(ROOT, f.add).unwrap();
    assert_eq!(dropped.len(), 3);
    assert!(f.document.validate().is_empty());
}

// ------------------------------------------------------------------ grouping

#[test]
fn a_collapse_derives_its_interface_from_what_crosses() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let definition = f.document.tree(made.definition).unwrap();
    assert_eq!(definition.interface().inputs().len(), 2);
    assert_eq!(definition.interface().outputs().len(), 1);
    // The names come from the INTERNAL end: the ports inside the definition.
    assert_eq!(definition.interface().inputs()[0].name, "Augend");
    assert_eq!(definition.interface().inputs()[1].name, "Addend");
    assert_eq!(definition.interface().outputs()[0].name, "Out");
    assert!(f.document.validate().is_empty());
}

#[test]
fn one_external_producer_feeding_two_selected_nodes_is_one_group_input() {
    let mut f = fixture();
    // `two` already feeds add.0; make it feed a second selected node too.
    let other = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 200, 200)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.two, 0), Socket::new(other, 0))
        .unwrap();
    let made = f.document.group(ROOT, &[f.add, other], "Both").unwrap();
    let definition = f.document.tree(made.definition).unwrap();
    // two (shared), three -> 2 inputs, not 3.
    assert_eq!(definition.interface().inputs().len(), 2);
    // And ONE link from `two` into the instance, feeding both nodes inside.
    let into_group = f
        .document
        .tree(ROOT)
        .unwrap()
        .links()
        .iter()
        .filter(|l| l.from == Socket::new(f.two, 0) && l.to.node == made.node)
        .count();
    assert_eq!(into_group, 1);
    assert!(f.document.validate().is_empty());
}

#[test]
fn collapsing_preserves_what_the_graph_computes() {
    let mut f = fixture();
    let before = f.document.evaluate(ROOT, f.sink);
    f.document.group(ROOT, &[f.add], "Sum").unwrap();
    assert_eq!(
        f.document.evaluator().inputs(ROOT, f.sink),
        before_inputs(5)
    );
    assert_eq!(f.document.evaluate(ROOT, f.sink), before);
}

fn before_inputs(n: i64) -> Vec<Option<Val>> {
    vec![Some(Val::Number(n))]
}

#[test]
fn a_bypass_is_refused_and_the_document_is_untouched() {
    let mut f = fixture();
    let untouched = f.document.clone();
    // {two, sink}: two -> add -> sink leaves the selection and returns.
    let error = f.document.group(ROOT, &[f.two, f.sink], "Bad").unwrap_err();
    assert_eq!(
        error,
        GroupError::Bypass {
            path: vec![f.two, f.add, f.sink]
        }
    );
    assert_eq!(
        f.document, untouched,
        "a refused collapse must not leave a half-built definition"
    );
}

#[test]
fn an_empty_selection_is_refused() {
    let mut f = fixture();
    assert_eq!(
        f.document.group(ROOT, &[], "Nothing").unwrap_err(),
        GroupError::Empty
    );
}

#[test]
fn a_trees_own_interface_node_cannot_be_grouped() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let inside = f
        .document
        .tree(made.definition)
        .unwrap()
        .interface_node(InterfaceSide::Input)
        .unwrap()
        .id;
    assert_eq!(
        f.document
            .group(made.definition, &[inside], "Nested")
            .unwrap_err(),
        GroupError::InterfaceNodeSelected(inside)
    );
}

#[test]
fn a_closed_subgraph_groups_with_an_empty_interface() {
    let mut document = Document::new("root");
    let two = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(2)), 0, 0)
        .unwrap();
    let sink = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 200, 0)
        .unwrap();
    document
        .connect(ROOT, Socket::new(two, 0), Socket::new(sink, 0))
        .unwrap();
    let made = document.group(ROOT, &[two, sink], "Closed").unwrap();
    assert!(
        document
            .tree(made.definition)
            .unwrap()
            .interface()
            .is_empty()
    );
    assert!(document.validate().is_empty());
}

// -------------------------------------------------------------- instantiating

#[test]
fn two_instances_of_one_definition_do_not_share_a_value() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let ten = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(10)), 0, 300)
        .unwrap();
    let again = f
        .document
        .instantiate(ROOT, made.definition, 200, 300)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(ten, 0), Socket::new(again, 0))
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(ten, 0), Socket::new(again, 1))
        .unwrap();

    // One evaluator, one memo, both instances read: this is the case a memo
    // keyed by (tree, node) rather than by INSTANCE gets wrong.
    let mut evaluator = f.document.evaluator();
    assert_eq!(number(&evaluator.outputs(ROOT, again)), Some(20));
    assert_eq!(number(&evaluator.outputs(ROOT, made.node)), Some(5));
    assert_eq!(number(&evaluator.outputs(ROOT, again)), Some(20));
}

#[test]
fn a_definition_cannot_be_placed_inside_itself() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let error = f
        .document
        .instantiate(made.definition, made.definition, 0, 0)
        .unwrap_err();
    assert_eq!(
        error,
        NestError::Recursion {
            chain: vec![made.definition]
        }
    );
}

#[test]
fn a_transitive_recursion_names_the_whole_chain() {
    let mut f = fixture();
    let inner = f.document.group(ROOT, &[f.add], "Inner").unwrap();
    // Wrap the instance itself, so Outer contains Inner.
    let outer = f.document.group(ROOT, &[inner.node], "Outer").unwrap();
    // Placing Outer inside Inner closes Inner -> ... no: Outer contains Inner,
    // so putting Outer in Inner closes Outer -> Inner -> Outer.
    let error = f
        .document
        .instantiate(inner.definition, outer.definition, 0, 0)
        .unwrap_err();
    assert_eq!(
        error,
        NestError::Recursion {
            chain: vec![outer.definition, inner.definition]
        }
    );
    assert!(format!("{error}").contains("contains"));
}

#[test]
fn the_root_is_not_a_definition() {
    let mut f = fixture();
    assert_eq!(
        f.document.instantiate(ROOT, ROOT, 0, 0).unwrap_err(),
        NestError::NotADefinition(ROOT)
    );
}

#[test]
fn an_unrelated_definition_nests_freely() {
    let mut f = fixture();
    let sum = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let spare = f.document.add_definition("Spare");
    assert!(f.document.instantiate(sum.definition, spare, 0, 0).is_ok());
    assert!(f.document.validate().is_empty());
}

// -------------------------------------------------------------- ungrouping

#[test]
fn inlining_a_group_restores_what_the_graph_computes() {
    let mut f = fixture();
    let before = f.document.evaluate(ROOT, f.sink);
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let out = f.document.ungroup(ROOT, made.node).unwrap();
    assert_eq!(out.nodes.len(), 1);
    assert!(out.definition_unused);
    assert_eq!(f.document.evaluate(ROOT, f.sink), before);
    assert!(f.document.validate().is_empty());
}

#[test]
fn inlining_one_of_two_instances_leaves_the_definition_in_use() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let again = f
        .document
        .instantiate(ROOT, made.definition, 200, 300)
        .unwrap();
    let out = f.document.ungroup(ROOT, made.node).unwrap();
    assert!(!out.definition_unused);
    assert!(f.document.tree(made.definition).is_some());
    assert_eq!(f.document.instance_count(made.definition), 1);
    assert!(f.document.tree(ROOT).unwrap().node(again).is_some());
}

#[test]
fn inlining_a_group_whose_output_feeds_two_consumers_rewires_both() {
    let mut f = fixture();
    let second = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 400, 200)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.add, 0), Socket::new(second, 0))
        .unwrap();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    f.document.ungroup(ROOT, made.node).unwrap();
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        Some(Val::Number(5))
    );
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(second, 0)),
        Some(Val::Number(5))
    );
    assert!(f.document.validate().is_empty());
}

#[test]
fn inlining_something_that_is_not_a_group_is_refused() {
    let mut f = fixture();
    assert_eq!(
        f.document.ungroup(ROOT, f.add).unwrap_err(),
        UngroupError::NotAGroup(f.add)
    );
}

// ------------------------------------------------------------- the edit path

#[test]
fn the_path_descends_and_comes_back() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let mut path = EditPath::root();
    assert_eq!(path.depth(), 0);
    assert_eq!(path.enter(&f.document, made.node).unwrap(), made.definition);
    assert_eq!(path.current(), made.definition);
    assert_eq!(path.depth(), 1);
    assert_eq!(path.breadcrumb(&f.document), vec!["root", "Sum"]);
    assert_eq!(path.exit().unwrap(), ROOT);
    assert_eq!(path.exit().unwrap_err(), PathError::AtRoot);
}

#[test]
fn entering_something_that_is_not_a_group_is_refused() {
    let f = fixture();
    let mut path = EditPath::root();
    assert_eq!(
        path.enter(&f.document, f.add).unwrap_err(),
        PathError::NotAGroup(f.add)
    );
    assert_eq!(path.current(), ROOT);
}

#[test]
fn a_path_into_a_group_that_was_inlined_is_pruned() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let mut path = EditPath::root();
    path.enter(&f.document, made.node).unwrap();
    f.document.ungroup(ROOT, made.node).unwrap();
    assert_eq!(path.prune(&f.document), 1);
    assert_eq!(path.current(), ROOT);
    assert_eq!(path.prune(&f.document), 0);
}

// -------------------------------------------------------------- evaluation

#[test]
fn an_unlinked_input_uses_its_port_default() {
    let mut f = fixture();
    f.document.remove_node(ROOT, f.two).unwrap();
    // Augend defaults to 0, Addend is still wired to 3.
    assert_eq!(number(&f.document.evaluate(ROOT, f.add)), Some(3));
}

#[test]
fn evaluating_inside_a_definition_uses_the_interface_defaults() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    // Inside, with nothing feeding the group, the interface carries the port
    // defaults the ports were derived from: 0 and 0.
    let inside_add = f
        .document
        .tree(made.definition)
        .unwrap()
        .nodes()
        .find(|n| matches!(n.body, NodeBody::Kind(Op::Add)))
        .unwrap()
        .id;
    assert_eq!(
        number(&f.document.evaluate(made.definition, inside_add)),
        Some(0)
    );
}

#[test]
fn a_group_two_levels_deep_still_evaluates() {
    let mut f = fixture();
    let inner = f.document.group(ROOT, &[f.add], "Inner").unwrap();
    let outer = f.document.group(ROOT, &[inner.node], "Outer").unwrap();
    assert_eq!(number(&f.document.evaluate(ROOT, outer.node)), Some(5));
    assert_eq!(f.document.evaluate(ROOT, f.sink), Vec::new());
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        Some(Val::Number(5))
    );
    assert!(f.document.validate().is_empty());
}

#[test]
fn a_shared_memo_computes_a_shared_upstream_once() {
    let mut f = fixture();
    let second = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 400, 200)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.add, 0), Socket::new(second, 0))
        .unwrap();
    let mut evaluator = f.document.evaluator();
    evaluator.inputs(ROOT, f.sink);
    let after_first = evaluator.cached();
    evaluator.inputs(ROOT, second);
    assert_eq!(
        evaluator.cached(),
        after_first,
        "the second read shares the whole upstream, so nothing new is memoised"
    );
    assert!(!evaluator.truncated());
}

#[test]
fn a_multi_output_node_reports_every_port() {
    let mut f = fixture();
    let split = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Split), 600, 40)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.add, 0), Socket::new(split, 0))
        .unwrap();
    let outputs = f.document.evaluate(ROOT, split);
    assert_eq!(
        outputs,
        vec![Some(Val::Number(2)), Some(Val::Number(3))],
        "5 splits into 2 and 3"
    );
}

// -------------------------------------------------- editing a live interface

#[test]
fn exposing_a_port_reaches_every_instance_at_once() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let again = f
        .document
        .instantiate(ROOT, made.definition, 200, 300)
        .unwrap();
    f.document
        .expose(
            made.definition,
            InterfaceSide::Input,
            Port::new("Extra", Ty::Number),
        )
        .unwrap();
    for instance in [made.node, again] {
        assert_eq!(
            f.document.signature(ROOT, instance).unwrap().inputs.len(),
            3
        );
    }
}

#[test]
fn unexposing_a_port_drops_its_links_and_shifts_the_rest() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    // Instance inputs 0 and 1 are fed by `two` and `three`. Drop input 0.
    let dropped = f
        .document
        .unexpose(made.definition, InterfaceSide::Input, 0)
        .unwrap();
    assert_eq!(dropped.len(), 2, "one link outside, one inside");
    let tree = f.document.tree(ROOT).unwrap();
    let remaining: Vec<_> = tree
        .links()
        .iter()
        .filter(|l| l.to.node == made.node)
        .collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].to.port, 0,
        "the surviving link slid down from port 1"
    );
    assert_eq!(remaining[0].from, Socket::new(f.three, 0));
    assert!(f.document.validate().is_empty());
    // Addend is now the only input, and it is fed by 3; Augend defaults to 0.
    assert_eq!(number(&f.document.evaluate(ROOT, made.node)), Some(3));
}

#[test]
fn unexposing_a_port_that_is_not_there_is_refused() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    assert!(
        f.document
            .unexpose(made.definition, InterfaceSide::Input, 9)
            .is_err()
    );
}

// -------------------------------------------------------------- validation

#[test]
fn a_document_built_through_the_api_never_violates_its_own_rules() {
    let mut f = fixture();
    assert!(f.document.validate().is_empty());
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    assert!(f.document.validate().is_empty());
    f.document
        .instantiate(ROOT, made.definition, 0, 400)
        .unwrap();
    assert!(f.document.validate().is_empty());
    f.document.ungroup(ROOT, made.node).unwrap();
    assert!(f.document.validate().is_empty());
}

#[test]
fn validation_catches_a_document_that_did_not_come_from_here() {
    let mut f = fixture();
    // A hand-built instance of a tree that is not in the document.
    let orphan = f
        .document
        .add_node(ROOT, NodeBody::Group(TreeId(77)), 0, 0)
        .unwrap();
    assert!(
        f.document
            .validate()
            .contains(&Violation::DanglingInstance {
                tree: ROOT,
                node: orphan,
                definition: TreeId(77),
            })
    );
}

#[test]
fn validation_catches_two_nodes_claiming_one_interface_side() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    f.document
        .add_node(
            made.definition,
            NodeBody::Interface(InterfaceSide::Input),
            0,
            0,
        )
        .unwrap();
    assert!(
        f.document
            .validate()
            .contains(&Violation::DuplicateInterfaceNode {
                tree: made.definition,
                side: InterfaceSide::Input,
            })
    );
}

#[test]
fn an_interface_carries_the_socket_type_that_crossed() {
    // The other type, so the derivation is shown not to be number-shaped.
    let mut document = Document::new("root");
    let word = document
        .add_node(ROOT, NodeBody::Kind(Op::Word("hi".to_owned())), 0, 0)
        .unwrap();
    let shout = document
        .add_node(ROOT, NodeBody::Kind(Op::Shout), 200, 0)
        .unwrap();
    let again = document
        .add_node(ROOT, NodeBody::Kind(Op::Shout), 400, 0)
        .unwrap();
    document
        .connect(ROOT, Socket::new(word, 0), Socket::new(shout, 0))
        .unwrap();
    document
        .connect(ROOT, Socket::new(shout, 0), Socket::new(again, 0))
        .unwrap();
    let made = document.group(ROOT, &[shout], "Louder").unwrap();
    let interface = document.tree(made.definition).unwrap().interface();
    assert_eq!(interface.inputs()[0].ty, Ty::Text);
    assert_eq!(interface.outputs()[0].ty, Ty::Text);
    assert_eq!(
        document.evaluator().input(ROOT, Socket::new(again, 0)),
        Some(Val::Text("HI".to_owned()))
    );
}

#[test]
fn a_chain_longer_than_the_recursion_cap_stops_and_says_so() {
    // The cap exists so a pathological document degrades instead of taking the
    // host process down with a stack overflow. A cap nothing tests is a cap
    // nobody can rely on, and this is the direction the other tests never go.
    let mut document = Document::new("deep");
    let source = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(1)), 0, 0)
        .unwrap();
    let mut previous = source;
    for step in 0..600 {
        let next = document
            .add_node(ROOT, NodeBody::Kind(Op::Add), step, 0)
            .unwrap();
        document
            .connect(ROOT, Socket::new(previous, 0), Socket::new(next, 0))
            .unwrap();
        previous = next;
    }
    let mut evaluator = document.evaluator();
    let value = evaluator.outputs(ROOT, previous);
    assert!(
        evaluator.truncated(),
        "the walk hit the cap and must say so rather than folding it into the \
         value it returns"
    );
    assert_eq!(value.len(), 1, "the answer is still the right SHAPE");

    // …and a chain that fits is not truncated, so the flag is not simply on.
    let mut shallow = Document::new("shallow");
    let one = shallow
        .add_node(ROOT, NodeBody::Kind(Op::Num(1)), 0, 0)
        .unwrap();
    let mut evaluator = shallow.evaluator();
    assert_eq!(number(&evaluator.outputs(ROOT, one)), Some(1));
    assert!(!evaluator.truncated());
}

#[test]
fn a_cyclic_document_that_arrived_from_elsewhere_is_caught_and_does_not_hang() {
    // `connect` refuses cycles, so this state is unreachable through the API —
    // which is exactly why the guard needs a document that did NOT come through
    // it. Serde is the door such a document actually arrives by.
    // The link is RE-POINTED rather than added: a second link into one input
    // is an over-fed input, not a cycle, because `link_into` answers with the
    // first — which is a fact this test found by asserting the wrong thing
    // first and being told the value was still 5.
    let f = fixture();
    let mut wire = serde_json::to_value(&f.document).unwrap();
    let links = wire["trees"][0]["links"].as_array_mut().unwrap();
    let feeding = links
        .iter_mut()
        .find(|l| l["to"]["node"] == f.add.0 && l["to"]["port"] == 0)
        .expect("the fixture wires add's first input");
    feeding["from"] = serde_json::json!({"node": f.add, "port": 0});
    let corrupt: Document<Op> = serde_json::from_value(wire).unwrap();

    assert!(
        corrupt
            .validate()
            .contains(&Violation::Cycle { tree: ROOT }),
        "validate names the cycle: {:?}",
        corrupt.validate()
    );
    // The honest answer for a value that depends on itself is "no value" — and
    // arriving at it at all is the assertion: an evaluator without the
    // re-entrancy guard does not return from this call.
    let mut evaluator = corrupt.evaluator();
    assert_eq!(
        evaluator.outputs(ROOT, f.add),
        vec![None],
        "evaluation terminates rather than recurring forever"
    );
    // …and it terminates because the RE-ENTRANCY guard caught it, not because
    // the depth cap did. Without this line the two guards shadow each other:
    // removing the re-entrancy check leaves the same value, reached 512 frames
    // later, and the test would pass for the wrong reason.
    assert!(
        !evaluator.truncated(),
        "a cycle is caught where it closes, not by running out of depth"
    );
}

#[test]
fn a_document_survives_a_round_trip_through_its_wire_form() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    f.document
        .instantiate(ROOT, made.definition, 0, 400)
        .unwrap();
    // Owned deserialization is the property a reactive `Signal<T>` payload
    // needs, so it is asserted at the type level rather than inferred from one
    // call site that happens to compile. A taxonomy carrying a borrowed field
    // is only `Deserialize<'static>` and fails HERE rather than at a consumer.
    owned::<Document<Op>>();

    let wire = serde_json::to_value(&f.document).unwrap();
    let back: Document<Op> = serde_json::from_value(wire).unwrap();
    assert_eq!(back, f.document);
    assert!(back.validate().is_empty());
    assert_eq!(
        back.evaluate(ROOT, made.node),
        f.document.evaluate(ROOT, made.node)
    );
}

#[test]
fn a_node_states_its_id_once() {
    // The map is a sequence on the wire, so there is no key to disagree with
    // the node under it.
    let f = fixture();
    let wire = serde_json::to_value(&f.document).unwrap();
    let nodes = &wire["trees"][0]["nodes"];
    assert!(nodes.is_array(), "nodes travel as a sequence, got {nodes}");
    assert_eq!(nodes[0]["id"], 0);
}

#[test]
fn a_group_node_is_named_without_the_taxonomy_being_asked() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let node = f.document.tree(ROOT).unwrap().node(made.node).unwrap();
    assert_eq!(node.display_name(), "Group");
    let inside = f
        .document
        .tree(made.definition)
        .unwrap()
        .interface_node(InterfaceSide::Input)
        .unwrap();
    assert_eq!(inside.display_name(), "Group Input");
}
