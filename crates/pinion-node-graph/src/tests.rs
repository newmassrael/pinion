//! R1577 — the crate's own adversarial fixtures.
//!
//! Every one of these is a hand-written document: no renderer, no window, no
//! pointer. That is the property the crate exists to have.

use serde::{Deserialize, Serialize};

use crate::{
    Appearance, ConnectError, Crossings, Definitions, Document, DuplicateError, EditPath,
    ExtractError, Fragment, GroupError, InsertError, InterfaceSide, NestError, NodeBody, NodeId,
    NodeKind, PathError, Port, ROOT, RepartitionError, Route, Severed, Sharing, Socket, TreeId,
    UngroupError, Violation,
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
    /// Text in, number out — the one kind whose output NO input can feed, which
    /// is what makes a dropped pass-through reachable on a node that has inputs.
    Measure,
    /// Two numbers in, two numbers out, exchanged. The one shape where routing
    /// by POSITION and routing by "the lowest input of the right type" give
    /// different answers, so it is what makes the identity rule falsifiable.
    Swap,
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
            Self::Measure => "Measure",
            Self::Swap => "Swap",
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
            Self::Shout | Self::Measure => vec![Port::new("Phrase", Ty::Text)],
            Self::Swap => vec![
                Port::new("Left", Ty::Number).with_default(Val::Number(-1)),
                Port::new("Right", Ty::Number).with_default(Val::Number(-2)),
            ],
            Self::Sink => vec![Port::new("Result", Ty::Number)],
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Num(_) | Self::Add | Self::Measure => vec![Port::new("Out", Ty::Number)],
            Self::Word(_) | Self::Shout => vec![Port::new("Out", Ty::Text)],
            Self::Split => vec![Port::new("Half", Ty::Number), Port::new("Rest", Ty::Number)],
            Self::Swap => vec![
                Port::new("Left", Ty::Number),
                Port::new("Right", Ty::Number),
            ],
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
            Self::Measure => vec![inputs.first().and_then(Option::as_ref).map(|v| match v {
                Val::Text(t) => Val::Number(i64::try_from(t.len()).unwrap_or(i64::MAX)),
                other @ Val::Number(_) => other.clone(),
            })],
            Self::Swap => vec![number(1).map(Val::Number), number(0).map(Val::Number)],
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

// ----------------------------------------------------------------- fragments
//
// R1578. A fragment is a piece of a graph standing on its own, so every one of
// these builds one from a hand-written document and asks it questions no
// clipboard-as-a-file can answer.

/// A document holding one `Sum` definition with two instances, plus the sources
/// feeding the first.
struct GroupedDoc {
    document: Document<Op>,
    definition: TreeId,
    /// The first instance, fed by `two` and `three`.
    instance: NodeId,
}

fn grouped() -> GroupedDoc {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    GroupedDoc {
        document: f.document,
        definition: made.definition,
        instance: made.node,
    }
}

#[test]
fn a_cut_records_the_wires_it_severed() {
    // Copying the middle of `two/three -> add -> sink` severs three wires.
    // Blender's `node_copy_local` copies a link only when both ends are
    // selected and records the others nowhere at all.
    let f = fixture();
    let cut = f.document.extract(ROOT, &[f.add]).unwrap();
    assert_eq!(cut.node_count(), 1);
    assert_eq!(cut.links(), &[] as &[crate::Link]);

    // Two values arrived; each is one crossing, keyed by its producer.
    let producers: Vec<Socket> = cut.inbound().iter().map(Severed::producer).collect();
    assert_eq!(
        producers,
        vec![Socket::new(f.two, 0), Socket::new(f.three, 0)]
    );
    assert_eq!(cut.inbound()[0].consumers(), &[Socket::new(f.add, 0)]);

    // One left.
    assert_eq!(cut.outbound().len(), 1);
    assert_eq!(cut.outbound()[0].producer(), Socket::new(f.add, 0));
    assert_eq!(cut.outbound()[0].consumers(), &[Socket::new(f.sink, 0)]);
}

#[test]
fn one_producer_feeding_two_copied_nodes_is_one_crossing() {
    // The producer keys the crossing, exactly as it keys a group interface
    // socket: two links, one value.
    let mut f = fixture();
    let other = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 200, 200)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.two, 0), Socket::new(other, 0))
        .unwrap();
    let cut = f.document.extract(ROOT, &[f.add, other]).unwrap();
    let from_two: Vec<&Severed> = cut
        .inbound()
        .iter()
        .filter(|s| s.producer() == Socket::new(f.two, 0))
        .collect();
    assert_eq!(from_two.len(), 1, "one value, not one per link");
    assert_eq!(
        from_two[0].consumers(),
        &[Socket::new(f.add, 0), Socket::new(other, 0)]
    );
}

#[test]
fn a_selection_a_collapse_would_refuse_can_still_be_copied() {
    // two -> add -> sink; copying {two, sink} bypasses `add`. A COLLAPSE is
    // refused because the new vertex would reach itself; a CUT severs the
    // crossings instead, so there is nothing to refuse. R1577 fused the two
    // questions into one derivation, and this is the case that separates them.
    let f = fixture();
    assert!(matches!(
        f.document.clone().group(ROOT, &[f.two, f.sink], "no"),
        Err(GroupError::Bypass { .. })
    ));
    let cut = f.document.extract(ROOT, &[f.two, f.sink]).unwrap();
    assert_eq!(cut.node_count(), 2);
    assert!(cut.links().is_empty(), "nothing joins them directly");
    assert_eq!(cut.outbound().len(), 1, "two -> add left the selection");
    assert_eq!(cut.inbound().len(), 1, "add -> sink entered it");
}

#[test]
fn a_cut_leaves_the_document_exactly_as_it_was() {
    let f = fixture();
    let before = f.document.clone();
    let _ = f.document.extract(ROOT, &[f.add, f.two]).unwrap();
    assert_eq!(f.document, before);
}

#[test]
fn an_empty_or_unknown_selection_is_refused_by_name() {
    let f = fixture();
    assert_eq!(
        f.document.extract(ROOT, &[]).unwrap_err(),
        ExtractError::Empty
    );
    assert_eq!(
        f.document.extract(ROOT, &[NodeId(99)]).unwrap_err(),
        ExtractError::NoSuchNode(NodeId(99))
    );
    assert_eq!(
        f.document.extract(TreeId(9), &[f.add]).unwrap_err(),
        ExtractError::NoSuchTree(TreeId(9))
    );
}

#[test]
fn an_interface_node_cannot_be_copied() {
    // It is a projection of the tree it belongs to, not a thing that travels:
    // a tree holds at most one per side, which `validate` enforces.
    let g = grouped();
    let inside = g
        .document
        .tree(g.definition)
        .unwrap()
        .interface_node(InterfaceSide::Input)
        .unwrap()
        .id;
    assert_eq!(
        g.document.extract(g.definition, &[inside]).unwrap_err(),
        ExtractError::InterfaceNodeSelected(inside)
    );
}

#[test]
fn a_fragment_is_a_document_that_still_answers() {
    // Blender's clipboard is `copybuffer_nodes.blend` in the temp directory, so
    // the only thing that can be done with it is a paste.
    let g = grouped();
    let cut = g.document.extract(ROOT, &[g.instance]).unwrap();
    assert!(cut.validate().is_empty());
    assert_eq!(cut.node_count(), 1);
    assert_eq!(cut.source_tree(), ROOT);
    assert_eq!(cut.definitions().count(), 1, "the group came too");
    assert_eq!(cut.definitions().next().unwrap().name, "Sum");
    // And it computes: the carried definition adds its own port defaults.
    let inner = cut.definitions().next().unwrap().id;
    let exit = cut
        .document()
        .tree(inner)
        .unwrap()
        .interface_node(InterfaceSide::Output)
        .unwrap()
        .id;
    assert!(cut.document().evaluate(inner, exit).is_empty());
}

#[test]
fn a_fragment_round_trips_through_serde() {
    owned::<Fragment<Op>>();
    let g = grouped();
    let cut = g.document.extract(ROOT, &[g.instance]).unwrap();
    let wire = serde_json::to_string(&cut).unwrap();
    let back: Fragment<Op> = serde_json::from_str(&wire).unwrap();
    assert_eq!(back, cut);
    assert_eq!(back.inbound().len(), cut.inbound().len());
}

#[test]
fn a_copied_group_instance_brings_the_whole_closure() {
    // Nest Sum inside Outer, then copy the Outer instance: BOTH definitions must
    // travel, or the paste lands a group pointing at nothing.
    let mut g = grouped();
    let outer = g.document.add_definition("Outer");
    g.document.instantiate(outer, g.definition, 0, 0).unwrap();
    let host = g.document.instantiate(ROOT, outer, 600, 600).unwrap();

    let cut = g.document.extract(ROOT, &[host]).unwrap();
    let names: Vec<&str> = cut.definitions().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["Sum", "Outer"], "inner-first, and both present");
    assert!(cut.validate().is_empty(), "no dangling instance");
}

