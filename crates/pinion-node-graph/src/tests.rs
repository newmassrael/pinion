//! R1577 — the crate's own adversarial fixtures.
//!
//! Every one of these is a hand-written document: no renderer, no window, no
//! pointer. That is the property the crate exists to have.

use serde::{Deserialize, Serialize};

use crate::{
    Appearance, ConnectError, Conversion, Crossings, Definitions, Document, DuplicateError,
    EditPath, ExtractError, Fragment, GroupError, Grow, InsertError, InterfaceSide, NestError,
    Node, NodeBody, NodeId, NodeKind, Orphaned, ParentError, PathError, Port, ROOT, Reach,
    RepartitionError, Route, SelectError, Severed, Sharing, Socket, TreeId, UngroupError,
    Violation,
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
    /// A limit and a value, both numbers — the shape whose control input shares
    /// the data type, so the bare identity rule would pass the LIMIT through.
    Gate,
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
            Self::Gate => "Gate",
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
            Self::Gate => vec![
                Port::new("Limit", Ty::Number).no_passthrough(),
                Port::new("Value", Ty::Number),
            ],
            Self::Swap => vec![
                Port::new("Left", Ty::Number).with_default(Val::Number(-1)),
                Port::new("Right", Ty::Number).with_default(Val::Number(-2)),
            ],
            Self::Sink => vec![Port::new("Result", Ty::Number)],
        }
    }

    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        match self {
            Self::Num(_) | Self::Add | Self::Measure | Self::Gate => {
                vec![Port::new("Out", Ty::Number)]
            }
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
            Self::Gate => vec![number(0).zip(number(1)).map(|(l, v)| Val::Number(v.min(l)))],
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
    assert_eq!(dropped.links.len(), 3);
    assert!(dropped.adopted.is_empty(), "it contained nothing");
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
            input: 0,
            converts: false,
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
                input: 0,
                converts: false,
            },
            Route {
                output: 1,
                input: 0,
                converts: false,
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
            input: 0,
            converts: false,
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
                input: 0,
                converts: false,
            },
            Route {
                output: 1,
                input: 1,
                converts: false,
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

// ------------------------------------------------------------------- R1587
// A port says whether a value passes through it.

/// A Blender node type that registers `internally_linked_input`, reduced to the
/// shape its callback computes.
///
/// Censused from `~/blender-ref` at `8cf50599`:
/// `grep -rln "ntype.internally_linked_input = " source/blender/` answers
/// **eleven** node types, and between them their callbacks compute exactly
/// **three** things. Holding the census as a TABLE rather than a paragraph is
/// what makes "our default already produces this" an assertion.
///
/// Eleven rows and three shapes, so this table's strength is three distinct
/// derivations rather than eleven — which is why the shape counts are asserted
/// too, and why the pairing (not just output 0) is what each row checks.
enum BlenderHook {
    /// `input_by_identifier(output_socket.identifier())` — seven of the twelve,
    /// under the comment "Internal links should always map corresponding input
    /// and output sockets". The identity, matched by name.
    CorrespondingSocket,
    /// `input_socket(output_socket.index())` — `node_geo_attribute_capture`.
    /// The identity, matched by index.
    SameIndex,
    /// `input_socket(1)` — the three switches, skipping a leading control
    /// input.
    FirstDataInput,
}

/// Every implementor, with the shape it computes and the port index its
/// callback answers with for output 0.
const BLENDER_HOOKS: &[(&str, BlenderHook, Option<u32>)] = &[
    ("node_geo_switch", BlenderHook::FirstDataInput, Some(1)),
    (
        "node_geo_index_switch",
        BlenderHook::FirstDataInput,
        Some(1),
    ),
    ("node_geo_menu_switch", BlenderHook::FirstDataInput, Some(1)),
    (
        "node_geo_attribute_capture",
        BlenderHook::SameIndex,
        Some(0),
    ),
    ("node_geo_bake", BlenderHook::CorrespondingSocket, Some(0)),
    (
        "node_geo_closure_to_list",
        BlenderHook::CorrespondingSocket,
        Some(0),
    ),
    (
        "node_geo_enable_output",
        BlenderHook::CorrespondingSocket,
        Some(0),
    ),
    (
        "node_geo_evaluate_closure",
        BlenderHook::CorrespondingSocket,
        Some(0),
    ),
    (
        "node_geo_field_to_grid",
        BlenderHook::CorrespondingSocket,
        Some(0),
    ),
    (
        "node_geo_field_to_list",
        BlenderHook::CorrespondingSocket,
        Some(0),
    ),
    (
        "node_geo_grid_advect",
        BlenderHook::CorrespondingSocket,
        Some(0),
    ),
];

/// Blender's OTHER mechanism, censused after the first pass got it wrong
/// (R1587.1): `(declarations, on outputs, on inputs)`.
///
/// The per-port form is *read* as `no_mute_links` and *set* through a builder
/// spelled `no_muted_links`, so a grep for the field name finds the two read
/// sites and none of the users — which is how this round briefly recorded that
/// no node type declares it. Re-censused at `8cf50599` with the builder's
/// spelling:
///
/// ```text
/// grep -rhoc "no_muted_links(" source/blender/nodes/   # 42, in 17 files
/// ```
///
/// Both ends are used, which is why one field read from both ends is the shape
/// this crate ships. The eleven per-node callbacks are the mechanism that has
/// nothing left to say here, not this one.
const NO_MUTED_LINKS: (usize, usize, usize) = (42, 28, 14);

/// A node built to order, so a Blender shape can be reproduced as a signature.
#[derive(Clone, PartialEq, Debug)]
struct Shaped {
    ins: Vec<(&'static str, Ty, bool)>,
    outs: Vec<(&'static str, Ty, bool)>,
}

impl NodeKind for Shaped {
    type Type = Ty;
    type Value = Val;
    fn name(&self) -> String {
        "Shaped".to_owned()
    }
    fn inputs(&self) -> Vec<Port<Ty, Val>> {
        self.ins
            .iter()
            .map(|&(n, t, through)| {
                let port = Port::new(n, t);
                if through { port } else { port.no_passthrough() }
            })
            .collect()
    }
    fn outputs(&self) -> Vec<Port<Ty, Val>> {
        self.outs
            .iter()
            .map(|&(n, t, through)| {
                let port = Port::new(n, t);
                if through { port } else { port.no_passthrough() }
            })
            .collect()
    }
    fn evaluate(&self, _inputs: &[Option<Val>]) -> Vec<Option<Val>> {
        vec![None; self.outs.len()]
    }
}

fn shaped(
    ins: &[(&'static str, Ty, bool)],
    outs: &[(&'static str, Ty, bool)],
) -> (Document<Shaped>, NodeId) {
    let mut document = Document::new("root");
    let node = document
        .add_node(
            ROOT,
            NodeBody::Kind(Shaped {
                ins: ins.to_vec(),
                outs: outs.to_vec(),
            }),
            0,
            0,
        )
        .unwrap();
    (document, node)
}

#[test]
fn our_default_reproduces_every_blender_pass_through_hook() {
    // The three shapes, as signatures. `true` = the port takes part.
    for &(name, ref shape, expected) in BLENDER_HOOKS {
        let (ins, outs): (Vec<_>, Vec<_>) = match shape {
            // Switch(Switch: Amount, False: Colour, True: Colour) -> Colour.
            // The control input's TYPE differs from the data, which is the
            // ordinary case, and our rule skips it for free.
            BlenderHook::FirstDataInput => (
                vec![
                    ("Switch", Ty::Number, true),
                    ("False", Ty::Text, true),
                    ("True", Ty::Text, true),
                ],
                vec![("Output", Ty::Text, true)],
            ),
            // Capture(Geometry, Value) -> (Geometry, Attribute), paired by
            // index.
            BlenderHook::SameIndex => (
                vec![("Geometry", Ty::Text, true), ("Value", Ty::Number, true)],
                vec![
                    ("Geometry", Ty::Text, true),
                    ("Attribute", Ty::Number, true),
                ],
            ),
            // The same pairing reached by NAME in Blender. Our rule reaches it
            // by index, and the two agree because a node that pairs its sockets
            // declares them in one order — which is free to do here.
            BlenderHook::CorrespondingSocket => (
                vec![("Geometry", Ty::Text, true), ("Count", Ty::Number, true)],
                vec![("Geometry", Ty::Text, true), ("Count", Ty::Number, true)],
            ),
        };
        let (document, node) = shaped(&ins, &outs);
        let through = document.passthrough(ROOT, node).unwrap();
        assert_eq!(
            through.source_of(0),
            expected,
            "{name}: our default must answer what its callback computes"
        );
        // The PAIRING, not just output 0 — otherwise a rule that sent every
        // output to one input would satisfy two of the three shapes.
        if matches!(
            shape,
            BlenderHook::SameIndex | BlenderHook::CorrespondingSocket
        ) {
            assert_eq!(
                through.source_of(1),
                Some(1),
                "{name}: the second pair is paired too"
            );
            assert!(through.is_identity(), "{name}: which is what pairing MEANS");
        }
    }

    // The census itself, asserted: eleven implementors, three shapes, in the
    // proportions the grep answered. A row added without a shape, or a shape
    // silently re-attributed, fails here rather than quietly weakening the
    // table above.
    assert_eq!(
        NO_MUTED_LINKS.1 + NO_MUTED_LINKS.2,
        NO_MUTED_LINKS.0,
        "the sides must account for every declaration"
    );
    assert_eq!(BLENDER_HOOKS.len(), 11);
    let count =
        |want: fn(&BlenderHook) -> bool| BLENDER_HOOKS.iter().filter(|h| want(&h.1)).count();
    assert_eq!(count(|s| matches!(s, BlenderHook::FirstDataInput)), 3);
    assert_eq!(count(|s| matches!(s, BlenderHook::SameIndex)), 1);
    assert_eq!(count(|s| matches!(s, BlenderHook::CorrespondingSocket)), 7);
}

#[test]
fn a_control_input_sharing_the_data_type_is_the_first_shape_a_declaration_is_for() {
    // Blender's Switch supports every socket data type, BOOLEAN included, so
    // `Switch(Switch: Bool, False: Bool, True: Bool) -> Bool` is a real
    // configuration — and there the identity rule picks the SWITCH.
    let control = ("Switch", Ty::Number, true);
    let (document, node) = shaped(
        &[
            control,
            ("False", Ty::Number, true),
            ("True", Ty::Number, true),
        ],
        &[("Output", Ty::Number, true)],
    );
    assert_eq!(
        document.passthrough(ROOT, node).unwrap().source_of(0),
        Some(0),
        "the bare identity passes the CONTROL through, which is not what a \
         switch means — the case this declaration exists for"
    );

    // Declared off the path, the first data input is left, which is what
    // `node_geo_switch`'s callback returns.
    let (document, node) = shaped(
        &[
            ("Switch", Ty::Number, false),
            ("False", Ty::Number, true),
            ("True", Ty::Number, true),
        ],
        &[("Output", Ty::Number, true)],
    );
    let through = document.passthrough(ROOT, node).unwrap();
    assert_eq!(through.source_of(0), Some(1));
    assert_eq!(
        through.unreached_inputs(),
        &[0, 2],
        "and the control is named among the inputs nothing reaches"
    );
}

#[test]
fn an_output_can_be_declared_to_carry_nothing_while_bypassed() {
    // `node_geo_menu_switch` answers `nullptr` for every output after its
    // first: those values are only meaningful while the node computes. Our rule
    // would route them on type alone, so the declaration is the second — and
    // last — shape it is needed for.
    let (document, node) = shaped(
        &[("Menu", Ty::Text, false), ("Item", Ty::Number, true)],
        &[("Value", Ty::Number, true), ("Derived", Ty::Number, false)],
    );
    let through = document.passthrough(ROOT, node).unwrap();
    assert_eq!(
        through.routes(),
        &[Route {
            output: 0,
            input: 1,
            converts: false,
        }]
    );
    assert_eq!(
        through.dropped_outputs(),
        &[1],
        "an excluded output is DROPPED, which is the same report an output no \
         input could feed gets — one fact, one place"
    );
}

#[test]
fn an_excluded_port_is_excluded_in_the_structure_too() {
    // A declaration that changed the bypass but not the dissolve would be two
    // rules again, which is the whole thing R1586 exists to avoid.
    let mut document = Document::new("root");
    let source = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(4)), 0, 0)
        .unwrap();
    let gate = document
        .add_node(ROOT, NodeBody::Kind(Op::Gate), 100, 0)
        .unwrap();
    let sink = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 200, 0)
        .unwrap();
    // Gate(Limit: Number [off the path], Value: Number) -> Number
    document
        .connect(ROOT, Socket::new(source, 0), Socket::new(gate, 0))
        .unwrap();
    document
        .connect(ROOT, Socket::new(gate, 0), Socket::new(sink, 0))
        .unwrap();

    let through = document.passthrough(ROOT, gate).unwrap();
    assert_eq!(through.source_of(0), Some(1), "the Limit is off the path");

    let rewired = document.dissolve(ROOT, gate).unwrap();
    assert!(
        rewired.bridged.is_empty(),
        "input 1 is unwired, so there is nothing to bridge FROM — and the \
         structural form agrees with the bypass rather than reaching for the \
         Limit, which the bypass would not have used either"
    );
    assert_eq!(rewired.severed.len(), 1);
    assert!(document.validate().is_empty());
}

#[test]
fn a_port_declaration_survives_serialization() {
    // The declaration is part of the signature the taxonomy supplies, so it is
    // re-derived on load rather than stored — but a document written before
    // R1587 must still read, and the default is "takes part".
    let port: Port<Ty, Val> = Port::new("Value", Ty::Number);
    assert!(port.passthrough);
    assert!(
        !Port::<Ty, Val>::new("Limit", Ty::Number)
            .no_passthrough()
            .passthrough
    );
    let old = r#"{"name":"Value","ty":"Number","default":null}"#;
    let parsed: Port<Ty, Val> = serde_json::from_str(old).expect("a pre-R1587 port still reads");
    assert!(parsed.passthrough, "and takes part, as it did before");
}

// ------------------------------------------------------------------- frames
//
// R1589. Every claim about Blender below is stated as a HELPER that reproduces
// its rule over this crate's types, so the divergence is asserted rather than
// described — the discipline R1577 and R1584 set, because a test that checks
// only our own answer cannot tell a better rule from an equal one.

/// Blender's containment guard, `node_is_parent_and_child(parent, child)` at
/// `8cf50599`: walk the CHILD's parent chain and see whether the parent is on
/// it.
///
/// Present because Blender states it as `BLI_assert` inside `node_attach_node`,
/// which is compiled out of a release build — and because its own
/// `NODE_OT_parent_set` calls `node_detach_node` first, so by the time the
/// assert runs the chain it would have walked is already cleared.
fn blender_is_parent_and_child(
    document: &Document<Op>,
    tree: TreeId,
    parent: NodeId,
    child: NodeId,
) -> bool {
    let mut cursor = Some(child);
    let mut seen = std::collections::BTreeSet::new();
    while let Some(current) = cursor {
        if current == parent {
            return true;
        }
        if !seen.insert(current) {
            return false;
        }
        cursor = document
            .tree(tree)
            .and_then(|t| t.node(current))
            .and_then(|n| n.parent);
    }
    false
}

/// A frame with two numbers in it, one of them also inside an inner frame.
///
/// `outer` contains `inner` and `two`; `inner` contains `three`. So every
/// question about roots, ancestry and lowest common container has a non-trivial
/// answer here.
struct Framed {
    document: Document<Op>,
    two: NodeId,
    three: NodeId,
    add: NodeId,
    sink: NodeId,
    outer: NodeId,
    inner: NodeId,
}

fn framed() -> Framed {
    let mut f = fixture();
    let outer = f.document.add_node(ROOT, NodeBody::Frame, 0, 0).unwrap();
    let inner = f.document.add_node(ROOT, NodeBody::Frame, 0, 60).unwrap();
    f.document.set_parent(ROOT, inner, Some(outer)).unwrap();
    f.document.set_parent(ROOT, f.two, Some(outer)).unwrap();
    f.document.set_parent(ROOT, f.three, Some(inner)).unwrap();
    Framed {
        document: f.document,
        two: f.two,
        three: f.three,
        add: f.add,
        sink: f.sink,
        outer,
        inner,
    }
}

#[test]
fn a_frame_takes_part_in_the_canvas_and_never_in_the_graph() {
    let f = framed();
    // Containment changed nothing about what the graph computes.
    assert_eq!(number(&f.document.evaluate(ROOT, f.add)), Some(5));
    // A frame has no ports, so nothing can be wired to one — and the refusal is
    // the ordinary arity refusal rather than an arm someone had to remember.
    let mut document = f.document;
    let refused = document
        .connect(ROOT, Socket::new(f.two, 0), Socket::new(f.outer, 0))
        .unwrap_err();
    assert!(matches!(
        refused,
        ConnectError::NoSuchPort { socket, arity: 0 } if socket.node == f.outer
    ));
    assert!(document.evaluate(ROOT, f.outer).is_empty());

    // Asserted inside a definition that HAS an interface, because the root's is
    // empty and a frame that wrongly presented its tree's own ports would give
    // the identical answer there — a counterfactual (CF-9) passed against the
    // first draft of this test for exactly that reason.
    let made = document.group(ROOT, &[f.add], "Sum").unwrap();
    let definition = made.definition;
    assert_eq!(
        document
            .tree(definition)
            .unwrap()
            .interface()
            .inputs()
            .len(),
        2,
        "the definition takes two values, so there is something to confuse it with"
    );
    let fenced = document
        .add_node(definition, NodeBody::Frame, 0, 0)
        .unwrap();
    let signature = document.signature(definition, fenced).unwrap();
    assert!(
        signature.inputs.is_empty() && signature.outputs.is_empty(),
        "a frame has no ports of its own, in a tree that has ports"
    );
    assert!(document.evaluate(definition, fenced).is_empty());
    assert!(
        document
            .connect(definition, Socket::new(fenced, 0), Socket::new(fenced, 0))
            .is_err()
    );
    assert!(document.validate().is_empty());
}

#[test]
fn the_forest_answers_ancestry_members_and_contents() {
    let f = framed();
    assert_eq!(f.document.ancestry(ROOT, f.three), vec![f.outer, f.inner]);
    assert_eq!(f.document.ancestry(ROOT, f.two), vec![f.outer]);
    assert!(f.document.ancestry(ROOT, f.add).is_empty());

    let mut direct = f.document.members(ROOT, f.outer);
    direct.sort_unstable();
    let mut expected = vec![f.inner, f.two];
    expected.sort_unstable();
    assert_eq!(direct, expected, "members is DIRECT containment");

    let mut all = f.document.contents(ROOT, f.outer);
    all.sort_unstable();
    let mut deep = vec![f.inner, f.two, f.three];
    deep.sort_unstable();
    assert_eq!(all, deep, "contents is transitive");
    assert_eq!(f.document.contents(ROOT, f.inner), vec![f.three]);
}

#[test]
fn the_common_frame_of_a_frame_and_its_content_is_the_frames_own_container() {
    let f = framed();
    // `inner` and what is inside it: the answer must be true of BOTH, and
    // `inner` is not inside itself.
    assert_eq!(
        f.document.common_frame(ROOT, &[f.inner, f.three]),
        Some(f.outer)
    );
    assert_eq!(
        f.document.common_frame(ROOT, &[f.two, f.three]),
        Some(f.outer)
    );
    assert_eq!(f.document.common_frame(ROOT, &[f.three]), Some(f.inner));
    assert_eq!(
        f.document.common_frame(ROOT, &[f.three, f.add]),
        None,
        "one of them is on the canvas, so the canvas is all they share"
    );
    assert_eq!(f.document.common_frame(ROOT, &[]), None);
}

#[test]
fn a_containment_cycle_is_refused_where_blenders_own_guard_passes_it() {
    let mut f = framed();
    // Blender's `NODE_OT_parent_set` with `outer` selected and `inner` active:
    // it calls `node_detach_node(outer)` and THEN asserts. Reproduce that exact
    // order and ask its guard the question it would ask.
    let mut mirror = f.document.clone();
    mirror
        .tree_mut(ROOT)
        .unwrap()
        .node_mut(f.outer)
        .unwrap()
        .parent = None;
    assert!(
        !blender_is_parent_and_child(&mirror, ROOT, f.inner, f.outer),
        "the detach cleared the chain the assert walks, so it fires on nothing"
    );

    // Here the same request is refused, and the refusal names the chain.
    let refused = f
        .document
        .set_parent(ROOT, f.outer, Some(f.inner))
        .unwrap_err();
    let ParentError::Cycle { chain } = &refused else {
        panic!("expected a cycle, got {refused:?}");
    };
    assert_eq!(chain, &vec![f.outer, f.inner]);
    assert!(refused.to_string().contains("inside itself"));
    // Refused means unchanged.
    assert_eq!(f.document.ancestry(ROOT, f.inner), vec![f.outer]);
    assert!(f.document.validate().is_empty());
}

#[test]
fn a_node_cannot_be_inside_itself_or_inside_something_that_is_not_a_frame() {
    let mut f = framed();
    assert_eq!(
        f.document.set_parent(ROOT, f.outer, Some(f.outer)),
        Err(ParentError::SelfParent(f.outer))
    );
    assert_eq!(
        f.document.set_parent(ROOT, f.two, Some(f.add)),
        Err(ParentError::NotAFrame { node: f.add }),
        "Blender states this one as an assertion too"
    );
    assert_eq!(
        f.document.set_parent(ROOT, f.two, Some(NodeId(99))),
        Err(ParentError::NoSuchNode {
            tree: ROOT,
            node: NodeId(99)
        })
    );
    assert_eq!(f.document.ancestry(ROOT, f.two), vec![f.outer]);
}

#[test]
fn framing_a_selection_attaches_only_its_outermost_members() {
    let mut f = framed();
    // `inner` and `three` are both selected, and `three` is inside `inner`.
    let made = f
        .document
        .enframe(ROOT, &[f.inner, f.three], Some("decode".to_owned()))
        .unwrap();
    assert_eq!(
        made.members,
        vec![f.inner],
        "the inner one keeps the container that is itself moving"
    );
    assert_eq!(
        f.document.ancestry(ROOT, f.three),
        vec![f.outer, made.frame, f.inner]
    );
    // The new frame landed INSIDE what already contained all of the selection,
    // so framing part of a pipeline does not lift it out of the pipeline.
    assert_eq!(f.document.ancestry(ROOT, made.frame), vec![f.outer]);
    assert_eq!(
        f.document
            .tree(ROOT)
            .unwrap()
            .node(made.frame)
            .unwrap()
            .label
            .as_deref(),
        Some("decode")
    );
    assert!(f.document.validate().is_empty());
}

#[test]
fn the_outermost_derivation_is_what_every_gesture_over_the_forest_uses() {
    let f = framed();
    // Blender computes this three times — `node_join_attach_recursive`,
    // `node_detach_recursive` (two recursive functions over two structs with
    // identical fields) and again in the transform code.
    assert_eq!(
        f.document.outermost(ROOT, &[f.inner, f.three]),
        vec![f.inner]
    );
    assert_eq!(
        f.document.outermost(ROOT, &[f.two, f.three]),
        {
            let mut both = vec![f.two, f.three];
            both.sort_unstable();
            both
        },
        "neither contains the other, so both are roots"
    );
    assert_eq!(
        f.document
            .outermost(ROOT, &[f.outer, f.inner, f.three, f.two]),
        vec![f.outer]
    );
    assert!(f.document.outermost(ROOT, &[]).is_empty());
}

#[test]
fn unframing_leaves_one_level_where_blender_leaves_all_of_them() {
    let mut f = framed();
    assert_eq!(f.document.ancestry(ROOT, f.three), vec![f.outer, f.inner]);

    let moved = f.document.unframe(ROOT, &[f.three]).unwrap();
    assert_eq!(moved, vec![f.three]);
    assert_eq!(
        f.document.ancestry(ROOT, f.three),
        vec![f.outer],
        "out of `inner`, still in `outer` — the containment the gesture did not touch"
    );
    // Blender's `NODE_OT_detach` sets parent to nullptr, which is reachable here
    // too and is a DIFFERENT request.
    f.document.set_parent(ROOT, f.three, None).unwrap();
    assert!(f.document.ancestry(ROOT, f.three).is_empty());
    // Nothing to leave.
    assert!(f.document.unframe(ROOT, &[f.three]).unwrap().is_empty());
    assert!(f.document.validate().is_empty());
}

#[test]
fn moving_a_frame_moves_everything_it_contains() {
    let mut f = framed();
    let before: Vec<(i32, i32)> = [f.two, f.three, f.add]
        .iter()
        .map(|&id| {
            let n = f.document.tree(ROOT).unwrap().node(id).unwrap();
            (n.x, n.y)
        })
        .collect();

    let moved = f.document.translate(ROOT, f.outer, 40, -10).unwrap();
    assert_eq!(
        moved.first(),
        Some(&f.outer),
        "the frame itself comes first"
    );
    let mut carried = moved[1..].to_vec();
    carried.sort_unstable();
    let mut deep = vec![f.inner, f.two, f.three];
    deep.sort_unstable();
    assert_eq!(carried, deep, "transitively, not just direct members");

    for (id, (x, y)) in [f.two, f.three].iter().zip(&before) {
        let now = f.document.tree(ROOT).unwrap().node(*id).unwrap();
        assert_eq!((now.x, now.y), (x + 40, y - 10));
    }
    let untouched = f.document.tree(ROOT).unwrap().node(f.add).unwrap();
    assert_eq!((untouched.x, untouched.y), before[2]);
    assert!(f.document.validate().is_empty());
}

#[test]
fn deleting_a_frame_hands_its_members_to_the_frame_above_it() {
    let mut f = framed();
    let removed = f.document.remove_node(ROOT, f.inner).unwrap();
    assert_eq!(removed.adopted, vec![f.three]);
    assert_eq!(
        f.document.ancestry(ROOT, f.three),
        vec![f.outer],
        "only the containment the deletion destroyed is destroyed"
    );
    // Blender's `node_unlink_attached` clears the child's parent outright, so
    // the same delete would put `three` on the canvas even though `outer` is
    // still there and still contains where it was.
    let mut blender = framed();
    for member in blender.document.members(ROOT, blender.inner) {
        blender
            .document
            .tree_mut(ROOT)
            .unwrap()
            .node_mut(member)
            .unwrap()
            .parent = None;
    }
    blender.document.remove_node(ROOT, blender.inner).unwrap();
    assert!(blender.document.ancestry(ROOT, blender.three).is_empty());
    assert!(f.document.validate().is_empty());
}

#[test]
fn a_dissolved_frame_hands_its_members_up_by_the_same_derivation() {
    let mut f = framed();
    let out = f.document.dissolve(ROOT, f.inner).unwrap();
    assert_eq!(out.adopted, vec![f.three]);
    assert_eq!(f.document.ancestry(ROOT, f.three), vec![f.outer]);
    assert!(
        f.document.validate().is_empty(),
        "without this a member would name a node that is gone"
    );
    // Detach removes no node, so it adopts nobody.
    let mut g = framed();
    assert!(g.document.detach(ROOT, g.inner).unwrap().adopted.is_empty());
}

#[test]
fn a_boundary_move_carries_the_whole_node() {
    // R1589 found this broken: `partition::move_nodes` copied the LABEL alone,
    // three rounds after `Node::adopt_from` was introduced so that a field added
    // to a node could not be dropped by a hand-rolled copy.
    let mut b = boundaried();
    b.document.set_bypassed(ROOT, b.two, true).unwrap();
    b.document
        .tree_mut(ROOT)
        .unwrap()
        .node_mut(b.two)
        .unwrap()
        .appearance
        .collapsed = true;

    let out = b
        .document
        .group_insert(ROOT, b.instance, &[b.two], Sharing::Shared)
        .unwrap();
    let moved = b
        .document
        .tree(b.definition)
        .unwrap()
        .node(out.moved[0])
        .unwrap();
    assert!(moved.bypassed, "a bypassed node stays bypassed");
    assert!(moved.appearance.collapsed, "and keeps its looks");
}

#[test]
fn a_node_moved_into_a_group_leaves_its_frame_behind_and_the_frame_is_named() {
    let mut b = boundaried();
    let frame = b.document.add_node(ROOT, NodeBody::Frame, 0, 0).unwrap();
    b.document.set_parent(ROOT, b.two, Some(frame)).unwrap();

    let out = b
        .document
        .group_insert(ROOT, b.instance, &[b.two], Sharing::Shared)
        .unwrap();
    assert_eq!(
        out.orphaned,
        vec![Orphaned {
            node: out.moved[0],
            frame
        }],
        "a host-tree frame id means nothing inside the definition"
    );
    assert!(
        b.document
            .tree(b.definition)
            .unwrap()
            .node(out.moved[0])
            .unwrap()
            .parent
            .is_none()
    );
    assert!(b.document.validate().is_empty());
}

#[test]
fn a_node_moved_out_of_a_group_lands_in_the_frame_the_instance_is_in() {
    let mut b = boundaried();
    let frame = b.document.add_node(ROOT, NodeBody::Frame, 0, 0).unwrap();
    b.document
        .set_parent(ROOT, b.instance, Some(frame))
        .unwrap();

    let out = b
        .document
        .group_separate(ROOT, b.instance, &[b.add], Sharing::Shared)
        .unwrap();
    assert_eq!(
        b.document.ancestry(ROOT, out.moved[0]),
        vec![frame],
        "it lands where the instance is, which is inside the instance's frame"
    );
    assert!(out.orphaned.is_empty());
    assert!(b.document.validate().is_empty());
}

#[test]
fn a_collapse_leaves_the_instance_where_the_selection_was() {
    let mut f = framed();
    let made = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    assert!(
        f.document.ancestry(ROOT, made.node).is_empty(),
        "the selection was on the canvas"
    );

    let mut g = framed();
    let frame = g.document.add_node(ROOT, NodeBody::Frame, 0, 0).unwrap();
    g.document.set_parent(ROOT, g.add, Some(frame)).unwrap();
    g.document.set_parent(ROOT, g.sink, Some(frame)).unwrap();
    let made = g.document.group(ROOT, &[g.add, g.sink], "Tail").unwrap();
    assert_eq!(
        g.document.ancestry(ROOT, made.node),
        vec![frame],
        "a pipeline stage collapsed into a group stays in its pipeline"
    );
    // The nodes themselves are in another tree now, where a host frame id means
    // nothing — so the containment survives at the level that can hold it, the
    // instance, and its loss at the node level is reported rather than assumed.
    assert_eq!(
        made.orphaned,
        vec![
            Orphaned { node: g.add, frame },
            Orphaned {
                node: g.sink,
                frame
            }
        ]
    );
    assert!(g.document.validate().is_empty());
}

#[test]
fn a_collapse_carries_a_selected_frame_and_names_one_that_stayed() {
    let mut f = fixture();
    let frame = f.document.add_node(ROOT, NodeBody::Frame, 100, 0).unwrap();
    f.document.set_parent(ROOT, f.add, Some(frame)).unwrap();

    // The frame is selected too, so it travels with what it contains.
    let carried = f.document.group(ROOT, &[frame, f.add], "Stage").unwrap();
    assert!(carried.orphaned.is_empty());
    let inside = f.document.tree(carried.definition).unwrap();
    let moved_frame = inside
        .nodes()
        .find(|n| n.is_frame())
        .expect("the frame came along");
    assert_eq!(
        f.document
            .tree(carried.definition)
            .unwrap()
            .nodes()
            .find(|n| matches!(n.body, NodeBody::Kind(Op::Add)))
            .unwrap()
            .parent,
        Some(moved_frame.id),
        "ids are preserved by a collapse, so the containment needs no remapping"
    );
    assert!(f.document.validate().is_empty());

    // The other way: the frame is NOT selected, so it stays and is reported.
    let mut g = fixture();
    let frame = g.document.add_node(ROOT, NodeBody::Frame, 100, 0).unwrap();
    g.document.set_parent(ROOT, g.add, Some(frame)).unwrap();
    let cut = g.document.group(ROOT, &[g.add], "Stage").unwrap();
    assert_eq!(cut.orphaned, vec![Orphaned { node: g.add, frame }]);
    assert!(g.document.validate().is_empty());
}

#[test]
fn an_inline_keeps_the_definitions_own_frames_where_blender_flattens_them() {
    let mut f = fixture();
    let inner_frame = f.document.add_node(ROOT, NodeBody::Frame, 100, 0).unwrap();
    f.document
        .set_parent(ROOT, f.add, Some(inner_frame))
        .unwrap();
    let made = f
        .document
        .group(ROOT, &[inner_frame, f.add], "Stage")
        .unwrap();
    // The instance itself sits in a frame, which is the case that triggers
    // Blender's flattening loop at all.
    let host_frame = f.document.add_node(ROOT, NodeBody::Frame, 0, 0).unwrap();
    f.document
        .set_parent(ROOT, made.node, Some(host_frame))
        .unwrap();

    let out = f.document.ungroup(ROOT, made.node).unwrap();
    let frames: Vec<NodeId> = out
        .nodes
        .iter()
        .copied()
        .filter(|&id| f.document.tree(ROOT).unwrap().node(id).unwrap().is_frame())
        .collect();
    assert_eq!(frames.len(), 1, "the definition's own frame came back");
    let adder = out
        .nodes
        .iter()
        .copied()
        .find(|&id| {
            matches!(
                f.document.tree(ROOT).unwrap().node(id).unwrap().body,
                NodeBody::Kind(Op::Add)
            )
        })
        .unwrap();
    assert_eq!(
        f.document.ancestry(ROOT, adder),
        vec![host_frame, frames[0]],
        "the definition's forest survived, grafted onto the instance's container"
    );
    // Blender assigns the group node's parent to EVERY copied node
    // (`node_group.cc`, the `if (group_node.parent)` loop), overwriting the
    // parent/child relationships its own copy step had just recreated.
    let mut blender = f.document.clone();
    for &id in &out.nodes {
        blender.tree_mut(ROOT).unwrap().node_mut(id).unwrap().parent = Some(host_frame);
    }
    assert_eq!(
        blender.ancestry(ROOT, adder),
        vec![host_frame],
        "one level, because the internal frame is no longer anyone's container"
    );
    assert!(f.document.validate().is_empty());
}

#[test]
fn a_fragment_names_the_frame_it_was_cut_out_of() {
    let f = framed();
    let cut = f.document.extract(ROOT, &[f.three]).unwrap();
    assert_eq!(
        cut.orphaned(),
        &[Orphaned {
            node: f.three,
            frame: f.inner
        }]
    );
    assert!(
        cut.nodes().all(|n| n.parent.is_none()),
        "the fragment does not hold the frame, so it does not claim to"
    );

    // A selection that takes its frame with it keeps the containment, because a
    // fragment preserves node ids. `inner`'s OWN container stayed behind, so
    // that one crossing of the boundary is the only thing reported.
    let whole = f.document.extract(ROOT, &[f.inner, f.three]).unwrap();
    assert_eq!(
        whole.orphaned(),
        &[Orphaned {
            node: f.inner,
            frame: f.outer
        }]
    );
    assert_eq!(
        whole
            .document()
            .tree(ROOT)
            .unwrap()
            .node(f.three)
            .unwrap()
            .parent,
        Some(f.inner)
    );
    assert!(whole.validate().is_empty());
}

#[test]
fn a_duplicate_lands_back_in_its_frame_where_blender_leaves_it_outside() {
    let mut f = framed();
    let out = f
        .document
        .duplicate(
            ROOT,
            &[f.three],
            (40, 40),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    let copy = out.nodes[0];
    assert_eq!(out.reframed, vec![copy]);
    assert!(out.unframed.is_empty());
    assert_eq!(
        f.document.ancestry(ROOT, copy),
        vec![f.outer, f.inner],
        "a duplicate of something in a frame is in that frame"
    );
    // Blender's copy path looks the parent up in the copy map, does not find it
    // because the frame was not selected, and calls `node_detach_node` — with no
    // record anywhere that it happened.
    let mut blender = f.document.clone();
    blender
        .tree_mut(ROOT)
        .unwrap()
        .node_mut(copy)
        .unwrap()
        .parent = None;
    assert!(blender.ancestry(ROOT, copy).is_empty());
    assert!(f.document.validate().is_empty());
}

#[test]
fn a_fragment_pasted_into_another_tree_never_joins_a_frame_by_number() {
    let mut f = framed();
    let cut = f.document.extract(ROOT, &[f.three]).unwrap();
    // A definition whose node ids collide with the root's by construction.
    let elsewhere = f.document.add_definition("elsewhere");
    let decoy = f
        .document
        .add_node(elsewhere, NodeBody::Frame, 0, 0)
        .unwrap();
    while f.document.tree(elsewhere).unwrap().node_count() <= f.inner.0 as usize {
        f.document
            .add_node(elsewhere, NodeBody::Frame, 0, 0)
            .unwrap();
    }
    assert!(
        f.document
            .tree(elsewhere)
            .unwrap()
            .node(f.inner)
            .is_some_and(Node::is_frame),
        "the same NUMBER names a frame there, which is the trap"
    );

    let out = f
        .document
        .insert(elsewhere, &cut, (0, 0), Crossings::Drop, Definitions::Share)
        .unwrap();
    assert!(out.reframed.is_empty());
    assert_eq!(
        out.unframed,
        vec![Orphaned {
            node: out.nodes[0],
            frame: f.inner
        }]
    );
    assert!(
        f.document
            .tree(elsewhere)
            .unwrap()
            .node(out.nodes[0])
            .unwrap()
            .parent
            .is_none()
    );
    assert_ne!(decoy, out.nodes[0]);
    assert!(f.document.validate().is_empty());
}

#[test]
fn a_broken_forest_is_reported_three_ways() {
    let f = framed();
    let mut json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&f.document).unwrap()).unwrap();
    let nodes = json["trees"][0]["nodes"].as_array_mut().unwrap();
    // `two` (index 0) points at a node that is not there; `add` (index 2) points
    // at something that is not a frame; and the two frames contain each other.
    nodes[0]["parent"] = serde_json::json!(99);
    nodes[2]["parent"] = serde_json::json!(f.sink.0);
    nodes[4]["parent"] = serde_json::json!(f.inner.0);
    let broken: Document<Op> = serde_json::from_value(json).unwrap();

    let found = broken.validate();
    assert!(found.contains(&Violation::DanglingParent {
        tree: ROOT,
        node: f.two,
        parent: NodeId(99)
    }));
    assert!(found.contains(&Violation::ParentNotAFrame {
        tree: ROOT,
        node: f.add,
        parent: f.sink
    }));
    assert!(found.contains(&Violation::ContainmentCycle {
        tree: ROOT,
        node: f.outer
    }));
    assert!(found.contains(&Violation::ContainmentCycle {
        tree: ROOT,
        node: f.inner
    }));
    // And every derivation over that document still terminates.
    assert_eq!(broken.ancestry(ROOT, f.outer).len(), 1);
    assert_eq!(broken.contents(ROOT, f.outer).len(), 2);
    assert!(broken.outermost(ROOT, &[f.outer, f.inner]).len() <= 2);
}

#[test]
fn the_forest_survives_a_round_trip() {
    let f = framed();
    let text = serde_json::to_string(&f.document).unwrap();
    let back: Document<Op> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, f.document);
    assert_eq!(back.ancestry(ROOT, f.three), vec![f.outer, f.inner]);
    assert!(back.validate().is_empty());

    // A document written before frames existed has no `parent` key at all.
    let older = serde_json::json!({
        "trees": [{
            "id": 0,
            "name": "root",
            "nodes": [{"id": 0, "body": {"Kind": {"Num": 1}}, "x": 0, "y": 0, "label": null}],
            "links": [],
            "interface": {"inputs": [], "outputs": []},
            "next_node": 1,
            "next_link": 0
        }]
    });
    let parsed: Document<Op> = serde_json::from_value(older).unwrap();
    assert!(
        parsed
            .tree(ROOT)
            .unwrap()
            .node(NodeId(0))
            .unwrap()
            .parent
            .is_none()
    );
}

#[test]
fn framing_nothing_is_refused_and_an_empty_frame_needs_no_derivation() {
    let mut f = fixture();
    assert_eq!(f.document.enframe(ROOT, &[], None), Err(ParentError::Empty));
    assert_eq!(
        f.document.enframe(ROOT, &[NodeId(42)], None),
        Err(ParentError::NoSuchNode {
            tree: ROOT,
            node: NodeId(42)
        })
    );
    // The empty frame is the plain constructor.
    let empty = f.document.add_node(ROOT, NodeBody::Frame, 10, 20).unwrap();
    assert!(f.document.members(ROOT, empty).is_empty());
    assert_eq!(
        f.document
            .tree(ROOT)
            .unwrap()
            .node(empty)
            .unwrap()
            .display_name(),
        "Frame"
    );
    assert!(f.document.validate().is_empty());
}

// ---------------------------------------------------------------- selecting
//
// R1590. As with the frames, every claim about Blender is a HELPER reproducing
// its rule over these types, so a divergence is asserted rather than described.

/// Blender's `node_select_linked_to_exec` / `..._from_exec` at `8cf50599`: for
/// each selected node, walk its sockets' **directly linked** sockets and select
/// their owners. One hop, every time — the reach is the number of keypresses.
fn blender_linked_one_hop(
    document: &Document<Op>,
    tree: TreeId,
    selection: &[NodeId],
    downstream: bool,
) -> std::collections::BTreeSet<NodeId> {
    let mut out: std::collections::BTreeSet<NodeId> = selection.iter().copied().collect();
    let Some(host) = document.tree(tree) else {
        return out;
    };
    let held: Vec<NodeId> = selection.to_vec();
    for link in host.links() {
        let (near, far) = if downstream {
            (link.from.node, link.to.node)
        } else {
            (link.to.node, link.from.node)
        };
        if held.contains(&near) {
            out.insert(far);
        }
    }
    out
}

/// Compile-time witness that growing a selection is a **query**.
///
/// This function body only compiles because `grow` takes `&self`, so the
/// guarantee is the signature rather than an assertion — an edit that happened
/// to change nothing would satisfy any runtime comparison, which is why
/// `growing_a_selection_changes_nothing_in_the_document` is a consistency check
/// and this is the proof. Blender's equivalents take the tree by mutable
/// reference and carry `OPTYPE_UNDO`.
fn growing_needs_no_mutable_document(document: &Document<Op>) -> Vec<NodeId> {
    document
        .grow(ROOT, &[], Grow::SameKind)
        .map(|grown| grown.selection)
        .unwrap_or_default()
}

/// Blender's `node_select_grouped_name` for a suffix: the run after the last
/// delimiter, or — its own special case — the WHOLE NAME when there is none.
fn blender_suffix(name: &str) -> &str {
    name.rsplit_once(['.', '-', '_'])
        .map_or(name, |(_, tail)| tail)
}

/// A chain long enough that one hop and the closure differ, with a branch so
/// "everything downstream" is not a straight line: `head -> mid -> tail`, and
/// `mid -> aside` as well.
struct Chained {
    document: Document<Op>,
    head: NodeId,
    mid: NodeId,
    tail: NodeId,
    aside: NodeId,
}

fn chained() -> Chained {
    let mut document = Document::new("root");
    let head = document
        .add_node(ROOT, NodeBody::Kind(Op::Num(2)), 0, 0)
        .unwrap();
    let mid = document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 200, 0)
        .unwrap();
    let tail = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 400, 0)
        .unwrap();
    let aside = document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 400, 100)
        .unwrap();
    document
        .connect(ROOT, Socket::new(head, 0), Socket::new(mid, 0))
        .unwrap();
    document
        .connect(ROOT, Socket::new(mid, 0), Socket::new(tail, 0))
        .unwrap();
    document
        .connect(ROOT, Socket::new(mid, 0), Socket::new(aside, 0))
        .unwrap();
    Chained {
        document,
        head,
        mid,
        tail,
        aside,
    }
}

