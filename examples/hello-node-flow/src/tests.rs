//! R1599/R1600 — the binding's own assertions. The framework's laws are proven
//! in `pinion-node-graph`; these prove the COMPOSITION is real, over the same
//! surface an agent reads.

use super::*;

fn document() -> Graph {
    seed().0
}

/// The seed's node ids, derived rather than written down — the group collapse
/// mints one, so a hand-written table would drift the moment the seed changes.
fn find(graph: &Graph, title: &str) -> NodeId {
    let found = graph
        .tree(TREE)
        .and_then(|t| t.nodes().find(|n| node_title(graph, n.id) == title));
    match found {
        Some(node) => node.id,
        None => panic!("no node titled {title:?}"),
    }
}

#[test]
fn r1600_the_seed_holds_a_control_loop_and_a_value_cycle_through_a_register() {
    let graph = document();
    assert!(
        graph.validate().is_empty(),
        "neither cycle is a violation: {:?}",
        graph.validate()
    );
    assert!(
        graph.cycle_nodes(TREE).is_empty(),
        "the value cycle is broken by the register, so nothing is ON a cycle"
    );
    assert!(
        !graph.control_loops(TREE).is_empty(),
        "and the control loop's members are nameable, statically"
    );
    assert_eq!(
        graph.delays(TREE).len(),
        1,
        "one register — the whole cost of iterating, whatever the count"
    );

    // The seed's closing wire (`bump -> elapsed`) is the one a register makes
    // legal. Take the register out of the loop — two ordinary nodes, same
    // shape — and the very same second wire is refused.
    let mut copy = document();
    let first = copy.add_node(TREE, NodeBody::Kind(Op::Bump), 0, 0).unwrap();
    let second = copy.add_node(TREE, NodeBody::Kind(Op::Bump), 0, 0).unwrap();
    copy.connect(TREE, Socket::new(first, 0), Socket::new(second, 0))
        .expect("the first wire is fine");
    let refused = copy.connect(TREE, Socket::new(second, 0), Socket::new(first, 0));
    assert!(
        matches!(
            refused,
            Err(pinion_node_graph::ConnectError::WouldCycle { .. })
        ),
        "a value cycle with no register on it is still a contradiction: {refused:?}"
    );
}

#[test]
fn r1600_the_loop_runs_to_budget_until_the_register_carries_it_past_the_limit() {
    let graph = document();
    let entry = graph.entry_points(TREE)[0];
    let mut machine = Machine::new();

    // Tick zero: elapsed is 0, which is not over the limit, so the branch loops
    // and the run exhausts its budget. This is R1599's behaviour, unchanged.
    let first = graph.run_on(TREE, entry, STEP_BUDGET, &machine).unwrap();
    assert_eq!(first.stop(), Stop::BudgetExhausted);

    // Now advance the world. The graph did not change; the register did.
    let mut ticks = 0;
    let halted = loop {
        let run = graph.run_on(TREE, entry, STEP_BUDGET, &machine).unwrap();
        if run.stop() == Stop::Halted {
            break run;
        }
        graph.tick(TREE, &mut machine);
        ticks += 1;
        assert!(ticks <= 16, "the scenario should converge long before this");
    };
    assert_eq!(
        ticks,
        usize::try_from(LIMIT + 1).unwrap(),
        "it takes exactly one tick per unit of the authored limit, plus the one \
         that carries it PAST — the answer is a function of the register"
    );
    assert_eq!(
        machine.read(&Instance::root(), find(&graph, "Delay <Number>")),
        Some(&Val::Number(LIMIT + 1))
    );
    assert_eq!(halted.stop(), Stop::Halted);
}