#[test]
fn pasting_a_group_twice_leaves_one_definition() {
    // Sharing is what a group IS: two instances of one definition, so editing
    // the definition moves both.
    let mut g = grouped();
    let cut = g.document.extract(ROOT, &[g.instance]).unwrap();
    let before = g.document.tree_count();
    for _ in 0..2 {
        let out = g
            .document
            .insert(ROOT, &cut, (900, 0), Crossings::Drop, Definitions::Share)
            .unwrap();
        assert_eq!(out.definitions_added, vec![]);
        assert_eq!(out.definitions_reused, vec![g.definition]);
    }
    assert_eq!(g.document.tree_count(), before);
    assert_eq!(g.document.instance_count(g.definition), 3);
}

#[test]
fn forking_gives_the_copy_a_definition_of_its_own() {
    // Blender's `duplicate(linked=false)`, where the arm is chosen by a USER
    // PREFERENCE (`U.dupflag & USER_DUP_NTREE`) rather than stated at the call.
    let mut g = grouped();
    let cut = g.document.extract(ROOT, &[g.instance]).unwrap();
    let out = g
        .document
        .insert(ROOT, &cut, (900, 0), Crossings::Drop, Definitions::Fork)
        .unwrap();
    assert_eq!(out.definitions_added.len(), 1);
    assert!(out.definitions_reused.is_empty());
    let forked = out.definitions_added[0];
    assert_ne!(forked, g.definition);

    // Editing the fork leaves the original alone — the whole point of the arm.
    g.document
        .add_node(forked, NodeBody::Kind(Op::Num(41)), 0, 0)
        .unwrap();
    assert_eq!(
        g.document.tree(g.definition).unwrap().node_count(),
        g.document.tree(forked).unwrap().node_count() - 1
    );
    assert!(g.document.validate().is_empty());
}

