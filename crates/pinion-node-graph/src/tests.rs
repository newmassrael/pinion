//! R1577 — the crate's own adversarial fixtures.
//!
//! Every one of these is a hand-written document: no renderer, no window, no
//! pointer. That is the property the crate exists to have.

use serde::{Deserialize, Serialize};

use crate::{
    ConnectError, Crossings, Definitions, Document, DuplicateError, EditPath, ExtractError,
    Fragment, GroupError, InsertError, InterfaceSide, NestError, NodeBody, NodeId, NodeKind,
    PathError, Port, ROOT, Severed, Socket, TreeId, UngroupError, Violation,
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