#[test]
fn r1600_arm_one_is_reached_only_after_arm_zero_completes_and_control_descends() {
    let graph = document();
    let entry = graph.entry_points(TREE)[0];
    let mut machine = Machine::new();
    for _ in 0..=LIMIT {
        graph.tick(TREE, &mut machine);
    }
    let run = graph.run_on(TREE, entry, STEP_BUDGET, &machine).unwrap();
    assert_eq!(run.stop(), Stop::Halted);

    // Control crossed a group boundary, and the trace says where it was.
    let entered = run.entered();
    assert_eq!(entered.len(), 1, "one instance was entered: {entered:?}");
    let inside = &entered[0];
    assert_eq!(inside.depth(), 1);
    assert!(
        run.visited().iter().any(|(instance, _)| instance == inside),
        "and steps are attributed to it"
    );
    // The step inside the definition is the drain task — reached only through
    // the tunnel, since it is not in the root tree at all.
    let names: Vec<String> = run
        .visited()
        .iter()
        .map(|(instance, id)| titled(&graph, instance, *id))
        .collect();
    assert!(
        names.contains(&"Task drain".to_owned()),
        "the step inside the group ran: {names:?}"
    );
    assert!(
        names.iter().position(|n| n == "Task settle")
            < names.iter().position(|n| n == "Task drain"),
        "arm 0 settled BEFORE arm 1 was entered — a stack property: {names:?}"
    );
}

#[test]
fn r1600_a_run_does_not_advance_the_world_and_a_tick_does() {
    let graph = document();
    let entry = graph.entry_points(TREE)[0];
    let mut machine = Machine::new();
    let before = machine.clone();
    for _ in 0..4 {
        graph.run_on(TREE, entry, STEP_BUDGET, &machine).unwrap();
    }
    assert_eq!(
        machine, before,
        "four runs and the registers have not moved: a run READS the machine"
    );

    let tick = graph.tick(TREE, &mut machine);
    assert_eq!(tick.at(), 1);
    assert_eq!(tick.changed(), 1);
    assert_ne!(machine, before, "and a tick is what moves it");
}

#[test]
fn r1600_a_register_is_forced_and_the_scenario_jumps_to_its_end() {
    let graph = document();
    let entry = graph.entry_points(TREE)[0];
    let elapsed = find(&graph, "Delay <Number>");
    let mut machine = Machine::new();
    assert_eq!(
        graph
            .run_on(TREE, entry, STEP_BUDGET, &machine)
            .unwrap()
            .stop(),
        Stop::BudgetExhausted
    );

    // The debugger's verb: write the register directly and the scenario is at
    // its end with no ticks taken at all.
    graph
        .force(
            &mut machine,
            &Instance::root(),
            elapsed,
            Val::Number(LIMIT + 1),
        )
        .expect("the register holds a Number");
    assert_eq!(machine.ticks(), 0, "forcing is not ticking");
    assert_eq!(
        graph
            .run_on(TREE, entry, STEP_BUDGET, &machine)
            .unwrap()
            .stop(),
        Stop::Halted
    );
}

#[test]
fn r1599_the_taxonomy_overrides_control_exactly_once() {
    // The provided default is a BEHAVIOUR, not a silence: Fork takes it and
    // gets visual script's Sequence semantics with no code at all.
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
fn r1599_a_pure_node_is_never_in_the_trace_but_its_value_is_read() {
    let graph = document();
    let entry = graph.entry_points(TREE)[0];
    let machine = Machine::new();
    let run = graph.run_on(TREE, entry, STEP_BUDGET, &machine).unwrap();
    let over = find(&graph, "Over budget?");
    assert!(
        !run.trace().contains(&over),
        "a node with no control port is pulled, not run"
    );
    assert_eq!(
        graph.evaluator_on(&machine).outputs(TREE, over),
        vec![Some(Val::Number(0))],
        "and its value is there to be read all the same"
    );
}

#[test]
fn r1599_the_two_planes_refuse_to_mix_in_this_binding() {
    let mut graph = document();
    let begin = graph.entry_points(TREE)[0];
    let over = find(&graph, "Over budget?");
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
    let finish = find(&graph, "Finish");
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
fn r1600_the_view_paints_the_tick_the_registers_and_the_trace() {
    let owner = Owner::new();
    let scene = owner.run(view);
    let text = painted_text(&scene);
    assert!(text.contains("Trace"), "the trace pane is painted");
    assert!(
        text.contains("tick 0"),
        "the world's clock is on screen: {text}"
    );
    assert!(
        text.contains("Delay <Number>"),
        "and the register is named as what it is"
    );
    assert!(
        text.contains("did not run"),
        "a node that never ran says so — the fact a dataflow view cannot show"
    );
    assert!(
        text.contains("pure — pulled, never in the trace"),
        "and a pure node says why it is not in the order"
    );
    assert!(text.contains("budget"), "and why the run stopped");
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