#[test]
fn a_same_named_but_different_definition_is_not_rebound() {
    // THE case. Blender's paste matches a candidate datablock by NAME
    // (`BKE_main_merge` keys on `id->name`, and for two local IDs
    // `are_ids_from_different_mains_matching` returns true on the name alone),
    // so pasting a group into a file with an unrelated same-named group binds
    // the instance to the wrong graph. Content matching cannot: the imposter
    // computes something else, so it is not a match.
    let g = grouped();
    let cut = g.document.extract(ROOT, &[g.instance]).unwrap();

    let mut other = Document::new("elsewhere");
    let imposter = other.add_definition("Sum");
    let lie = other
        .add_node(imposter, NodeBody::Kind(Op::Num(9000)), 0, 0)
        .unwrap();
    other
        .expose(
            imposter,
            InterfaceSide::Output,
            Port::new("Out", Ty::Number),
        )
        .unwrap();
    let exit = other
        .add_node(imposter, NodeBody::Interface(InterfaceSide::Output), 200, 0)
        .unwrap();
    other
        .connect(imposter, Socket::new(lie, 0), Socket::new(exit, 0))
        .unwrap();

    let out = other
        .insert(ROOT, &cut, (0, 0), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert_eq!(
        out.definitions_reused,
        vec![],
        "the name is not the identity"
    );
    assert_eq!(out.definitions_added.len(), 1);
    assert_ne!(out.definitions_added[0], imposter);

    // And the pasted instance computes what it computed at home, not 9000.
    let pasted = out.nodes[0];
    assert_eq!(number(&other.evaluate(ROOT, pasted)), Some(0));
    assert!(other.validate().is_empty());
}

#[test]
fn a_definition_edited_since_the_copy_is_added_rather_than_silently_reused() {
    // The fragment is a snapshot. If the original has moved on, re-using it
    // would paste something other than what was copied.
    let mut g = grouped();
    let cut = g.document.extract(ROOT, &[g.instance]).unwrap();
    g.document
        .add_node(g.definition, NodeBody::Kind(Op::Num(1)), 500, 500)
        .unwrap();
    let out = g
        .document
        .insert(ROOT, &cut, (900, 0), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert!(out.definitions_reused.is_empty());
    assert_eq!(out.definitions_added.len(), 1);
}

#[test]
fn pasting_a_group_inside_itself_is_refused_and_names_the_chain() {
    // Copy the instance, enter the group, paste: the one recursion an editor
    // meets by accident.
    let mut g = grouped();
    let cut = g.document.extract(ROOT, &[g.instance]).unwrap();
    let error = g
        .document
        .insert(
            g.definition,
            &cut,
            (0, 0),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap_err();
    assert_eq!(
        error,
        InsertError::Recursion {
            chain: vec![g.definition]
        }
    );
    assert!(format!("{error}").contains("nest a group inside itself"));
}

#[test]
fn a_deeper_recursion_is_refused_too_and_the_chain_is_longer() {
    // Outer contains Sum. Pasting an Outer instance into Sum closes
    // Outer -> Sum -> Outer, and the chain names the definition in between —
    // Blender's `node_group_poll` reports the same flat sentence at any depth.
    let mut g = grouped();
    let outer = g.document.add_definition("Outer");
    g.document.instantiate(outer, g.definition, 0, 0).unwrap();
    let host = g.document.instantiate(ROOT, outer, 600, 600).unwrap();
    let cut = g.document.extract(ROOT, &[host]).unwrap();
    let error = g
        .document
        .insert(
            g.definition,
            &cut,
            (0, 0),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap_err();
    assert_eq!(
        error,
        InsertError::Recursion {
            chain: vec![outer, g.definition]
        }
    );
}

#[test]
fn a_refused_insert_leaves_the_document_untouched() {
    // Blender's paste does the opposite: `node_copy_local` reports the node it
    // cannot place, skips it AND its links, and finishes — so a five-node paste
    // can land four nodes and a message in a report list.
    let mut g = grouped();
    let loose = g
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(7)), 0, 900)
        .unwrap();
    let cut = g.document.extract(ROOT, &[g.instance, loose]).unwrap();
    let before = g.document.clone();
    assert!(
        g.document
            .insert(
                g.definition,
                &cut,
                (0, 0),
                Crossings::Drop,
                Definitions::Share
            )
            .is_err()
    );
    assert_eq!(g.document, before, "not one of the two nodes landed");
}

#[test]
fn keeping_the_inbound_crossings_refeeds_the_copies() {
    // Blender's `keep_inputs`, which exists on `NODE_OT_duplicate` only:
    // `NODE_OT_clipboard_paste` declares one property, `offset`.
    let f = fixture();
    let mut document = f.document.clone();
    let cut = document.extract(ROOT, &[f.add]).unwrap();
    let out = document
        .insert(
            ROOT,
            &cut,
            (200, 400),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(out.reattached.len(), 2);
    assert!(out.unattached.is_empty());
    let copy = out.nodes[0];
    assert_eq!(
        number(&document.evaluate(ROOT, copy)),
        Some(5),
        "fed from the same two sources as the original"
    );
    // The original kept its own wires: an output feeds any number of consumers.
    assert_eq!(number(&document.evaluate(ROOT, f.add)), Some(5));
    assert!(document.validate().is_empty());
}

#[test]
fn dropping_the_crossings_leaves_the_copy_on_its_port_defaults() {
    let f = fixture();
    let mut document = f.document.clone();
    let cut = document.extract(ROOT, &[f.add]).unwrap();
    let out = document
        .insert(ROOT, &cut, (200, 400), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert!(out.reattached.is_empty());
    assert!(out.unattached.is_empty(), "nothing was attempted");
    assert_eq!(number(&document.evaluate(ROOT, out.nodes[0])), Some(0));
}

#[test]
fn an_inbound_crossing_that_cannot_be_restored_is_named() {
    // Paste into a tree where the producing socket is not there. Naming it is
    // what lets a UI say "two inputs did not come back".
    //
    // It is also the case that caught the ordering defect this insertion was
    // first written with. The crossings name node 0 and node 1; the destination
    // is EMPTY, so the copy is allocated node 0 — and a re-attachment resolved
    // after the copies exist finds the copy where its own producer used to be
    // and wires it to itself. The address space must be read before it is
    // written to.
    let f = fixture();
    let cut = f.document.extract(ROOT, &[f.add]).unwrap();
    let mut elsewhere = Document::new("elsewhere");
    let out = elsewhere
        .insert(
            ROOT,
            &cut,
            (0, 0),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert!(out.reattached.is_empty());
    assert_eq!(out.unattached.len(), 2);
    assert_eq!(out.unattached[0].producer(), Socket::new(f.two, 0));
    assert!(format!("{}", out.unattached[0]).contains("node"));
    assert_eq!(out.nodes, vec![NodeId(0)], "the copy took the vacant id");
    assert!(
        elsewhere.tree(ROOT).unwrap().links().is_empty(),
        "and nothing wired it to itself"
    );
    assert!(elsewhere.validate().is_empty());
}

#[test]
fn a_restored_crossing_is_type_checked_before_it_is_wired() {
    // A fragment names the outside end of a crossing by its socket ADDRESS,
    // which is exact going home and a guess anywhere else. The guess is checked:
    // node 0 exists in the destination too, and produces Text.
    let f = fixture();
    let cut = f.document.extract(ROOT, &[f.add]).unwrap();
    let mut elsewhere = Document::new("elsewhere");
    for _ in 0..2 {
        elsewhere
            .add_node(ROOT, NodeBody::Kind(Op::Word("no".to_owned())), 0, 0)
            .unwrap();
    }
    assert_eq!(elsewhere.tree(ROOT).unwrap().node_count(), 2);
    let out = elsewhere
        .insert(
            ROOT,
            &cut,
            (0, 0),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert!(
        out.reattached.is_empty(),
        "a Text output must not be wired into a Number input"
    );
    assert_eq!(out.unattached.len(), 2);
    assert!(elsewhere.validate().is_empty());
}

#[test]
fn the_outbound_crossings_are_published_and_never_restored() {
    // An input takes at most one link, so restoring an outbound crossing would
    // displace the original's — the copy would steal the connection. Blender has
    // `keep_inputs` and no `keep_outputs`, and says nowhere why.
    let f = fixture();
    let mut document = f.document.clone();
    let cut = document.extract(ROOT, &[f.add]).unwrap();
    assert_eq!(cut.outbound().len(), 1);
    let out = document
        .insert(
            ROOT,
            &cut,
            (0, 400),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    let sink_feed = document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.sink, 0))
        .unwrap();
    assert_eq!(
        sink_feed.from,
        Socket::new(f.add, 0),
        "the original still feeds the sink"
    );
    assert!(!out.nodes.contains(&f.add));
}

#[test]
fn inserting_at_the_origin_puts_the_nodes_back_where_they_were() {
    let f = fixture();
    let mut document = f.document.clone();
    let cut = document.extract(ROOT, &[f.two, f.three]).unwrap();
    let out = document
        .insert(
            ROOT,
            &cut,
            cut.origin(),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    let placed: Vec<(i32, i32)> = out
        .nodes
        .iter()
        .map(|&id| {
            let n = document.tree(ROOT).unwrap().node(id).unwrap();
            (n.x, n.y)
        })
        .collect();
    assert_eq!(placed, vec![(0, 0), (0, 80)]);
}

#[test]
fn a_fragment_carrying_an_interface_node_is_refused() {
    // Not reachable through `extract`; reachable through serde, which is what
    // `validate` exists for and what this refusal protects.
    let g = grouped();
    let inside = g.document.extract(ROOT, &[g.instance]).unwrap();
    let mut wire = serde_json::to_value(&inside).unwrap();
    wire["content"]["trees"][0]["nodes"][0]["body"] = serde_json::json!({ "Interface": "Input" });
    let doctored: Fragment<Op> = serde_json::from_value(wire).unwrap();
    let mut document = g.document.clone();
    let before = document.clone();
    assert_eq!(
        document
            .insert(ROOT, &doctored, (0, 0), Crossings::Drop, Definitions::Share)
            .unwrap_err(),
        InsertError::InterfaceNodeInFragment(g.instance)
    );
    assert_eq!(document, before);
}

#[test]
fn duplicate_is_a_cut_and_a_paste_and_says_which_half_refused() {
    let mut g = grouped();
    let out = g
        .document
        .duplicate(
            ROOT,
            &[g.instance],
            (0, 300),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(out.nodes.len(), 1);
    assert_eq!(out.definitions_reused, vec![g.definition]);
    assert_eq!(out.reattached.len(), 2);
    assert_eq!(number(&g.document.evaluate(ROOT, out.nodes[0])), Some(5));

    let original = g.document.tree(ROOT).unwrap().node(g.instance).unwrap();
    let copy = g.document.tree(ROOT).unwrap().node(out.nodes[0]).unwrap();
    assert_eq!((copy.x, copy.y), (original.x, original.y + 300));

    assert_eq!(
        g.document
            .duplicate(ROOT, &[], (0, 0), Crossings::Drop, Definitions::Share)
            .unwrap_err(),
        DuplicateError::Cut(ExtractError::Empty)
    );
}

#[test]
fn a_copy_evaluates_to_what_the_original_evaluates_to() {
    // The end-to-end property: a duplicate is a duplicate.
    let mut f = fixture();
    let out = f
        .document
        .duplicate(
            ROOT,
            &[f.two, f.three, f.add],
            (0, 400),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(out.nodes.len(), 3);
    assert_eq!(out.links.len(), 2, "the internal wires came too");
    let copied_add = out.nodes[2];
    assert_eq!(
        number(&f.document.evaluate(ROOT, copied_add)),
        number(&f.document.evaluate(ROOT, f.add))
    );
    assert!(f.document.validate().is_empty());
}

#[test]
fn a_label_survives_the_round_trip() {
    let mut f = fixture();
    f.document
        .tree_mut(ROOT)
        .unwrap()
        .node_mut(f.add)
        .unwrap()
        .label = Some("Total".to_owned());
    let out = f
        .document
        .duplicate(
            ROOT,
            &[f.add],
            (0, 400),
            Crossings::Drop,
            Definitions::Share,
        )
        .unwrap();
    let copy = f.document.tree(ROOT).unwrap().node(out.nodes[0]).unwrap();
    assert_eq!(copy.display_name(), "Total");
}

#[test]
fn a_fragment_crosses_into_a_document_that_never_had_the_definition() {
    // The clipboard case: the destination knows nothing of `Sum`.
    let g = grouped();
    let cut = g.document.extract(ROOT, &[g.instance]).unwrap();
    let mut other = Document::new("elsewhere");
    let out = other
        .insert(ROOT, &cut, (0, 0), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert_eq!(out.definitions_added.len(), 1);
    assert_eq!(other.tree(out.definitions_added[0]).unwrap().name, "Sum");
    assert!(other.validate().is_empty());
    assert_eq!(number(&other.evaluate(ROOT, out.nodes[0])), Some(0));
}

// ------------------------------------------------- R1584, the boundary moves

/// The fixture, with `add` collapsed into `Sum` and everything named.
///
/// `two -> g.0`, `three -> g.1`, `g.0 -> sink.0`, and inside the definition the
/// `add` node keeps its id, because ids are per tree.
struct Boundaried {
    document: Document<Op>,
    two: NodeId,
    three: NodeId,
    add: NodeId,
    sink: NodeId,
    definition: TreeId,
    instance: NodeId,
}

fn boundaried() -> Boundaried {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    Boundaried {
        document: f.document,
        two: f.two,
        three: f.three,
        add: f.add,
        sink: f.sink,
        definition: made.definition,
        instance: made.node,
    }
}

/// The value arriving at a socket: what the graph actually delivers there.
///
/// The question a user asks after moving a node — "is my result still my
/// result" — rather than the question the structure answers.
fn value_into(document: &Document<Op>, tree: TreeId, socket: Socket) -> Option<Val> {
    let link = document.tree(tree)?.link_into(socket)?;
    let from = link.from;
    document
        .evaluate(tree, from.node)
        .get(from.port as usize)
        .cloned()
        .flatten()
}

/// Blender's Separate/Move, re-expressed over this crate's types.
///
/// `node_group_separate_selected` at `8cf50599`: copy the selected nodes into
/// the parent tree, and for the Move arm delete them from the group. It does
/// not touch the interface and does not reconnect anything. Present so the
/// divergence is *asserted* rather than described — a test that only checked
/// our own answer could not tell a better rule from an equal one.
fn blender_separate(
    document: &mut Document<Op>,
    host: TreeId,
    definition: TreeId,
    instance: NodeId,
    selection: &[NodeId],
) {
    let origin = document
        .tree(host)
        .and_then(|t| t.node(instance))
        .map_or((0, 0), |n| (n.x, n.y));
    let mut copied = std::collections::BTreeMap::new();
    for &id in selection {
        let node = document.tree(definition).unwrap().node(id).unwrap().clone();
        let fresh = document
            .add_node(host, node.body, origin.0 + node.x, origin.1 + node.y)
            .unwrap();
        copied.insert(id, fresh);
    }
    let inner: Vec<crate::Link> = document.tree(definition).unwrap().links().to_vec();
    for link in inner {
        if let (Some(&from), Some(&to)) = (copied.get(&link.from.node), copied.get(&link.to.node)) {
            document
                .connect(
                    host,
                    Socket::new(from, link.from.port),
                    Socket::new(to, link.to.port),
                )
                .unwrap();
        }
    }
    for &id in selection {
        document.remove_node(definition, id).unwrap();
    }
}

/// Blender's Group Insert interface rule, counted rather than performed.
///
/// `build_node_set_interface` walks only the sockets of the nodes being moved
/// and appends one interface socket per value linked to a node outside them. It
/// never consults the group's existing interface, so a value that already
/// crosses at this instance gets a second port.
fn blender_insert_port_count(document: &Document<Op>, tree: TreeId, moving: &[NodeId]) -> usize {
    let host = document.tree(tree).unwrap();
    let moved: std::collections::BTreeSet<NodeId> = moving.iter().copied().collect();
    // Keyed by SIDE as well as socket: a `Socket` says which port, never which
    // half of the node, so `twin.in0` and `twin.out0` are the same value.
    let mut sockets = std::collections::BTreeSet::new();
    for link in host.links() {
        if moved.contains(&link.to.node) && !moved.contains(&link.from.node) {
            sockets.insert((InterfaceSide::Input, link.to));
        }
        if moved.contains(&link.from.node) && !moved.contains(&link.to.node) {
            sockets.insert((InterfaceSide::Output, link.from));
        }
    }
    sockets.len()
}

#[test]
fn a_node_moved_into_a_group_keeps_what_the_graph_computes() {
    let mut b = boundaried();
    assert_eq!(
        value_into(&b.document, ROOT, Socket::new(b.sink, 0)),
        Some(Val::Number(5))
    );

    let out = b
        .document
        .group_insert(ROOT, b.instance, &[b.two], Sharing::Shared)
        .unwrap();

    assert_eq!(out.definition, b.definition);
    assert_eq!(out.forked_from, None);
    assert_eq!(out.moved.len(), 1);
    assert!(b.document.tree(ROOT).unwrap().node(b.two).is_none());
    assert!(
        b.document
            .tree(b.definition)
            .unwrap()
            .node(out.moved[0])
            .is_some()
    );
    // The value it used to carry across the boundary is produced inside now, so
    // the port that carried it is gone rather than left describing nothing.
    assert_eq!(out.unexposed.len(), 1);
    assert_eq!(out.unexposed[0].side, InterfaceSide::Input);
    assert_eq!(out.unexposed[0].port.name, "Augend");
    assert!(out.exposed.is_empty());
    assert_eq!(
        b.document
            .tree(b.definition)
            .unwrap()
            .interface()
            .inputs()
            .len(),
        1
    );
    assert_eq!(
        value_into(&b.document, ROOT, Socket::new(b.sink, 0)),
        Some(Val::Number(5))
    );
    assert!(b.document.validate().is_empty());
}

#[test]
fn a_value_that_already_crosses_does_not_get_a_second_port() {
    let mut b = boundaried();
    // A second adder fed by the same two sources, sending its result on.
    let twin = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 200, 300)
        .unwrap();
    let tail = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 400, 300)
        .unwrap();
    for (from, port) in [(b.two, 0), (b.three, 1)] {
        b.document
            .connect(ROOT, Socket::new(from, 0), Socket::new(twin, port))
            .unwrap();
    }
    b.document
        .connect(ROOT, Socket::new(twin, 0), Socket::new(tail, 0))
        .unwrap();

    // Blender would expose one socket per connected socket of the moved node:
    // three, two of them duplicating values that already cross here.
    assert_eq!(blender_insert_port_count(&b.document, ROOT, &[twin]), 3);

    let out = b
        .document
        .group_insert(ROOT, b.instance, &[twin], Sharing::Shared)
        .unwrap();

    // One value is one port: only the twin's own result is new.
    assert_eq!(out.exposed.len(), 1);
    assert_eq!(out.exposed[0].side, InterfaceSide::Output);
    assert!(out.unexposed.is_empty());
    let interface = b.document.tree(b.definition).unwrap().interface();
    assert_eq!(interface.inputs().len(), 2);
    assert_eq!(interface.outputs().len(), 2);
    assert_eq!(
        value_into(&b.document, ROOT, Socket::new(b.sink, 0)),
        Some(Val::Number(5))
    );
    assert_eq!(
        value_into(&b.document, ROOT, Socket::new(tail, 0)),
        Some(Val::Number(5))
    );
    assert!(b.document.validate().is_empty());
}

#[test]
fn an_insert_through_a_shared_definition_names_what_it_cost_elsewhere() {
    let mut b = boundaried();
    let seven = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(7)), 0, 400)
        .unwrap();
    let eight = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(8)), 0, 480)
        .unwrap();
    let twin = b
        .document
        .instantiate(ROOT, b.definition, 200, 440)
        .unwrap();
    b.document
        .connect(ROOT, Socket::new(seven, 0), Socket::new(twin, 0))
        .unwrap();
    b.document
        .connect(ROOT, Socket::new(eight, 0), Socket::new(twin, 1))
        .unwrap();
    assert_eq!(number(&b.document.evaluate(ROOT, twin)), Some(15));

    let out = b
        .document
        .group_insert(ROOT, b.instance, &[b.two], Sharing::Shared)
        .unwrap();

    // The other instance came along, and the link it lost is named WITH the
    // tree it was in — a link id means nothing without one.
    assert_eq!(out.other_instances, 1);
    assert_eq!(out.severed.len(), 1);
    assert_eq!(out.severed[0].tree, ROOT);
    assert_eq!(out.severed[0].link.to, Socket::new(twin, 0));
    assert_eq!(out.severed[0].link.from, Socket::new(seven, 0));
    // `seven` is disconnected and `eight` slid down onto the surviving port.
    assert_eq!(number(&b.document.evaluate(ROOT, twin)), Some(10));
    assert!(b.document.validate().is_empty());
}

#[test]
fn a_forked_insert_leaves_the_other_instance_exactly_as_it_was() {
    let mut b = boundaried();
    let seven = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(7)), 0, 400)
        .unwrap();
    let eight = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(8)), 0, 480)
        .unwrap();
    let twin = b
        .document
        .instantiate(ROOT, b.definition, 200, 440)
        .unwrap();
    b.document
        .connect(ROOT, Socket::new(seven, 0), Socket::new(twin, 0))
        .unwrap();
    b.document
        .connect(ROOT, Socket::new(eight, 0), Socket::new(twin, 1))
        .unwrap();

    let out = b
        .document
        .group_insert(ROOT, b.instance, &[b.two], Sharing::Fork)
        .unwrap();

    assert_eq!(out.forked_from, Some(b.definition));
    assert_ne!(out.definition, b.definition);
    assert_eq!(out.other_instances, 0);
    assert!(out.severed.is_empty());
    // The original definition is untouched, and so is the instance using it.
    assert_eq!(
        b.document
            .tree(b.definition)
            .unwrap()
            .interface()
            .inputs()
            .len(),
        2
    );
    assert_eq!(number(&b.document.evaluate(ROOT, twin)), Some(15));
    assert_eq!(
        value_into(&b.document, ROOT, Socket::new(b.sink, 0)),
        Some(Val::Number(5))
    );
    assert!(b.document.validate().is_empty());
}