#[test]
fn one_hop_is_blenders_answer_and_the_closure_is_the_question() {
    let c = chained();
    let direct = c
        .document
        .grow(ROOT, &[c.head], Grow::Downstream(Reach::Direct))
        .unwrap();
    assert_eq!(direct.added, vec![c.mid], "one hop reaches the adder only");
    assert_eq!(
        direct
            .selection
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        blender_linked_one_hop(&c.document, ROOT, &[c.head], true),
        "and it is exactly what Blender's rule answers"
    );

    let closure = c
        .document
        .grow(ROOT, &[c.head], Grow::Downstream(Reach::Transitive))
        .unwrap();
    assert_eq!(
        closure.added,
        vec![c.mid, c.tail, c.aside],
        "PAST BLENDER — the branch and the far end in ONE call, where the reach \
         is a keypress count"
    );
    // The property that makes the answer knowable: asking again adds nothing.
    let again = c
        .document
        .grow(
            ROOT,
            &closure.selection,
            Grow::Downstream(Reach::Transitive),
        )
        .unwrap();
    assert!(!again.changed(), "a transitive walk is idempotent");
    assert_eq!(again.selection, closure.selection);
}

#[test]
fn growing_a_selection_changes_nothing_in_the_document() {
    let c = chained();
    let before = c.document.clone();
    for by in [
        Grow::Downstream(Reach::Transitive),
        Grow::Upstream(Reach::Transitive),
        Grow::SameKind,
        Grow::NamePrefix,
        Grow::NameSuffix,
        Grow::Contents(Reach::Direct),
        Grow::Containers(Reach::Direct),
    ] {
        c.document.grow(ROOT, &[c.mid], by).unwrap();
    }
    assert_eq!(
        c.document, before,
        "the document is the same value afterwards — a consistency check. The \
         GUARANTEE is the signature: see `growing_needs_no_mutable_document`, \
         which compiles only because `grow` takes `&self`, where Blender's \
         equivalents take the tree mutably and carry OPTYPE_UNDO"
    );
    assert!(growing_needs_no_mutable_document(&c.document).is_empty());
}

