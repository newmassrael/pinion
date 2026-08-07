//! R1599 — the binding's own assertions. The framework's laws are proven in
//! `pinion-node-graph`; these prove the COMPOSITION is real, over the same
//! surface an agent reads.

use super::*;

fn document() -> Graph {
    seed().0
}

#[test]
fn r1599_the_seed_holds_a_loop_and_is_valid() {
    let graph = document();
    assert!(
        graph.validate().is_empty(),
        "a control loop is not a violation: {:?}",
        graph.validate()
    );
    assert!(
        graph.cycle_nodes(TREE).is_empty(),
        "and it is not a DEPENDENCY cycle either"
    );
    assert!(
        !graph.control_loops(TREE).is_empty(),
        "but the loop's members are nameable, statically"
    );
}

#[test]
fn r1599_the_taxonomy_overrides_control_exactly_once() {
    // The provided default is a BEHAVIOUR, not a silence: Fork takes it and
    // gets Blueprint's Sequence semantics with no code at all.
    let inputs = [None, None];
    assert!(matches!(
        Op::Fork(3).control(&inputs),
        pinion_node_graph::Control::FallThrough
    ));
    assert!(matches!(
        Op::Task("x".into()).control(&inputs),
        pinion_node_graph::Control::FallThrough
    ));
    assert!(matches!(
        Op::Branch.control(&[None, Some(Val::Number(1))]),
        pinion_node_graph::Control::Take(_)
    ));
}

#[test]
fn r1599_a_fork_runs_each_arm_to_completion() {
    let graph = document();
    let entry = graph.entry_points(TREE)[0];
    let run = graph.run(TREE, entry, 100).unwrap();
    let titles: Vec<String> = run
        .trace()
        .iter()
        .map(|id| node_title(&graph, *id))
        .collect();
    // Arm 0 loops (the reading is over budget), so it exhausts before arm 1 is
    // ever reached -- which is exactly what "to completion, then the next"
    // means, and is visible only because the order is derived.
    assert_eq!(titles[0], "Begin");
    assert_eq!(titles[1], "Fork x2");
    assert_eq!(titles[2], "Task warm");
    assert_eq!(run.stop(), Stop::BudgetExhausted);
    assert!(
        !run.trace()
            .iter()
            .any(|id| node_title(&graph, *id) == "Task drain"),
        "arm 1 is never reached, because arm 0 never completes"
    );
}

#[test]
fn r1599_the_branch_reads_the_value_plane_and_the_loop_ends() {
    let mut graph = document();
    // Find the Reading node and drop it under the limit, so the branch takes
    // its False arm and the loop terminates.
    let reading = graph
        .tree(TREE)
        .unwrap()
        .nodes()
        .find(|n| matches!(&n.body, NodeBody::Kind(Op::Reading(_))))
        .unwrap()
        .id;
    graph.set_kind(TREE, reading, Op::Reading(3)).unwrap();

    let entry = graph.entry_points(TREE)[0];
    let run = graph.run(TREE, entry, 100).unwrap();
    assert_eq!(
        run.stop(),
        Stop::Halted,
        "the same graph terminates once the DATUM changes -- the control \
         choice is a function of the value plane"
    );
    let titles: Vec<String> = run
        .trace()
        .iter()
        .map(|id| node_title(&graph, *id))
        .collect();
    assert_eq!(
        titles,
        vec![
            "Begin",
            "Fork x2",
            "Task warm",
            "Branch",
            "Task drain",
            "Finish"
        ],
        "and NOW arm 1 runs, after arm 0 completed"
    );
}

#[test]
fn r1599_a_pure_node_is_never_in_the_trace_but_its_value_is_read() {
    let mut graph = document();
    let reading = graph
        .tree(TREE)
        .unwrap()
        .nodes()
        .find(|n| matches!(&n.body, NodeBody::Kind(Op::Reading(_))))
        .unwrap()
        .id;
    graph.set_kind(TREE, reading, Op::Reading(3)).unwrap();
    let entry = graph.entry_points(TREE)[0];
    let run = graph.run(TREE, entry, 100).unwrap();
    assert!(
        !run.trace().contains(&reading),
        "a node with no control port is pulled, not run"
    );
    let over = graph
        .tree(TREE)
        .unwrap()
        .nodes()
        .find(|n| matches!(&n.body, NodeBody::Kind(Op::Over)))
        .unwrap()
        .id;
    assert!(!run.trace().contains(&over));
    assert_eq!(
        graph.evaluate(TREE, over),
        vec![Some(Val::Number(0))],
        "and its value is there to be read all the same"
    );
}

#[test]
fn r1599_the_two_planes_refuse_to_mix_in_this_binding() {
    let mut graph = document();
    let begin = graph.entry_points(TREE)[0];
    let over = graph
        .tree(TREE)
        .unwrap()
        .nodes()
        .find(|n| matches!(&n.body, NodeBody::Kind(Op::Over)))
        .unwrap()
        .id;
    let refused = graph.connect(TREE, Socket::new(begin, 0), Socket::new(over, 0));
    assert!(
        matches!(
            refused,
            Err(pinion_node_graph::ConnectError::FlowMismatch { .. })
        ),
        "an execution wire cannot feed a number: {refused:?}"
    );
}

#[test]
fn r1599_a_control_output_takes_one_successor_in_this_binding() {
    let mut graph = document();
    let begin = graph.entry_points(TREE)[0];
    let finish = graph
        .tree(TREE)
        .unwrap()
        .nodes()
        .find(|n| matches!(&n.body, NodeBody::Kind(Op::Finish)))
        .unwrap()
        .id;
    let before = graph.tree(TREE).unwrap().links().len();
    let made = graph
        .connect(TREE, Socket::new(begin, 0), Socket::new(finish, 0))
        .unwrap();
    assert!(
        made.displaced.is_some(),
        "Begin already had a successor, and a control output has exactly one"
    );
    assert_eq!(
        graph.tree(TREE).unwrap().links().len(),
        before,
        "so the count is unchanged: one went as one came"
    );
}

#[test]
fn r1599_the_view_paints_the_trace_and_names_what_never_ran() {
    let owner = Owner::new();
    let scene = owner.run(view);
    let text = painted_text(&scene);
    {
        assert!(text.contains("Trace"), "the trace pane is painted");
        assert!(
            text.contains("did not run"),
            "and a node that never ran says so -- the fact a dataflow view \
             cannot show: {text}"
        );
        assert!(
            text.contains("pure — pulled, never in the trace"),
            "and a pure node says why it is not in the order"
        );
        assert!(text.contains("budget"), "and why the run stopped");
    }
}

/// Every string this scene actually paints, which is the only honest source for
/// "what does the screen say" -- R1547's rule, one binding over.
fn painted_text(scene: &Scene) -> String {
    fn walk(scene: &Scene, found: &mut Vec<String>) {
        match scene {
            Scene::Text(node) => found.push(node.content.clone()),
            Scene::Container(node) => {
                for child in &node.children {
                    walk(child, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(scene, &mut found);
    found.join("\n")
}