#[test]
fn a_group_cannot_be_moved_into_itself() {
    let mut b = boundaried();
    let error = b
        .document
        .group_insert(ROOT, b.instance, &[b.instance], Sharing::Shared)
        .unwrap_err();
    assert_eq!(error, RepartitionError::InstanceSelected(b.instance));
    assert!(b.document.validate().is_empty());
}

#[test]
fn moving_a_group_into_a_group_it_contains_names_the_chain() {
    // `outer` holds an instance of `Sum`; moving `outer` into `Sum` would make
    // `Sum` contain `outer` contain `Sum`.
    let mut b = boundaried();
    let outer = b.document.add_definition("Outer");
    let nested = b.document.instantiate(outer, b.definition, 0, 0).unwrap();
    let _ = nested;
    let placed = b.document.instantiate(ROOT, outer, 600, 400).unwrap();

    let error = b
        .document
        .group_insert(ROOT, b.instance, &[placed], Sharing::Shared)
        .unwrap_err();
    match error {
        RepartitionError::Recursion { chain } => {
            assert_eq!(chain, vec![outer, b.definition]);
        }
        other => panic!("expected a recursion refusal, got {other}"),
    }
    assert!(b.document.validate().is_empty());
}

#[test]
fn an_inward_move_that_would_close_a_cycle_is_refused_and_changes_nothing() {
    // The group feeds `relay`, `relay` feeds `bystander`. Nothing is cyclic —
    // until `bystander` moves in, and then the group feeds itself through a node
    // that stayed outside. Blender's own test for this is one hop deep
    // (`node_group_make_test_selected`: no unselected node may have both an
    // input from the selection and an output to it), and `relay` has exactly
    // that, so this case is the one where a one-hop rule and a reachability rule
    // agree; R1577 covers the two-hop case where they do not.
    let mut b = boundaried();
    let relay = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 600, 40)
        .unwrap();
    let bystander = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 800, 40)
        .unwrap();
    b.document
        .connect(ROOT, Socket::new(b.instance, 0), Socket::new(relay, 0))
        .unwrap();
    b.document
        .connect(ROOT, Socket::new(relay, 0), Socket::new(bystander, 0))
        .unwrap();
    let before = b.document.clone();

    let error = b
        .document
        .group_insert(ROOT, b.instance, &[bystander], Sharing::Shared)
        .unwrap_err();
    match error {
        RepartitionError::Bypass { path } => {
            assert!(
                path.contains(&relay),
                "the walk should name the relay: {path:?}"
            );
            assert!(path.contains(&bystander), "{path:?}");
        }
        other => panic!("expected a bypass refusal, got {other}"),
    }
    assert_eq!(b.document, before);
}