#[test]
fn upstream_is_the_other_direction_of_the_same_relation() {
    let c = chained();
    let up = c
        .document
        .grow(ROOT, &[c.tail], Grow::Upstream(Reach::Transitive))
        .unwrap();
    assert_eq!(up.added, vec![c.head, c.mid]);
    assert!(
        !up.selection.contains(&c.aside),
        "the sibling branch is downstream of `mid`, not upstream of `tail`"
    );
    assert_eq!(
        c.document
            .grow(ROOT, &[c.tail], Grow::Upstream(Reach::Direct))
            .unwrap()
            .selection
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        blender_linked_one_hop(&c.document, ROOT, &[c.tail], false),
    );
}

#[test]
fn a_muted_link_is_still_a_wire_when_a_selection_grows() {
    let mut c = chained();
    let link = c
        .document
        .tree(ROOT)
        .unwrap()
        .link_into(Socket::new(c.tail, 0))
        .unwrap()
        .id;
    c.document.set_link_muted(ROOT, link, true).unwrap();
    // The value has stopped flowing...
    assert_eq!(number(&c.document.evaluate(ROOT, c.tail)), None);
    // ...and the wire is still on screen, so it is still a way to the node.
    let grown = c
        .document
        .grow(ROOT, &[c.mid], Grow::Downstream(Reach::Direct))
        .unwrap();
    assert!(
        grown.selection.contains(&c.tail),
        "R1586 — every STRUCTURAL derivation goes on seeing a muted link"
    );
}