#[test]
fn a_node_moved_out_of_a_group_keeps_its_wiring_where_blender_loses_it() {
    let mut ours = boundaried();
    let out = ours
        .document
        .group_separate(ROOT, ours.instance, &[ours.add], Sharing::Shared)
        .unwrap();

    // The node is in the host tree, fed by what fed the group and feeding what
    // the group fed. The graph still delivers 5.
    assert_eq!(out.moved.len(), 1);
    assert!(
        ours.document
            .tree(ROOT)
            .unwrap()
            .node(out.moved[0])
            .is_some()
    );
    assert_eq!(
        value_into(&ours.document, ROOT, Socket::new(ours.sink, 0)),
        Some(Val::Number(5))
    );
    // Every port is gone, because not one of them describes a crossing now.
    assert_eq!(out.unexposed.len(), 3);
    let interface = ours.document.tree(ours.definition).unwrap().interface();
    assert!(interface.is_empty());
    assert!(ours.document.validate().is_empty());

    // Blender's rule, on the same fixture: the sink is fed by a group that
    // produces nothing, and the group keeps three sockets that reach nothing.
    let mut theirs = boundaried();
    blender_separate(
        &mut theirs.document,
        ROOT,
        theirs.definition,
        theirs.instance,
        &[theirs.add],
    );
    assert_eq!(
        value_into(&theirs.document, ROOT, Socket::new(theirs.sink, 0)),
        None
    );
    let stranded = theirs.document.tree(theirs.definition).unwrap().interface();
    assert_eq!(stranded.inputs().len(), 2);
    assert_eq!(stranded.outputs().len(), 1);
}

#[test]
fn a_node_moved_out_that_still_feeds_the_group_gains_it_an_input() {
    // Group both adders, then separate the first: its result still has to reach
    // the second, so it crosses the boundary the other way now.
    let mut f = fixture();
    let doubler = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 300, 40)
        .unwrap();
    f.document
        .disconnect(
            ROOT,
            f.document
                .tree(ROOT)
                .unwrap()
                .link_into(Socket::new(f.sink, 0))
                .unwrap()
                .id,
        )
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.add, 0), Socket::new(doubler, 0))
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.add, 0), Socket::new(doubler, 1))
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(doubler, 0), Socket::new(f.sink, 0))
        .unwrap();
    let made = f.document.group(ROOT, &[f.add, doubler], "Both").unwrap();
    assert_eq!(
        value_into(&f.document, ROOT, Socket::new(f.sink, 0)),
        Some(Val::Number(10))
    );

    let out = f
        .document
        .group_separate(ROOT, made.node, &[f.add], Sharing::Shared)
        .unwrap();

    // One value now enters the group where two used to, and the arithmetic is
    // unchanged.
    assert_eq!(out.exposed.len(), 1);
    assert_eq!(out.exposed[0].side, InterfaceSide::Input);
    assert_eq!(out.unexposed.len(), 2);
    assert_eq!(
        value_into(&f.document, ROOT, Socket::new(f.sink, 0)),
        Some(Val::Number(10))
    );
    assert!(f.document.validate().is_empty());
}

#[test]
fn an_outward_move_that_would_close_a_cycle_is_refused_and_changes_nothing() {
    // Inside the definition: `head -> middle -> tail -> foot`. Separating the
    // two in the middle leaves `head` and `foot` fused into one node out in the
    // host, and the walk between them closes.
    let mut document = Document::new("root");
    let head = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(4)), 0, 0)
        .unwrap();
    let middle = document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 200, 0)
        .unwrap();
    let tail = document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 400, 0)
        .unwrap();
    let foot = document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 600, 0)
        .unwrap();
    for (from, to) in [(head, middle), (middle, tail), (tail, foot)] {
        document
            .connect(ROOT, Socket::new(from, 0), Socket::new(to, 0))
            .unwrap();
    }
    let made = document
        .group(ROOT, &[head, middle, tail, foot], "Chain")
        .unwrap();
    let before = document.clone();

    let error = document
        .group_separate(ROOT, made.node, &[middle, tail], Sharing::Shared)
        .unwrap_err();
    match error {
        RepartitionError::Bypass { path } => {
            assert!(path.contains(&middle) && path.contains(&tail), "{path:?}");
        }
        other => panic!("expected a bypass refusal, got {other}"),
    }
    assert_eq!(document, before);
}

#[test]
fn a_separate_through_a_shared_definition_names_what_it_cost_elsewhere() {
    let mut b = boundaried();
    let seven = b
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(7)), 0, 400)
        .unwrap();
    let twin = b
        .document
        .instantiate(ROOT, b.definition, 200, 440)
        .unwrap();
    b.document
        .connect(ROOT, Socket::new(seven, 0), Socket::new(twin, 0))
        .unwrap();

    let out = b
        .document
        .group_separate(ROOT, b.instance, &[b.add], Sharing::Shared)
        .unwrap();

    assert_eq!(out.other_instances, 1);
    assert_eq!(out.severed.len(), 1);
    assert_eq!(out.severed[0].tree, ROOT);
    assert_eq!(out.severed[0].link.from, Socket::new(seven, 0));
    assert!(b.document.validate().is_empty());

    // The same move on a fork touches nobody else.
    let mut other = boundaried();
    let spare = other
        .document
        .instantiate(ROOT, other.definition, 200, 440)
        .unwrap();
    let forked = other
        .document
        .group_separate(ROOT, other.instance, &[other.add], Sharing::Fork)
        .unwrap();
    assert_eq!(forked.other_instances, 0);
    assert!(forked.severed.is_empty());
    assert_eq!(
        other
            .document
            .tree(other.definition)
            .unwrap()
            .interface()
            .inputs()
            .len(),
        2
    );
    assert_eq!(number(&other.document.evaluate(ROOT, spare)), Some(0));
}

#[test]
fn an_interface_node_cannot_change_sides() {
    let mut b = boundaried();
    let entry = b
        .document
        .tree(b.definition)
        .unwrap()
        .interface_node(InterfaceSide::Input)
        .unwrap()
        .id;
    let error = b
        .document
        .group_separate(ROOT, b.instance, &[entry], Sharing::Shared)
        .unwrap_err();
    assert_eq!(error, RepartitionError::InterfaceNodeSelected(entry));
}

#[test]
fn a_boundary_move_needs_a_boundary() {
    let mut b = boundaried();
    let error = b
        .document
        .group_insert(ROOT, b.sink, &[b.two], Sharing::Shared)
        .unwrap_err();
    assert_eq!(error, RepartitionError::NotAGroup(b.sink));
    let error = b
        .document
        .group_separate(ROOT, b.instance, &[], Sharing::Shared)
        .unwrap_err();
    assert_eq!(error, RepartitionError::Empty);
}

#[test]
fn forking_a_definition_leaves_the_original_and_its_other_users_alone() {
    let mut b = boundaried();
    let twin = b
        .document
        .instantiate(ROOT, b.definition, 200, 440)
        .unwrap();
    let copy = b.document.fork_definition(ROOT, b.instance).unwrap();

    assert_ne!(copy, b.definition);
    assert_eq!(b.document.instance_count(b.definition), 1);
    assert_eq!(b.document.instance_count(copy), 1);
    // A name is not an identity here, so the copy keeps it. Blender must rename,
    // because an ID's name IS its key.
    assert_eq!(
        b.document.tree(copy).unwrap().name,
        b.document.tree(b.definition).unwrap().name
    );
    // Editing the copy leaves the original's users untouched.
    b.document
        .group_insert(ROOT, b.instance, &[b.two], Sharing::Shared)
        .unwrap();
    assert_eq!(
        b.document
            .tree(b.definition)
            .unwrap()
            .interface()
            .inputs()
            .len(),
        2
    );
    assert_eq!(b.document.signature(ROOT, twin).unwrap().inputs.len(), 2);
    assert!(b.document.validate().is_empty());
}

#[test]
fn a_dropped_link_is_named_with_the_tree_it_was_in() {
    let mut b = boundaried();
    let dropped = b
        .document
        .unexpose(b.definition, InterfaceSide::Input, 0)
        .unwrap();
    // One inside the definition, one at the instance out in the root.
    let trees: Vec<TreeId> = dropped.iter().map(|d| d.tree).collect();
    assert!(trees.contains(&ROOT));
    assert!(trees.contains(&b.definition));
    assert_eq!(dropped.len(), 2);
}

#[test]
fn a_link_naming_a_missing_node_is_refused_rather_than_crashing() {
    // A document that arrived from a file can hold one; `validate` is what
    // reports it, and every derivation now refuses rather than indexing a map
    // that has no such key.
    let mut f = fixture();
    let ghost = f.document.push_link(
        ROOT,
        Socket::new(NodeId(97), 0),
        Socket::new(f.sink, 0),
        false,
    );
    assert!(matches!(
        f.document.validate().first(),
        Some(Violation::DanglingLink { .. })
    ));
    assert_eq!(
        f.document.group(ROOT, &[f.add], "Sum").unwrap_err(),
        GroupError::Malformed { link: ghost }
    );
    assert_eq!(
        f.document.extract(ROOT, &[f.add]).unwrap_err(),
        ExtractError::Malformed { link: ghost }
    );
}

// ------------------------------------------------------------------- R1586
// A node says how it takes part, and only one of those facts is the meaning.