#[test]
fn a_selection_does_not_grow_into_a_group_it_stops_at_the_instance() {
    let mut c = chained();
    let made = c.document.group(ROOT, &[c.mid], "Stage").unwrap();
    let inside: Vec<NodeId> = c
        .document
        .tree(made.definition)
        .unwrap()
        .nodes()
        .map(|n| n.id)
        .collect();
    let grown = c
        .document
        .grow(ROOT, &[c.head], Grow::Downstream(Reach::Transitive))
        .unwrap();
    assert!(
        grown.selection.contains(&made.node),
        "it reaches the instance"
    );
    for id in inside {
        assert!(
            !grown.added.contains(&id) || grown.selection.contains(&made.node),
            "a selection is within ONE tree; the definition's nodes are in another"
        );
    }
    // Read the other way: the growth's answer is entirely in the host tree.
    for &id in &grown.selection {
        assert!(c.document.tree(ROOT).unwrap().node(id).is_some());
    }
}

#[test]
fn the_two_relations_cannot_collide() {
    let f = framed();
    // A frame has no ports, so no link reaches one...
    let by_link = f
        .document
        .grow(ROOT, &[f.two], Grow::Downstream(Reach::Transitive))
        .unwrap();
    assert!(!by_link.selection.contains(&f.outer));
    assert!(!by_link.selection.contains(&f.inner));
    // ...and containment only ever relates a frame to its members.
    let by_frame = f
        .document
        .grow(ROOT, &[f.outer], Grow::Contents(Reach::Transitive))
        .unwrap();
    assert_eq!(by_frame.added, vec![f.two, f.three, f.inner]);
    assert!(!by_frame.selection.contains(&f.add), "which is outside it");
}

#[test]
fn contents_and_containers_read_the_reach_the_same_way_as_the_links_do() {
    let f = framed();
    assert_eq!(
        f.document
            .grow(ROOT, &[f.outer], Grow::Contents(Reach::Direct))
            .unwrap()
            .added,
        vec![f.two, f.inner],
        "direct members"
    );
    assert_eq!(
        f.document
            .grow(ROOT, &[f.three], Grow::Containers(Reach::Direct))
            .unwrap()
            .added,
        vec![f.inner],
        "the fence it is immediately in"
    );
    assert_eq!(
        f.document
            .grow(ROOT, &[f.three], Grow::Containers(Reach::Transitive))
            .unwrap()
            .added,
        vec![f.outer, f.inner],
        "and every fence above it — R1589's ancestry, asked as a selection"
    );
    assert!(
        !f.document
            .grow(ROOT, &[f.add], Grow::Containers(Reach::Transitive))
            .unwrap()
            .changed(),
        "a node on the canvas is in nothing"
    );
}

#[test]
fn same_kind_is_what_a_node_does_and_never_what_it_is_set_to() {
    let mut f = fixture();
    // `Num(2)` and `Num(3)` are two settings of ONE kind.
    let grown = f.document.grow(ROOT, &[f.two], Grow::SameKind).unwrap();
    assert_eq!(grown.added, vec![f.three]);
    assert!(
        !grown.selection.contains(&f.add),
        "an adder is not a number"
    );

    // A label does not change what a node is. `NodeKind::name` is a stable
    // identity token, which is Blender's `type_legacy` too.
    f.document
        .tree_mut(ROOT)
        .unwrap()
        .node_mut(f.three)
        .unwrap()
        .label = Some("Renamed".to_owned());
    assert_eq!(
        f.document
            .grow(ROOT, &[f.two], Grow::SameKind)
            .unwrap()
            .added,
        vec![f.three]
    );
}