/// Blender's own rule for which input feeds a muted node's output, held here so
/// the divergence is an **assertion** rather than a description.
///
/// `find_internally_linked_input` (`node_tree_update.cc`, `8cf50599`) scans the
/// inputs per output, keeping the best by a static table of socket-type pairs
/// and breaking ties by whether the input happens to be **wired**. Our taxonomy
/// has no implicit conversions, so the table reduces to its diagonal; the
/// tie-break is the part that carries meaning, and it is reproduced exactly.
fn blender_internal_link(
    document: &Document<Op>,
    tree: TreeId,
    node: NodeId,
    output: u32,
) -> Option<u32> {
    let signature = document.signature(tree, node)?;
    let host = document.tree(tree)?;
    let out_ty = signature.outputs.get(output as usize)?.ty;
    let mut selected: Option<u32> = None;
    let mut selected_priority = -1_i32;
    let mut selected_is_linked = false;
    for (index, input) in signature.inputs.iter().enumerate() {
        let port = u32::try_from(index).unwrap_or(u32::MAX);
        let priority = if input.ty == out_ty { 4 } else { -1 };
        if priority < 0 {
            continue;
        }
        let is_linked = host.link_into(Socket::new(node, port)).is_some();
        if !(priority > selected_priority || (is_linked && !selected_is_linked)) {
            continue;
        }
        selected = Some(port);
        selected_priority = priority;
        selected_is_linked = is_linked;
    }
    selected
}

#[test]
fn a_bypassed_node_is_the_identity_as_far_as_its_signature_allows() {
    let f = fixture();
    // Add: two Number inputs, one Number output. Output 0 takes input 0.
    let through = f.document.passthrough(ROOT, f.add).unwrap();
    assert_eq!(
        through.routes(),
        &[Route {
            output: 0,
            input: 0
        }]
    );
    assert!(through.is_identity(), "same index, agreeing types");
    assert_eq!(through.dropped_outputs(), &[] as &[u32]);
    assert_eq!(
        through.unreached_inputs(),
        &[1],
        "the addend reaches no output and is named as such"
    );
}

#[test]
fn one_input_can_feed_several_outputs_and_the_second_is_not_the_identity() {
    let mut document = Document::new("root");
    let split = document
        .add_node(ROOT, NodeBody::Kind(Op::Split), 0, 0)
        .unwrap();
    let through = document.passthrough(ROOT, split).unwrap();
    assert_eq!(
        through.routes(),
        &[
            Route {
                output: 0,
                input: 0
            },
            Route {
                output: 1,
                input: 0
            },
        ],
        "one value in, the same value out of both halves"
    );
    assert!(
        !through.is_identity(),
        "output 1 has no input 1 to be the identity of"
    );
    assert!(through.unreached_inputs().is_empty());
}

#[test]
fn an_output_no_input_can_feed_is_named_rather_than_silently_empty() {
    let mut document = Document::new("root");
    let measure = document
        .add_node(ROOT, NodeBody::Kind(Op::Measure), 0, 0)
        .unwrap();
    let source = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(7)), 0, 80)
        .unwrap();
    let through = document.passthrough(ROOT, measure).unwrap();
    assert!(
        through.routes().is_empty(),
        "Text in cannot become Number out"
    );
    assert_eq!(through.dropped_outputs(), &[0]);
    assert_eq!(through.unreached_inputs(), &[0]);
    // A source node is the other way in to the same fact: nothing to pass.
    let through = document.passthrough(ROOT, source).unwrap();
    assert_eq!(through.dropped_outputs(), &[0]);
}

#[test]
fn the_route_is_a_function_of_the_signature_where_blenders_reads_the_wiring() {
    let mut f = fixture();
    // Both inputs wired: the two rules agree.
    assert_eq!(
        f.document.passthrough(ROOT, f.add).unwrap().source_of(0),
        Some(0)
    );
    assert_eq!(blender_internal_link(&f.document, ROOT, f.add, 0), Some(0));

    // Unwire the FIRST input and change nothing else about the node.
    let feed = f
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.add, 0))
        .unwrap()
        .id;
    f.document.disconnect(ROOT, feed).unwrap();

    assert_eq!(
        f.document.passthrough(ROOT, f.add).unwrap().source_of(0),
        Some(0),
        "unchanged: the routing is a property of the signature alone"
    );
    assert_eq!(
        blender_internal_link(&f.document, ROOT, f.add, 0),
        Some(1),
        "Blender's linked-tie-break re-routes a DIFFERENT port's value \
         because this one was unplugged"
    );
}

#[test]
fn the_pass_through_is_derived_and_not_stored() {
    let mut f = fixture();
    // Blender materialises this into `node->runtime->internal_links` and keeps a
    // tree-update pass to notice when the stored answer has gone stale. Here the
    // answer follows an edit with nothing asked to refresh it.
    let split = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Split), 300, 200)
        .unwrap();
    assert_eq!(
        f.document.passthrough(ROOT, split).unwrap().routes().len(),
        2
    );
    f.document.remove_node(ROOT, split).unwrap();
    assert!(
        f.document.passthrough(ROOT, split).is_none(),
        "no cached answer survives the node it described"
    );
}

#[test]
fn a_bypassed_node_does_not_compute_and_passes_its_input_on() {
    let mut f = fixture();
    let sink_in = Socket::new(f.sink, 0);
    assert_eq!(
        f.document.evaluator().input(ROOT, sink_in),
        Some(Val::Number(5)),
        "2 + 3 while the adder is doing its job"
    );
    f.document.set_bypassed(ROOT, f.add, true).unwrap();
    assert_eq!(
        f.document.evaluator().input(ROOT, sink_in),
        Some(Val::Number(2)),
        "bypassed: the augend passes straight through, unadded"
    );
    assert!(
        f.document.set_bypassed(ROOT, f.add, false).unwrap(),
        "the verb answers what it was"
    );
    assert_eq!(
        f.document.evaluator().input(ROOT, sink_in),
        Some(Val::Number(5))
    );
}

#[test]
fn bypassing_and_dissolving_agree_on_every_value_when_the_route_is_wired() {
    let sink_in = |f: &Fixture| Socket::new(f.sink, 0);

    let mut bypassed = fixture();
    bypassed
        .document
        .set_bypassed(ROOT, bypassed.add, true)
        .unwrap();
    let through_bypass = bypassed
        .document
        .evaluator()
        .input(ROOT, sink_in(&bypassed));

    let mut dissolved = fixture();
    let rewired = dissolved.document.dissolve(ROOT, dissolved.add).unwrap();
    let through_dissolve = dissolved
        .document
        .evaluator()
        .input(ROOT, sink_in(&dissolved));

    assert_eq!(through_bypass, Some(Val::Number(2)));
    assert_eq!(
        through_bypass, through_dissolve,
        "one derivation, so the non-destructive and destructive forms agree"
    );
    assert!(rewired.lossless(), "nothing was reported lost");
    assert_eq!(rewired.bridged.len(), 1);
    assert_eq!(rewired.bridged[0].from, Socket::new(dissolved.two, 0));
    assert_eq!(rewired.bridged[0].to, Socket::new(dissolved.sink, 0));
    assert_eq!(
        rewired.removed.len(),
        3,
        "both feeds and the outgoing link touched the node"
    );
    assert!(dissolved.document.validate().is_empty());
}

#[test]
fn where_the_two_cannot_agree_the_dissolve_names_the_difference() {
    // The routed input is UNWIRED, so a bypass passes its declared default on
    // and a dissolve has no link to redirect. Blender removes exactly the same
    // link and reports nothing at all.
    let unwire = |f: &mut Fixture| {
        let feed = f
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(f.add, 0))
            .unwrap()
            .id;
        f.document.disconnect(ROOT, feed).unwrap();
    };

    let mut bypassed = fixture();
    unwire(&mut bypassed);
    bypassed
        .document
        .set_bypassed(ROOT, bypassed.add, true)
        .unwrap();
    assert_eq!(
        bypassed
            .document
            .evaluator()
            .input(ROOT, Socket::new(bypassed.sink, 0)),
        Some(Val::Number(0)),
        "the augend's own declared default passes through"
    );

    let mut dissolved = fixture();
    unwire(&mut dissolved);
    let rewired = dissolved.document.dissolve(ROOT, dissolved.add).unwrap();
    assert!(rewired.bridged.is_empty());
    assert_eq!(
        rewired.severed.len(),
        1,
        "the link no value reaches is NAMED, not merely gone"
    );
    assert_eq!(rewired.severed[0].to, Socket::new(dissolved.sink, 0));
    assert!(!rewired.lossless());
    assert_eq!(
        dissolved
            .document
            .evaluator()
            .input(ROOT, Socket::new(dissolved.sink, 0)),
        None,
        "the sink falls back to its own port, which declares no default"
    );
}

#[test]
fn a_dissolve_bridges_at_any_arity_not_only_one_in_one_out() {
    let mut f = fixture();
    // `add` has TWO inputs and one output — the case a one-in-one-out reroute
    // rule (this tree's own `hello-node-editor` predicate) refuses outright.
    assert_eq!(f.document.tree(ROOT).unwrap().links().len(), 3);
    let rewired = f.document.dissolve(ROOT, f.add).unwrap();
    assert_eq!(rewired.bridged.len(), 1);
    let links = f.document.tree(ROOT).unwrap().links();
    assert_eq!(links.len(), 1, "three incident links became one bridge");
    assert_eq!(links[0].from, Socket::new(f.two, 0));
    assert!(f.document.tree(ROOT).unwrap().node(f.add).is_none());
    assert!(f.document.tree(ROOT).unwrap().node(f.three).is_some());
}