#[test]
fn two_instances_of_different_definitions_are_not_one_kind() {
    let mut f = fixture();
    let one = f.document.group(ROOT, &[f.add], "Sum").unwrap();
    let other = f.document.add_definition("Other");
    let twin = f.document.instantiate(ROOT, other, 600, 0).unwrap();

    let grown = f.document.grow(ROOT, &[one.node], Grow::SameKind).unwrap();
    assert!(
        !grown.selection.contains(&twin),
        "PAST BLENDER — every group node there is `type_legacy == NODE_GROUP`, \
         so grouping by type sweeps in instances of unrelated definitions. An \
         instance's signature IS its definition's interface, so two instances \
         of different definitions are alike in nothing this model can see"
    );
    // Same definition, though, is the same kind.
    let again = f
        .document
        .instantiate(ROOT, one.definition, 600, 200)
        .unwrap();
    assert_eq!(
        f.document
            .grow(ROOT, &[one.node], Grow::SameKind)
            .unwrap()
            .added,
        vec![again]
    );
}

#[test]
fn an_affix_that_is_not_there_offers_no_criterion() {
    let mut f = fixture();
    let named = |document: &mut Document<Op>, id: NodeId, name: &str| {
        document.tree_mut(ROOT).unwrap().node_mut(id).unwrap().label = Some(name.to_owned());
    };
    named(&mut f.document, f.two, "decode.header");
    named(&mut f.document, f.three, "decode.body");
    named(&mut f.document, f.add, "verify.header");
    // TWO delimiter-free nodes with the SAME name: without a second one, "this
    // node has no suffix" and "its suffix is its whole name" give the identical
    // answer, and the counterfactual for Blender's substitution passes (CF-3).
    named(&mut f.document, f.sink, "plain");
    let twin = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Sink), 600, 0)
        .unwrap();
    named(&mut f.document, twin, "plain");

    assert_eq!(
        f.document
            .grow(ROOT, &[f.two], Grow::NamePrefix)
            .unwrap()
            .added,
        vec![f.three],
        "`decode` is the run up to the first delimiter"
    );
    assert_eq!(
        f.document
            .grow(ROOT, &[f.two], Grow::NameSuffix)
            .unwrap()
            .added,
        vec![f.add],
        "and `header` the run after the last"
    );

    // `plain` has no delimiter, so it has no affix and offers no criterion —
    // even though another node is called exactly that.
    let from_plain = f.document.grow(ROOT, &[f.sink], Grow::NameSuffix).unwrap();
    assert!(
        !from_plain.changed(),
        "PAST BLENDER — `node_select_grouped_name` substitutes the WHOLE NAME \
         for a missing suffix, which conflates 'this node has no suffix' with \
         'its suffix is its entire name'"
    );
    // Blender's rule held as a helper, and the divergence asserted: under it the
    // twin WOULD join, because both nodes' whole names stand in as suffixes.
    assert_eq!(blender_suffix("plain"), blender_suffix("plain"));
    assert!(
        !from_plain.selection.contains(&twin),
        "which is the node Blender's substitution would have swept in"
    );
    assert_ne!(blender_suffix("decode.header"), "decode.header");
    // And a node that is not selected is never a criterion.
    assert!(
        !f.document
            .grow(ROOT, &[f.sink], Grow::NamePrefix)
            .unwrap()
            .changed()
    );
}

#[test]
fn the_affix_is_read_off_the_name_that_is_painted() {
    let mut f = fixture();
    // No label, so the displayed name is the body's own — which is what a node
    // header shows. Blender groups on `bNode::name`, the datablock id
    // (`Mix.001`), which is not what its own header draws.
    assert_eq!(
        f.document
            .tree(ROOT)
            .unwrap()
            .node(f.two)
            .unwrap()
            .display_name(),
        "Num"
    );
    f.document
        .tree_mut(ROOT)
        .unwrap()
        .node_mut(f.two)
        .unwrap()
        .label = Some("stage.one".to_owned());
    f.document
        .tree_mut(ROOT)
        .unwrap()
        .node_mut(f.add)
        .unwrap()
        .label = Some("stage.two".to_owned());
    assert_eq!(
        f.document
            .grow(ROOT, &[f.two], Grow::NamePrefix)
            .unwrap()
            .added,
        vec![f.add],
        "the rename is what the user typed and what the card shows"
    );
}

#[test]
fn the_same_kind_run_is_in_evaluation_order_and_says_where_you_are() {
    let mut f = fixture();
    let far = f
        .document
        .add_node(ROOT, NodeBody::Kind(Op::Add), 600, 0)
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.add, 0), Socket::new(far, 0))
        .unwrap();

    let run = f.document.same_kind_run(ROOT, f.add).unwrap();
    assert_eq!(run, vec![f.add, far], "producers before consumers");
    assert_eq!(
        run.iter().position(|&id| id == far),
        Some(1),
        "PAST BLENDER — the RUN is published, so an editor can say `2 of 2`. \
         NODE_OT_select_same_type_step answers by moving the active node and \
         reports only whether it moved"
    );
    // Blender's operator is one line over this.
    let step = |from: NodeId, forward: bool| -> Option<NodeId> {
        let at = run.iter().position(|&id| id == from)?;
        let next = if forward {
            at.checked_add(1)?
        } else {
            at.checked_sub(1)?
        };
        run.get(next).copied()
    };
    assert_eq!(step(f.add, true), Some(far));
    assert_eq!(
        step(far, true),
        None,
        "it stops at the end, as Blender's does"
    );
    assert_eq!(step(far, false), Some(f.add));
    assert_eq!(f.document.same_kind_run(ROOT, NodeId(99)), None);
}

#[test]
fn the_evaluation_order_is_a_permutation_even_when_the_document_is_not_a_graph() {
    let f = fixture();
    let order = f.document.evaluation_order(ROOT);
    assert_eq!(order.len(), f.document.tree(ROOT).unwrap().node_count());
    assert!(
        order.iter().position(|&id| id == f.add) < order.iter().position(|&id| id == f.sink),
        "the adder is resolved before its sink"
    );

    // A document that arrived with a link cycle: every node still appears once.
    let mut json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&f.document).unwrap()).unwrap();
    let links = json["trees"][0]["links"].as_array_mut().unwrap();
    links.push(serde_json::json!({
        "id": 90, "from": {"node": f.sink.0, "port": 0},
        "to": {"node": f.add.0, "port": 0}, "muted": false
    }));
    let broken: Document<Op> = serde_json::from_value(json).unwrap();
    let order = broken.evaluation_order(ROOT);
    let unique: std::collections::BTreeSet<NodeId> = order.iter().copied().collect();
    assert_eq!(order.len(), unique.len(), "no repeats");
    assert_eq!(
        unique.len(),
        broken.tree(ROOT).unwrap().node_count(),
        "none lost"
    );
    // And a growth over it terminates.
    assert!(
        broken
            .grow(ROOT, &[f.add], Grow::Downstream(Reach::Transitive))
            .unwrap()
            .changed()
    );
}

#[test]
fn a_stale_selection_is_refused_rather_than_quietly_narrowed() {
    let f = fixture();
    assert_eq!(
        f.document.grow(ROOT, &[f.two, NodeId(99)], Grow::SameKind),
        Err(SelectError::NoSuchNode {
            tree: ROOT,
            node: NodeId(99)
        }),
        "Blender's operators skip such a node, so the question silently becomes \
         a different question"
    );
    assert_eq!(
        f.document.grow(TreeId(9), &[], Grow::SameKind),
        Err(SelectError::NoSuchTree(TreeId(9)))
    );
    // An empty selection is a legitimate question with an empty answer.
    let empty = f.document.grow(ROOT, &[], Grow::SameKind).unwrap();
    assert!(empty.selection.is_empty() && !empty.changed());
}

// ------------------------------------------------------------------- R1593
// A link may convert.