#[test]
fn a_detach_rewires_around_the_node_and_leaves_it_there() {
    let mut f = fixture();
    let rewired = f.document.detach(ROOT, f.add).unwrap();
    assert_eq!(rewired.bridged.len(), 1);
    let host = f.document.tree(ROOT).unwrap();
    assert!(
        host.node(f.add).is_some(),
        "the node stays; only its wiring goes"
    );
    assert!(
        host.links()
            .iter()
            .all(|l| l.from.node != f.add && l.to.node != f.add),
        "and it is wired to nothing"
    );
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        Some(Val::Number(2)),
        "the same value a bypass would pass"
    );
    assert!(f.document.validate().is_empty());
}

#[test]
fn a_muted_link_keeps_its_place_and_carries_nothing() {
    let mut f = fixture();
    let feed = f
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.add, 0))
        .unwrap()
        .id;
    assert!(!f.document.set_link_muted(ROOT, feed, true).unwrap());
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        Some(Val::Number(3)),
        "the augend falls back to its declared 0, so 0 + 3"
    );
    let host = f.document.tree(ROOT).unwrap();
    assert_eq!(host.links().len(), 3, "the wire is still there");
    assert!(
        host.link_into(Socket::new(f.add, 0)).is_some(),
        "and still occupies the input, so nothing else may be wired to it"
    );
    // Which is why a second wire into that input still DISPLACES the muted one.
    let other = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Num(9)), 0, 300)
        .unwrap();
    let connected = f
        .document
        .connect(ROOT, Socket::new(other, 0), Socket::new(f.add, 0))
        .unwrap();
    assert_eq!(connected.displaced.map(|l| l.id), Some(feed));
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        Some(Val::Number(12)),
    );
}

#[test]
fn a_bridge_is_muted_when_either_link_it_replaces_was() {
    for (upstream, downstream) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut f = fixture();
        let feed = f
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(f.add, 0))
            .unwrap()
            .id;
        let out = f
            .document
            .tree(ROOT)
            .unwrap()
            .link_into(Socket::new(f.sink, 0))
            .unwrap()
            .id;
        f.document.set_link_muted(ROOT, feed, upstream).unwrap();
        f.document.set_link_muted(ROOT, out, downstream).unwrap();
        let rewired = f.document.dissolve(ROOT, f.add).unwrap();
        assert_eq!(
            rewired.bridged[0].muted,
            upstream || downstream,
            "a value being stopped goes on being stopped ({upstream}, {downstream})"
        );
    }
}

#[test]
fn a_bypassed_group_instance_is_not_descended_into() {
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let sink_in = Socket::new(f.sink, 0);
    assert_eq!(
        f.document.evaluator().input(ROOT, sink_in),
        Some(Val::Number(5))
    );

    // The instance's signature IS the derived interface, so the same rule that
    // routes an application node routes this one: output 0 from input 0.
    let through = f.document.passthrough(ROOT, made.node).unwrap();
    assert_eq!(
        through.routes(),
        &[Route {
            output: 0,
            input: 0
        }]
    );

    f.document.set_bypassed(ROOT, made.node, true).unwrap();
    assert_eq!(
        f.document.evaluator().input(ROOT, sink_in),
        Some(Val::Number(2)),
        "bypassing a group is the request NOT to run what is inside it"
    );
}

#[test]
fn mutedness_survives_a_collapse_and_an_inline() {
    let mut f = fixture();
    let feed = f
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.add, 0))
        .unwrap()
        .id;
    f.document.set_link_muted(ROOT, feed, true).unwrap();
    let before = f.document.evaluator().input(ROOT, Socket::new(f.sink, 0));
    assert_eq!(before, Some(Val::Number(3)));

    // A crossing becomes two links; only the per-consumer half carries the fact.
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        before,
        "the graph goes on computing what it computed"
    );

    // And the inline joins them back into one link that is muted again.
    f.document.ungroup(ROOT, made.node).unwrap();
    let host = f.document.tree(ROOT).unwrap();
    let muted: Vec<_> = host.links().iter().filter(|l| l.muted).collect();
    assert_eq!(muted.len(), 1, "one muted wire in, one muted wire out");
    let sink = host
        .nodes()
        .find(|n| n.body == NodeBody::Kind(Op::Sink))
        .unwrap()
        .id;
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(sink, 0)),
        before
    );
}

#[test]
fn mutedness_survives_a_cut_and_a_paste() {
    let mut f = fixture();
    let feed = f
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.add, 0))
        .unwrap()
        .id;
    f.document.set_link_muted(ROOT, feed, true).unwrap();

    let cut = f.document.extract(ROOT, &[f.two, f.add]).unwrap();
    assert_eq!(
        cut.document()
            .tree(ROOT)
            .unwrap()
            .links()
            .iter()
            .filter(|l| l.muted)
            .count(),
        1,
        "an internal link keeps the fact"
    );

    // And an inbound crossing that was muted is recorded, so re-attaching it
    // puts back a wire that stops the value rather than one that carries it.
    let cut = f.document.extract(ROOT, &[f.add]).unwrap();
    let inbound: Vec<&Severed> = cut.inbound().iter().collect();
    let muted_crossings: usize = inbound.iter().map(|s| s.muted_consumers().len()).sum();
    assert_eq!(muted_crossings, 1);
    let landed = f
        .document
        .insert(
            ROOT,
            &cut,
            (500, 500),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(landed.reattached.len(), 2, "both crossings came back");
    let host = f.document.tree(ROOT).unwrap();
    let pasted = landed.nodes[0];
    assert!(
        host.link_into(Socket::new(pasted, 0)).unwrap().muted,
        "the re-attached wire is muted, as the one it reproduces was"
    );
    assert!(!host.link_into(Socket::new(pasted, 1)).unwrap().muted);
}

#[test]
fn mutedness_survives_a_boundary_move_in_both_directions() {
    let mut f = fixture();
    let feed = f
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.add, 0))
        .unwrap()
        .id;
    f.document.set_link_muted(ROOT, feed, true).unwrap();
    let before = f.document.evaluator().input(ROOT, Socket::new(f.sink, 0));

    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    // Move the two-producer INTO the group, then back out again.
    let inserted = f
        .document
        .group_insert(ROOT, made.node, &[f.two], Sharing::Shared)
        .unwrap();
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        before,
        "moving the boundary did not un-stop a stopped value"
    );
    let moved = inserted.moved[0];
    f.document
        .group_separate(ROOT, made.node, &[moved], Sharing::Shared)
        .unwrap();
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        before,
        "nor did moving it back"
    );
}

#[test]
fn a_bypass_and_a_look_travel_with_the_node() {
    let mut f = fixture();
    {
        let node = f.document.tree_mut(ROOT).unwrap().node_mut(f.add).unwrap();
        node.bypassed = true;
        node.appearance.collapsed = true;
        node.appearance.width = Some(140);
        node.label = Some("bypassed adder".to_owned());
    }

    // Through a collapse and an inline, which mints a fresh id in another tree.
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let inside = f
        .document
        .tree(made.definition)
        .unwrap()
        .nodes()
        .find(|n| matches!(n.body, NodeBody::Kind(Op::Add)))
        .unwrap();
    assert!(
        inside.bypassed,
        "a bypass is not a property of the tree it is in"
    );
    assert!(inside.appearance.collapsed);
    assert_eq!(inside.appearance.width, Some(140));

    let inlined = f.document.ungroup(ROOT, made.node).unwrap();
    let back = f
        .document
        .tree(ROOT)
        .unwrap()
        .node(inlined.nodes[0])
        .unwrap();
    assert!(back.bypassed);
    assert!(back.appearance.collapsed);
    assert_eq!(back.label.as_deref(), Some("bypassed adder"));

    // And through a cut and a paste.
    let cut = f.document.extract(ROOT, &[inlined.nodes[0]]).unwrap();
    let landed = f
        .document
        .insert(ROOT, &cut, (900, 900), Crossings::Drop, Definitions::Share)
        .unwrap();
    let copy = f
        .document
        .tree(ROOT)
        .unwrap()
        .node(landed.nodes[0])
        .unwrap();
    assert!(copy.bypassed);
    assert_eq!(copy.appearance.width, Some(140));
}

#[test]
fn what_a_node_looks_like_cannot_change_what_the_graph_computes() {
    let baseline = fixture().document.evaluate(ROOT, fixture().add);
    // Every appearance field, one at a time and then all together. Blender keeps
    // all of these in the same `flag` integer as `NODE_MUTED`, so which of them
    // its evaluator may read is not stated anywhere in its model.
    let looks: [Appearance; 5] = [
        Appearance {
            collapsed: true,
            ..Appearance::default()
        },
        Appearance {
            hide_unused_ports: true,
            ..Appearance::default()
        },
        Appearance {
            show_options: false,
            ..Appearance::default()
        },
        Appearance {
            show_preview: true,
            ..Appearance::default()
        },
        Appearance {
            collapsed: true,
            hide_unused_ports: true,
            show_options: false,
            show_preview: true,
            width: Some(1),
        },
    ];
    for look in looks {
        let mut f = fixture();
        for id in [f.two, f.three, f.add, f.sink] {
            f.document
                .tree_mut(ROOT)
                .unwrap()
                .node_mut(id)
                .unwrap()
                .appearance = look.clone();
        }
        assert_eq!(f.document.evaluate(ROOT, f.add), baseline);
        assert_eq!(
            f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
            Some(Val::Number(5))
        );
    }
}

#[test]
fn hiding_unused_ports_hides_only_the_unwired_ones_and_names_them() {
    let mut f = fixture();
    let all = f.document.visible_ports(ROOT, f.add).unwrap();
    assert_eq!(all.inputs, vec![0, 1]);
    assert_eq!(all.outputs, vec![0]);
    assert_eq!(all.hidden_count(), 0);

    // Free the addend, then ask the node to hide what is unused.
    let feed = f
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.add, 1))
        .unwrap()
        .id;
    f.document.disconnect(ROOT, feed).unwrap();
    f.document
        .tree_mut(ROOT)
        .unwrap()
        .node_mut(f.add)
        .unwrap()
        .appearance
        .hide_unused_ports = true;

    let some = f.document.visible_ports(ROOT, f.add).unwrap();
    assert_eq!(some.inputs, vec![0]);
    assert_eq!(some.hidden_inputs, vec![1]);
    assert_eq!(some.outputs, vec![0], "the output is wired to the sink");

    // A muted link still counts as wired: the value is stopped, the wire is not.
    let feed = f
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.add, 0))
        .unwrap()
        .id;
    f.document.set_link_muted(ROOT, feed, true).unwrap();
    assert_eq!(
        f.document.visible_ports(ROOT, f.add).unwrap().inputs,
        vec![0]
    );
}

#[test]
fn a_bypass_chain_passes_a_value_through_every_hop() {
    let mut f = fixture();
    let first = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Split), 250, 0)
        .unwrap();
    let second = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Split), 300, 0)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.two, 0), Socket::new(first, 0))
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(first, 0), Socket::new(second, 0))
        .unwrap();
    for id in [first, second] {
        f.document.set_bypassed(ROOT, id, true).unwrap();
    }
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(second, 0)),
        Some(Val::Number(2))
    );
    assert_eq!(
        f.document.evaluate(ROOT, second),
        vec![Some(Val::Number(2)), Some(Val::Number(2))],
        "both outputs of the second hop carry the original value"
    );
}

#[test]
fn a_document_that_arrived_from_elsewhere_cannot_be_dissolved_into_a_self_link() {
    // `connect` refuses a cycle, so a 2-cycle can only arrive from outside. A
    // bridge across it would land on its own producer; the value has nowhere to
    // go, so the link is severed like any other nothing reaches.
    let mut document = Document::new("root");
    let a = document
        .add_node(ROOT, NodeBody::Kind(Op::Split), 0, 0)
        .unwrap();
    let b = document
        .add_node(ROOT, NodeBody::Kind(Op::Split), 100, 0)
        .unwrap();
    document
        .connect(ROOT, Socket::new(a, 0), Socket::new(b, 0))
        .unwrap();
    assert!(
        document
            .connect(ROOT, Socket::new(b, 0), Socket::new(a, 0))
            .is_err()
    );
    document.push_link(ROOT, Socket::new(b, 0), Socket::new(a, 0), false);

    let rewired = document.dissolve(ROOT, b).unwrap();
    assert!(
        rewired.bridged.is_empty(),
        "no bridge from a socket to itself"
    );
    assert_eq!(rewired.severed.len(), 1);
    let host = document.tree(ROOT).unwrap();
    assert!(host.links().is_empty());
    assert!(host.nodes().all(|n| n.id != b));
}

#[test]
fn bypassing_a_node_that_is_not_there_is_refused_by_name() {
    let mut f = fixture();
    assert!(f.document.set_bypassed(ROOT, NodeId(97), true).is_err());
    assert!(f.document.set_bypassed(TreeId(9), f.add, true).is_err());
    assert!(f.document.dissolve(ROOT, NodeId(97)).is_err());
    assert!(f.document.detach(TreeId(9), f.add).is_err());
    assert!(f.document.passthrough(ROOT, NodeId(97)).is_none());
    assert!(f.document.visible_ports(ROOT, NodeId(97)).is_none());
}

#[test]
fn the_identity_rule_is_falsifiable_where_position_and_type_order_disagree() {
    // Swap has two Number inputs and two Number outputs. "Lowest input of the
    // right type" would send BOTH outputs to input 0; the identity sends each
    // output to the input at its own index. This is the only shape that tells
    // the two rules apart, so without it the rule would be untested rather than
    // merely unstated.
    let mut document = Document::new("root");
    let left = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(10)), 0, 0)
        .unwrap();
    let right = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(20)), 0, 80)
        .unwrap();
    let swap = document
        .add_node(ROOT, NodeBody::Kind(Op::Swap), 200, 40)
        .unwrap();
    document
        .connect(ROOT, Socket::new(left, 0), Socket::new(swap, 0))
        .unwrap();
    document
        .connect(ROOT, Socket::new(right, 0), Socket::new(swap, 1))
        .unwrap();

    let through = document.passthrough(ROOT, swap).unwrap();
    assert_eq!(
        through.routes(),
        &[
            Route {
                output: 0,
                input: 0
            },
            Route {
                output: 1,
                input: 1
            },
        ]
    );
    assert!(through.is_identity());
    assert!(through.unreached_inputs().is_empty());

    assert_eq!(
        document.evaluate(ROOT, swap),
        vec![Some(Val::Number(20)), Some(Val::Number(10))],
        "computing, it exchanges them"
    );
    document.set_bypassed(ROOT, swap, true).unwrap();
    assert_eq!(
        document.evaluate(ROOT, swap),
        vec![Some(Val::Number(10)), Some(Val::Number(20))],
        "bypassed, it is the identity — which is what bypassing MEANS"
    );

    // Blender's rule gives output 1 the FIRST input, because its type-pair
    // priority is equal for both and its tie-break is linked-ness, which both
    // satisfy. So a bypassed Swap there emits 10 twice and the right-hand value
    // vanishes.
    assert_eq!(blender_internal_link(&document, ROOT, swap, 1), Some(0));
}

#[test]
fn a_link_between_two_moved_nodes_keeps_its_mutedness_in_both_directions() {
    // The crossing case and the CARRIED case are different code paths, and only
    // this one has both ends of the link among the nodes changing sides. A test
    // that moves a source node exercises crossings alone and would pass with the
    // carried path broken — which is how this test came to exist.
    let mut f = fixture();
    let feed = f
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(f.add, 0))
        .unwrap()
        .id;
    f.document.set_link_muted(ROOT, feed, true).unwrap();

    // Group the SINK, so `two` and `add` are both outside it and the wire
    // between them moves whole.
    let made = f.document.group(ROOT, &[f.sink], "End").unwrap();
    let inserted = f
        .document
        .group_insert(ROOT, made.node, &[f.two, f.add], Sharing::Shared)
        .unwrap();
    let inside = f.document.tree(made.definition).unwrap();
    assert_eq!(
        inside.links().iter().filter(|l| l.muted).count(),
        1,
        "the carried link kept the fact on the way in"
    );

    let moved = inserted.moved.clone();
    f.document
        .group_separate(ROOT, made.node, &moved, Sharing::Shared)
        .unwrap();
    assert_eq!(
        f.document
            .tree(ROOT)
            .unwrap()
            .links()
            .iter()
            .filter(|l| l.muted)
            .count(),
        1,
        "and on the way back out"
    );
    assert!(f.document.validate().is_empty());
}
#[test]
fn dissolving_an_interface_node_severs_and_says_so() {
    // `group` and `group_insert` REFUSE an interface node in their selection,
    // because it projects the tree rather than being content and so cannot
    // change sides. Deleting one is a different question and is legal — a
    // definition with no inside-output node is a legal, empty one, and `expose`
    // makes another. So `dissolve` treats it like any other node, which means
    // the honest thing to pin is that it degrades rather than corrupts.
    let mut f = fixture();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let entry = f
        .document
        .tree(made.definition)
        .unwrap()
        .interface_node(InterfaceSide::Input)
        .unwrap()
        .id;
    assert!(matches!(
        f.document.group(made.definition, &[entry], "x"),
        Err(GroupError::InterfaceNodeSelected(_))
    ));

    let out = f.document.dissolve(made.definition, entry).unwrap();
    assert!(
        out.bridged.is_empty(),
        "it has no inputs, so nothing passes"
    );
    assert_eq!(out.severed.len(), 2, "and both values it fed are NAMED");
    assert!(!out.lossless());
    assert!(
        f.document.validate().is_empty(),
        "the document still satisfies every invariant"
    );
    assert_eq!(
        f.document.evaluator().input(ROOT, Socket::new(f.sink, 0)),
        Some(Val::Number(0)),
        "and the graph degrades to the ports' declared defaults"
    );
}