/// A second test taxonomy, whose type relation is **asymmetric**.
///
/// Kept apart from [`Op`] on purpose. `Op`'s relation is equality, and every
/// assertion above this line reads its meaning from that — `Measure`'s output is
/// the one "no input can feed" only because `Text` does not reach `Number`. Two
/// taxonomies in one crate is also what proves the default is a *default*: one
/// mechanism, two different relations, neither compiled in.
///
/// The lattice is the one the crate's flagship consumer has: a scalar
/// broadcasts into a vector, and a vector never narrows back into a scalar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum LTy {
    Scalar,
    Vector,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum LVal {
    Scalar(i64),
    Vector([i64; 3]),
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum LOp {
    /// A scalar source.
    Level(i64),
    /// A vector source.
    Swatch([i64; 3]),
    /// `Vector + Vector -> Vector`, so a scalar reaches it only by broadcasting.
    Sum,
    /// `(Scalar, Vector) -> (Vector, Scalar)`. The shape that makes the routing
    /// preference falsifiable: output 0 is a vector, its own index holds a
    /// scalar that would have to CONVERT, and index 1 holds a vector that would
    /// not.
    Cross,
    /// `Vector -> Scalar`. Its output is the one no input can feed, because the
    /// narrowing direction is refused — the analogue of `Op::Measure`, reached
    /// through the lattice rather than through disjointness.
    Meter,
    /// `Scalar -> Vector`. The mirror of `Meter`, and the shape whose only
    /// pass-through CONVERTS: bypassed, its scalar input can reach its vector
    /// output only by broadcasting. Under a relation that compared types with
    /// `==` this output would be dropped instead.
    Wash,
    Sink,
}

impl NodeKind for LOp {
    type Type = LTy;
    type Value = LVal;

    fn name(&self) -> String {
        match self {
            Self::Level(_) => "Level",
            Self::Swatch(_) => "Swatch",
            Self::Sum => "Sum",
            Self::Cross => "Cross",
            Self::Meter => "Meter",
            Self::Wash => "Wash",
            Self::Sink => "Sink",
        }
        .to_owned()
    }

    fn inputs(&self) -> Vec<Port<LTy, LVal>> {
        match self {
            Self::Level(_) | Self::Swatch(_) => Vec::new(),
            Self::Sum => vec![
                Port::new("A", LTy::Vector).with_default(LVal::Vector([0, 0, 0])),
                Port::new("B", LTy::Vector).with_default(LVal::Vector([0, 0, 0])),
            ],
            Self::Cross => vec![
                Port::new("Amount", LTy::Scalar).with_default(LVal::Scalar(1)),
                Port::new("Colour", LTy::Vector).with_default(LVal::Vector([9, 9, 9])),
            ],
            Self::Meter => vec![Port::new("Colour", LTy::Vector)],
            Self::Wash => vec![Port::new("Amount", LTy::Scalar).with_default(LVal::Scalar(3))],
            Self::Sink => vec![Port::new("Result", LTy::Vector)],
        }
    }

    fn outputs(&self) -> Vec<Port<LTy, LVal>> {
        match self {
            Self::Level(_) | Self::Meter => vec![Port::new("Out", LTy::Scalar)],
            Self::Swatch(_) | Self::Sum | Self::Wash => vec![Port::new("Out", LTy::Vector)],
            Self::Cross => vec![
                Port::new("Colour", LTy::Vector),
                Port::new("Amount", LTy::Scalar),
            ],
            Self::Sink => Vec::new(),
        }
    }

    fn evaluate(&self, inputs: &[Option<LVal>]) -> Vec<Option<LVal>> {
        let vector = |i: usize| match inputs.get(i).and_then(Option::as_ref) {
            Some(LVal::Vector(v)) => Some(*v),
            _ => None,
        };
        let scalar = |i: usize| match inputs.get(i).and_then(Option::as_ref) {
            Some(LVal::Scalar(s)) => Some(*s),
            _ => None,
        };
        match self {
            Self::Level(n) => vec![Some(LVal::Scalar(*n))],
            Self::Swatch(v) => vec![Some(LVal::Vector(*v))],
            Self::Sum => vec![
                vector(0)
                    .zip(vector(1))
                    .map(|(a, b)| LVal::Vector([a[0] + b[0], a[1] + b[1], a[2] + b[2]])),
            ],
            Self::Cross => vec![vector(1).map(LVal::Vector), scalar(0).map(LVal::Scalar)],
            Self::Meter => vec![vector(0).map(|v| LVal::Scalar(v[0] + v[1] + v[2]))],
            // Deliberately NOT the broadcast: a `Wash` that computed doubles
            // its input, so the value a bypassed one passes through is
            // distinguishable from the value a computing one produces.
            Self::Wash => vec![scalar(0).map(|s| LVal::Vector([s * 2, s * 2, s * 2]))],
            Self::Sink => Vec::new(),
        }
    }

    fn conversion(from: &LTy, to: &LTy) -> Conversion<LVal> {
        match (from, to) {
            (LTy::Scalar, LTy::Scalar) | (LTy::Vector, LTy::Vector) => Conversion::Direct,
            // The broadcast: a scalar becomes the grey of that magnitude.
            (LTy::Scalar, LTy::Vector) => Conversion::Converted(|value| match value {
                LVal::Scalar(s) => Some(LVal::Vector([s, s, s])),
                LVal::Vector(_) => None,
            }),
            // No narrowing.
            (LTy::Vector, LTy::Scalar) => Conversion::Refused,
        }
    }
}

/// Every ordered pair of the lattice's types, so a property holds over the whole
/// relation rather than over the one pair a fixture happens to use.
const LATTICE: [(LTy, LTy); 4] = [
    (LTy::Scalar, LTy::Scalar),
    (LTy::Scalar, LTy::Vector),
    (LTy::Vector, LTy::Scalar),
    (LTy::Vector, LTy::Vector),
];

/// A lattice document: `Level`(scalar) and `Swatch`(vector) feeding a `Sum`,
/// with a `Meter` and a `Sink` to hand.
struct Lattice {
    document: Document<LOp>,
    level: NodeId,
    swatch: NodeId,
    sum: NodeId,
    meter: NodeId,
    cross: NodeId,
}

fn lattice() -> Lattice {
    let mut document = Document::new("root");
    let level = document
        .add_node(ROOT, NodeBody::Kind(LOp::Level(4)), 0, 0)
        .unwrap();
    let swatch = document
        .add_node(ROOT, NodeBody::Kind(LOp::Swatch([1, 2, 3])), 0, 60)
        .unwrap();
    let sum = document
        .add_node(ROOT, NodeBody::Kind(LOp::Sum), 200, 0)
        .unwrap();
    let meter = document
        .add_node(ROOT, NodeBody::Kind(LOp::Meter), 200, 120)
        .unwrap();
    let cross = document
        .add_node(ROOT, NodeBody::Kind(LOp::Cross), 400, 0)
        .unwrap();
    Lattice {
        document,
        level,
        swatch,
        sum,
        meter,
        cross,
    }
}

#[test]
fn the_type_relation_is_directed_and_no_equality_could_be() {
    // The headline. The SAME document accepts this pair of types one way round
    // and refuses it the other. `connect`'s pre-R1593 gate was
    // `source.ty != sink.ty`, and `!=` is symmetric, so no implementation of
    // `PartialEq` — however clever — can produce both of these outcomes at once.
    // That is the proof that this crate's own doc was wrong to say a coercing
    // taxonomy "models that by making the coercion part of equality": making
    // them equal would have admitted the narrowing too.
    let mut f = lattice();

    // Scalar -> Vector: accepted.
    assert!(
        f.document
            .connect(ROOT, Socket::new(f.level, 0), Socket::new(f.sum, 0))
            .is_ok()
    );
    // Vector -> Scalar, the mirror image: refused, and the refusal names both
    // ends rather than reporting a bare failure.
    assert_eq!(
        f.document
            .connect(ROOT, Socket::new(f.swatch, 0), Socket::new(f.cross, 0)),
        Err(ConnectError::TypeMismatch {
            from: Socket::new(f.swatch, 0),
            from_type: LTy::Vector,
            to: Socket::new(f.cross, 0),
            to_type: LTy::Scalar,
        })
    );
}

#[test]
fn a_crossing_is_answerable_before_a_wire_exists() {
    // The question an editor asks while a wire is being dragged. Blender's
    // `validate_link` is a C function pointer on the tree type with no accessor
    // in front of it, and whether the value would be CHANGED on the way lives in
    // a different table again (`DataTypeConversions`).
    let f = lattice();
    let ask = |from: (NodeId, u32), to: (NodeId, u32)| {
        f.document
            .conversion(ROOT, Socket::new(from.0, from.1), Socket::new(to.0, to.1))
            .map(|c| c.name())
    };
    assert_eq!(ask((f.swatch, 0), (f.sum, 0)), Some("direct"));
    assert_eq!(ask((f.level, 0), (f.sum, 0)), Some("converted"));
    assert_eq!(ask((f.swatch, 0), (f.cross, 0)), Some("refused"));

    // "There is no such port" is a different answer from "no value may go
    // there", so it is a different value and not a third arm of `Conversion`.
    assert_eq!(ask((f.level, 7), (f.sum, 0)), None);
    assert_eq!(ask((f.level, 0), (NodeId(99), 0)), None);
}

#[test]
fn the_value_arrives_converted() {
    let mut f = lattice();
    f.document
        .connect(ROOT, Socket::new(f.level, 0), Socket::new(f.sum, 0))
        .unwrap();
    f.document
        .connect(ROOT, Socket::new(f.swatch, 0), Socket::new(f.sum, 1))
        .unwrap();
    // The scalar 4 broadcast to [4,4,4], plus the swatch.
    assert_eq!(
        f.document.evaluate(ROOT, f.sum),
        vec![Some(LVal::Vector([5, 6, 7]))],
        "the link carried the value THROUGH the declared conversion"
    );
    // And the conversion is readable off the link itself, after the fact.
    let link = f.document.tree(ROOT).unwrap().links()[0].id;
    assert_eq!(
        f.document.link_conversion(ROOT, link).map(|c| c.converts()),
        Some(true)
    );
}

#[test]
fn every_accepted_wire_can_carry_a_value() {
    // The property that comes of legality and the conversion being ONE
    // declaration: over the whole relation, `connect` accepts a pair exactly
    // when a value survives the trip. Blender cannot state this — a shader
    // tree's `validate_link` returns `true` for pairs `DataTypeConversions`
    // has no entry for at all, so acceptance there does not entail carriage.
    for (from, to) in LATTICE {
        let mut document = Document::new("root");
        // A source of `from` and a consumer of `to`, whichever they are.
        let (source, out_port) = match from {
            LTy::Scalar => (
                document
                    .add_node(ROOT, NodeBody::Kind(LOp::Level(7)), 0, 0)
                    .unwrap(),
                0,
            ),
            LTy::Vector => (
                document
                    .add_node(ROOT, NodeBody::Kind(LOp::Swatch([7, 7, 7])), 0, 0)
                    .unwrap(),
                0,
            ),
        };
        let (consumer, in_port) = match to {
            LTy::Scalar => (
                document
                    .add_node(ROOT, NodeBody::Kind(LOp::Cross), 200, 0)
                    .unwrap(),
                0,
            ),
            LTy::Vector => (
                document
                    .add_node(ROOT, NodeBody::Kind(LOp::Meter), 200, 0)
                    .unwrap(),
                0,
            ),
        };
        let accepted = document
            .connect(
                ROOT,
                Socket::new(source, out_port),
                Socket::new(consumer, in_port),
            )
            .is_ok();
        assert_eq!(
            accepted,
            LOp::conversion(&from, &to).is_allowed(),
            "{from:?} -> {to:?}: acceptance and the declared crossing are one answer"
        );
        if accepted {
            // A value arrives, AND it arrives as the port's own type. Merely
            // asserting `is_some()` would pass on a wire that carried the value
            // across unconverted, which is the very failure this property
            // exists to exclude — a counterfactual that stopped the evaluator
            // applying the conversion left this test green until it checked the
            // arriving type.
            let arrived = document
                .evaluator()
                .input(ROOT, Socket::new(consumer, in_port));
            let shape = match arrived {
                Some(LVal::Scalar(_)) => Some(LTy::Scalar),
                Some(LVal::Vector(_)) => Some(LTy::Vector),
                None => None,
            };
            assert_eq!(
                shape,
                Some(to),
                "{from:?} -> {to:?}: accepted, so a value of the DESTINATION's \
                 type must arrive"
            );
        }
    }
}

#[test]
fn a_bypassed_node_changes_a_value_only_when_it_has_to() {
    // `Cross` is `(Amount: Scalar, Colour: Vector) -> (Colour: Vector, Amount:
    // Scalar)`. Output 0 is a vector: its OWN index holds a scalar, which could
    // reach it by converting, and index 1 holds a vector, which reaches it
    // unchanged. The rule prefers the value that survives.
    let f = lattice();
    let through = f.document.passthrough(ROOT, f.cross).unwrap();
    assert_eq!(
        through.routes(),
        &[
            Route {
                output: 0,
                input: 1,
                converts: false,
            },
            Route {
                output: 1,
                input: 0,
                converts: false,
            },
        ],
        "a direct crossing beats a converting one at the output's own index"
    );
    assert!(
        !through.is_identity(),
        "neither value leaves by the port it arrived on"
    );

    // And when converting is the only way through, it is taken and SAID.
    let mut document = Document::new("root");
    let sum = document
        .add_node(ROOT, NodeBody::Kind(LOp::Sum), 0, 0)
        .unwrap();
    let level = document
        .add_node(ROOT, NodeBody::Kind(LOp::Level(5)), -200, 0)
        .unwrap();
    document
        .connect(ROOT, Socket::new(level, 0), Socket::new(sum, 0))
        .unwrap();
    document.set_bypassed(ROOT, sum, true).unwrap();
    let through = document.passthrough(ROOT, sum).unwrap();
    assert_eq!(
        through.routes(),
        &[Route {
            output: 0,
            input: 0,
            converts: false,
        }],
        "Sum is vector-in vector-out, so nothing converts here"
    );
    // A `Meter` bypassed: vector in, scalar out, and the narrowing is refused,
    // so the output is DROPPED rather than silently carrying the wrong thing.
    let meter = document
        .add_node(ROOT, NodeBody::Kind(LOp::Meter), 200, 0)
        .unwrap();
    let through = document.passthrough(ROOT, meter).unwrap();
    assert!(through.routes().is_empty());
    assert_eq!(through.dropped_outputs(), &[0]);
}

#[test]
fn a_converting_route_is_not_the_identity_and_the_evaluator_applies_it() {
    // A node whose only pass-through converts: `Cross`'s output 1 (Scalar) can
    // only come from input 0 (Scalar) — direct. So build the converting case
    // explicitly with a `Sum` whose input is fed a broadcast scalar.
    let mut document = Document::new("root");
    let swatch = document
        .add_node(ROOT, NodeBody::Kind(LOp::Swatch([2, 2, 2])), 0, 0)
        .unwrap();
    let cross = document
        .add_node(ROOT, NodeBody::Kind(LOp::Cross), 200, 0)
        .unwrap();
    let sink = document
        .add_node(ROOT, NodeBody::Kind(LOp::Sink), 400, 0)
        .unwrap();
    document
        .connect(ROOT, Socket::new(swatch, 0), Socket::new(cross, 1))
        .unwrap();
    document
        .connect(ROOT, Socket::new(cross, 0), Socket::new(sink, 0))
        .unwrap();
    assert_eq!(
        document.evaluator().input(ROOT, Socket::new(sink, 0)),
        Some(LVal::Vector([2, 2, 2]))
    );

    // Bypassed, the vector still comes out of output 0 — the direct route.
    document.set_bypassed(ROOT, cross, true).unwrap();
    assert_eq!(
        document.evaluator().input(ROOT, Socket::new(sink, 0)),
        Some(LVal::Vector([2, 2, 2]))
    );

    // A shape whose ONLY route converts, asserted end to end. `Wash` is
    // `Scalar -> Vector`: bypassed, its scalar input can reach its vector output
    // only by broadcasting, so under a relation that compared types with `==`
    // this output would be DROPPED and the sink would fall back to its default.
    let mut document = Document::new("root");
    let level = document
        .add_node(ROOT, NodeBody::Kind(LOp::Level(5)), 0, 0)
        .unwrap();
    let wash = document
        .add_node(ROOT, NodeBody::Kind(LOp::Wash), 200, 0)
        .unwrap();
    let sink = document
        .add_node(ROOT, NodeBody::Kind(LOp::Sink), 400, 0)
        .unwrap();
    document
        .connect(ROOT, Socket::new(level, 0), Socket::new(wash, 0))
        .unwrap();
    document
        .connect(ROOT, Socket::new(wash, 0), Socket::new(sink, 0))
        .unwrap();

    // Computing, `Wash` doubles: [10,10,10]. The two answers are distinguishable
    // on purpose, so "the bypass ran" is not confusable with "it computed".
    assert_eq!(
        document.evaluator().input(ROOT, Socket::new(sink, 0)),
        Some(LVal::Vector([10, 10, 10]))
    );

    let through = document.passthrough(ROOT, wash).unwrap();
    assert_eq!(
        through.routes(),
        &[Route {
            output: 0,
            input: 0,
            converts: true,
        }],
        "the only way through is the broadcast, and the routing SAYS it converts"
    );
    assert!(
        !through.is_identity(),
        "same index, but the value that leaves is not the value that arrived"
    );
    assert!(through.dropped_outputs().is_empty());

    document.set_bypassed(ROOT, wash, true).unwrap();
    assert_eq!(
        document.evaluator().input(ROOT, Socket::new(sink, 0)),
        Some(LVal::Vector([5, 5, 5])),
        "the scalar 5 crossed the bypassed node THROUGH the declared conversion"
    );
}

#[test]
fn a_document_from_a_file_is_checked_against_the_same_relation() {
    // `validate` is the standing check for a document that promised nothing.
    // Before R1593 it compared types with `!=`, which would have flagged every
    // legitimate broadcast in a saved lattice document as a violation.
    let mut f = lattice();
    f.document
        .connect(ROOT, Socket::new(f.level, 0), Socket::new(f.sum, 0))
        .unwrap();
    let text = serde_json::to_string(&f.document).unwrap();
    let reloaded: Document<LOp> = serde_json::from_str(&text).unwrap();
    assert!(
        reloaded.validate().is_empty(),
        "a broadcast survives the round trip without being called a mismatch"
    );

    // And a link the relation refuses IS flagged — the narrowing direction,
    // which no gesture in this crate can produce, so it can only arrive from
    // outside.
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let links = json["trees"][0]["links"].as_array_mut().unwrap();
    links.push(serde_json::json!({
        "id": 90, "from": {"node": f.swatch.0, "port": 0},
        "to": {"node": f.meter.0, "port": 0}, "muted": false
    }));
    // swatch(Vector) -> meter(Vector): legal, direct. And meter's OWN output is
    // a Scalar, so meter -> sum would BROADCAST and be legal too — which is the
    // trap this fixture walked into on the first draft. The refused direction is
    // a Vector reaching a Scalar port: swatch -> cross's `Amount`.
    links.push(serde_json::json!({
        "id": 91, "from": {"node": f.swatch.0, "port": 0},
        "to": {"node": f.cross.0, "port": 0}, "muted": false
    }));
    let broken: Document<LOp> = serde_json::from_value(json).unwrap();
    assert_eq!(
        broken.validate(),
        vec![Violation::TypeMismatch {
            tree: ROOT,
            link: crate::LinkId(91),
        }],
        "the narrowing is named, and the broadcast beside it is not"
    );
}

#[test]
fn the_default_relation_is_equality_and_nothing_had_to_say_so() {
    // `Op`, the taxonomy every assertion above this line uses, declares no
    // `crossing` at all — so this is the provided default, asserted directly
    // rather than inferred from those tests passing.
    assert!(Op::conversion(&Ty::Number, &Ty::Number).is_allowed());
    assert!(!Op::conversion(&Ty::Number, &Ty::Number).converts());
    assert!(Op::conversion(&Ty::Number, &Ty::Text).is_refused());
    assert!(Op::conversion(&Ty::Text, &Ty::Number).is_refused());
    assert_eq!(Op::conversion(&Ty::Text, &Ty::Text).name(), "direct");
}

#[test]
fn a_cut_that_a_broadcast_carried_can_be_re_attached() {
    // `Fragment` re-attaches a severed crossing only when a value can still
    // travel it. That test was `out.ty == input.ty`, so before R1593 a
    // copy-paste across a broadcasting wire would have dropped the wire
    // silently — the fragment's own `inbound` records it, and the re-attachment
    // would have refused it.
    let mut f = lattice();
    f.document
        .connect(ROOT, Socket::new(f.level, 0), Socket::new(f.sum, 0))
        .unwrap();
    let fragment = f.document.extract(ROOT, &[f.sum]).unwrap();
    assert_eq!(
        fragment.inbound().len(),
        1,
        "the broadcast wire was cut and recorded"
    );
    let inserted = f
        .document
        .insert(
            ROOT,
            &fragment,
            (600, 300),
            Crossings::KeepInbound,
            Definitions::Share,
        )
        .unwrap();
    assert_eq!(
        inserted.reattached.len(),
        1,
        "and put back, because a value can still cross it"
    );
    let copy = inserted.nodes[0];
    assert_eq!(
        f.document.evaluate(ROOT, copy),
        vec![Some(LVal::Vector([4, 4, 4]))],
        "the pasted node is fed the same broadcast the original was"
    );
}
