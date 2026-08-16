use super::*;
use pinion_core::scene::ExternalNode;
use pinion_core::test_fixtures::assert_out_of_range_saying;
use pinion_core::test_fixtures::assert_refused_saying;
use pinion_core::widgets::wheel::WheelReading;
use pinion_node_graph::Violation;

/// R878 — the idle paint posture (no rename in flight).
const IDLE_TF: RootState = (TextFieldState::Idle, 0);

// ── R1596 model fixtures ──────────────────────────────────────────
//
// A node used to be a struct a test could hand-build with any port list it
// liked. It is a [`Document`] entry now, so a fixture STATES a graph the way a
// user could have reached it: palette kinds at positions, then wires. That is
// the point rather than a cost — a fixture can no longer describe a graph the
// editor cannot produce, which is how three of these tests used to assert
// against shapes the invariants forbid.

/// R1596 — the same graph with one more wire, appended through the WIRE FORM so
/// it can close a cycle.
///
/// `connect` refuses a wire that would make the graph cyclic and names the path,
/// so a cyclic document cannot be built by editing — it only ever arrives from a
/// peer or a file. Building one the way it actually arrives is what makes the
/// cycle tests test the reachable case rather than an impossible one.
fn force_cycle(graph: &Graph, from: NodeId, to: NodeId) -> Graph {
    let mut wire = serde_json::to_value(graph).expect("a document serializes");
    let next = wire["trees"][0]["next_link"]
        .as_u64()
        .expect("a tree mints link ids");
    wire["trees"][0]["links"]
        .as_array_mut()
        .expect("a tree holds links")
        .push(serde_json::json!({
            "id": next,
            "from": {"node": from.0, "port": 0},
            "to": {"node": to.0, "port": 0},
            "muted": false,
        }));
    wire["trees"][0]["next_link"] = serde_json::json!(next + 1);
    serde_json::from_value(wire).expect("the forced document round-trips")
}

/// R1596 — a graph of palette-kind nodes and wires with values AUTHORED on
/// named ports (R1594) — what the eval tests used to express by writing an
/// `input_defaults` vector straight onto a hand-built node.
///
/// Authoring goes through `set_port_value`, so a fixture cannot state a value
/// the port cannot hold: the type gate that protects a live edit protects a test
/// too, and a test asserting on an impossible graph is worse than no test.
fn graph_with(
    nodes: &[(usize, i32, i32)],
    wires: &[(u32, u32, u32, u32)],
    values: &[(u32, PortRef, CellValue)],
) -> Graph {
    let mut graph = graph_of(nodes, wires);
    for (node, port, value) in values {
        graph
            .set_port_value(TREE, NodeId(*node), *port, value.clone())
            .expect("the fixture's authored value fits its port");
    }
    graph
}

/// The palette index of a kind by name — a fixture names what it means rather
/// than carrying the index the palette happens to give it.
fn kind_of(name: &str) -> usize {
    PALETTE
        .iter()
        .position(|&(n, _)| n == name)
        .unwrap_or_else(|| panic!("no palette kind named {name:?}"))
}

/// A graph of `(palette kind, x, y)` nodes, ids minted `0..`, then `(from node,
/// from port, to node, to port)` wires, ids minted `0..`.
///
/// Panics on a wire the model refuses, because a fixture that silently drops one
/// would leave the test asserting about a graph it did not describe.
fn graph_of(nodes: &[(usize, i32, i32)], wires: &[(u32, u32, u32, u32)]) -> Graph {
    let mut graph = Graph::new("fixture");
    for &(kind, x, y) in nodes {
        add_palette_node(&mut graph, kind, x, y).expect("PALETTE kind in range");
    }
    for &(fnode, fport, tnode, tport) in wires {
        graph
            .connect(
                TREE,
                Socket::new(NodeId(fnode), fport),
                Socket::new(NodeId(tnode), tport),
            )
            .expect("the fixture's wire is one the model accepts");
    }
    graph
}

fn boot_scene() -> Scene {
    // Build the primary from `coordinator()` (in-memory storage) rather than
    // `create_external` so a unit test never spins up the real `FileStorage`
    // (which eagerly create_dir_all's the OS data dir).
    Scene::External(
        ExternalNode::new(Box::new(coordinator()) as Box<dyn External>).with_tag(GRAPH_TAG),
    )
}

fn graph_intro(scene: &Scene) -> &dyn ExternalIntrospect {
    scene
        .find_external_with_tag(GRAPH_TAG)
        .and_then(|n| n.handle.introspect())
        .expect("graph external present")
}

fn query_int(scene: &Scene, path: &str) -> i64 {
    match graph_intro(scene).query(path) {
        Ok(IntrospectValue::Int(v)) => v,
        other => panic!("expected Int at {path}, got {other:?}"),
    }
}

/// The `edge.<id>` read (`"<from>:<port>-><to>:<port>"`), or `None` when no
/// edge carries that id — the gesture tests assert a reconnected edge's new
/// wiring and that the grabbed edge's old id is retired.
fn edge_str(scene: &Scene, id: u32) -> Option<String> {
    match graph_intro(scene).query(&format!("edge.{id}")) {
        Ok(IntrospectValue::Text(s)) => Some(s),
        // R1667 — a refusal is the "no such edge" answer this helper models;
        // an `Ok` of any other variant would be the surface contradicting its
        // own declaration, which is worth a panic rather than a silent `None`.
        Err(_) => None,
        other => panic!("expected Text or a refusal at edge.{id}, got {other:?}"),
    }
}

/// Send a pointer wire event through the coordinator (a borrow-scoped
/// helper so the `&mut scene` borrow ends before the next read).
fn send(scene: &mut Scene, wire: &str) {
    let node = scene
        .find_external_with_tag_mut(GRAPH_TAG)
        .expect("present");
    let intro = node.handle.introspect_mut().expect("introspectable");
    let _ = intro.invoke("send", IntrospectValue::Text(wire.to_owned()));
}

// These build the `Option<ActiveEdit>` value an `assert_eq!` compares
// `use_active_edit().get()` against, so the `Some` wrap is the point — not a
// candidate for `unnecessary_wraps`.
/// R918 — an in-flight edit on `target` hosted by the node card (the R878 /
/// R901 surface), the expected `use_active_edit()` value for a card edit.
#[allow(clippy::unnecessary_wraps)]
fn card(target: EditTarget) -> Option<ActiveEdit> {
    Some(ActiveEdit {
        target,
        surface: EditSurface::Card,
    })
}

/// R918 — an in-flight edit on `target` hosted by the Details panel row.
#[allow(clippy::unnecessary_wraps)]
fn panel(target: EditTarget) -> Option<ActiveEdit> {
    Some(ActiveEdit {
        target,
        surface: EditSurface::Panel,
    })
}

#[test]
fn r838_shape_and_defaults() {
    assert_eq!(default_nodes().len(), 4);
    assert_eq!(default_edges().len(), 3);
    Owner::new().run(|| {
        let scene = boot_scene();
        let intro = graph_intro(&scene);
        assert_eq!(intro.query("node_count"), Ok(IntrospectValue::Int(4)));
        assert_eq!(intro.query("edge_count"), Ok(IntrospectValue::Int(3)));
        assert_eq!(intro.query("selected"), Ok(IntrospectValue::Null));
        assert_eq!(
            intro.query("node.2.title"),
            Ok(IntrospectValue::Text("Multiply".to_owned()))
        );
        assert_eq!(intro.query("node.2.inputs"), Ok(IntrospectValue::Int(2)));
        assert_eq!(intro.query("node.3.outputs"), Ok(IntrospectValue::Int(0)));
        assert_eq!(
            intro.query("edge.0"),
            Ok(IntrospectValue::Text("0:0->2:0".to_owned()))
        );
        assert!(
            matches!(
                intro.query("node.9.title"),
                Err(ReadRefusal::NoSuchMember(_))
            ),
            "out-of-range -> Err(ReadRefusal::UnknownPath)"
        );
    });
}

/// R916 — `detail.<field>` reflects the single selected node (the Details
/// panel's selection-relative addressing), and equals the absolute
/// `node.<selected>.<field>` read.
#[test]
fn r916_detail_reflects_single_selected_node() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Nothing selected at boot -> the panel has no node to inspect.
        assert_eq!(
            graph_intro(&scene).query("detail.node"),
            Ok(IntrospectValue::Null)
        );
        assert_eq!(
            graph_intro(&scene).query("detail.title"),
            Ok(IntrospectValue::Null)
        );
        // Select node 2 (Multiply, 2 Vector inputs at x=250).
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            intro
                .intervene("selected_ids", IntrospectValue::Text("2".to_owned()))
                .unwrap();
        }
        let intro = graph_intro(&scene);
        assert_eq!(
            intro.query("detail.node"),
            Ok(IntrospectValue::Int(2)),
            "the single selected id"
        );
        assert_eq!(
            intro.query("detail.title"),
            Ok(IntrospectValue::Text("Multiply".to_owned()))
        );
        assert_eq!(intro.query("detail.x"), Ok(IntrospectValue::Int(250)));
        assert_eq!(intro.query("detail.inputs"), Ok(IntrospectValue::Int(2)));
        // The alias equals the absolute address of the selected node.
        assert_eq!(intro.query("detail.title"), intro.query("node.2.title"));
        assert_eq!(intro.query("detail.x"), intro.query("node.2.x"));
        assert_eq!(
            intro.query("detail.input_default.0"),
            intro.query("node.2.input_default.0")
        );
    });
}

/// R916 — when the selection is not exactly one node, `detail.*` is `Null`
/// (no unambiguous "the" node — the panel shows its placeholder).
#[test]
fn r916_detail_null_when_not_single_selection() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            intro
                .intervene("selected_ids", IntrospectValue::Text("0, 2".to_owned()))
                .unwrap();
        }
        let intro = graph_intro(&scene);
        assert_eq!(
            intro.query("detail.node"),
            Ok(IntrospectValue::Null),
            "multi-select has no single detail node"
        );
        assert_eq!(intro.query("detail.title"), Ok(IntrospectValue::Null));
        assert_eq!(intro.query("detail.x"), Ok(IntrospectValue::Null));
        assert_eq!(
            intro.query("detail.input_default.0"),
            Ok(IntrospectValue::Null)
        );
    });
}

/// R916 — `intervene detail.<field>` writes the *selected* node through the
/// identical funnel `node.<id>.<field>` uses (rename / move), and is rejected
/// when the selection is not exactly one node.
#[test]
fn r916_detail_intervene_edits_the_selected_node() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            intro
                .intervene("selected_ids", IntrospectValue::Text("0".to_owned()))
                .unwrap();
            // The Details panel's AI-first edits route to node 0.
            assert!(
                intro
                    .intervene("detail.title", IntrospectValue::Text("Albedo".to_owned()))
                    .is_ok()
            );
            assert!(
                intro
                    .intervene("detail.x", IntrospectValue::Int(88))
                    .is_ok()
            );
        }
        {
            let intro = graph_intro(&scene);
            assert_eq!(
                intro.query("node.0.title"),
                Ok(IntrospectValue::Text("Albedo".to_owned())),
                "detail.title wrote node 0"
            );
            assert_eq!(intro.query("node.0.x"), Ok(IntrospectValue::Int(88)));
            assert_eq!(
                intro.query("detail.title"),
                Ok(IntrospectValue::Text("Albedo".to_owned())),
                "the panel reflects the edit"
            );
        }
        // Clearing the single-selection makes a detail write unaddressable.
        let intro = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .unwrap()
            .handle
            .introspect_mut()
            .unwrap();
        intro
            .intervene("selected_ids", IntrospectValue::Text("0, 1".to_owned()))
            .unwrap();
        assert_eq!(
            intro.intervene("detail.title", IntrospectValue::Text("X".to_owned())),
            Err(InterveneError::UnknownPath),
            "a detail write with no single selection is rejected",
        );
    });
}

/// R917 — the `detail.*` mirror is *complete*: every readable
/// `node.<id>.<field>` resolves through `detail.<field>` with the identical
/// value, AND the schema declares the full set (the AI-first contract
/// matches the resolvable surface — no silently-undeclared path).
/// `detail.node` is a read-only selection mirror.
#[test]
fn r917_detail_mirror_is_complete_and_schema_declared() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            intro
                .intervene("selected_ids", IntrospectValue::Text("2".to_owned()))
                .unwrap(); // Multiply
        }
        let intro = graph_intro(&scene);
        // Parity: the previously-undeclared fields resolve and match the
        // absolute address of the selected node.
        assert_eq!(intro.query("detail.outputs"), intro.query("node.2.outputs"));
        assert_eq!(
            intro.query("detail.input_types"),
            intro.query("node.2.input_types")
        );
        assert_eq!(
            intro.query("detail.output_types"),
            intro.query("node.2.output_types")
        );
        assert_eq!(
            intro.query("detail.outputs"),
            Ok(IntrospectValue::Int(1)),
            "Multiply has 1 output"
        );
        // The schema declares the full mirror (no undeclared-but-resolvable path).
        let fields: Vec<&str> = intro.schema().fields.iter().map(|f| f.path).collect();
        for f in [
            "detail.outputs",
            "detail.input_types",
            "detail.output_types",
        ] {
            assert!(
                fields.contains(&f),
                "{f} must be schema-declared (mirrors node.<id>.*)"
            );
        }
        // `detail.node` is read-only: write the selection via `selected_ids`.
        let intro = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .unwrap()
            .handle
            .introspect_mut()
            .unwrap();
        assert_eq!(
            intro.intervene("detail.node", IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath),
            "detail.node is a read-only selection mirror",
        );
    });
}

#[test]
fn r838_intervene_moves_node_clamped() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        assert!(
            intro
                .intervene("node.0.x", IntrospectValue::Int(120))
                .is_ok()
        );
        assert!(
            intro
                .intervene("node.0.y", IntrospectValue::Int(90))
                .is_ok()
        );
        assert_eq!(intro.query("node.0.x"), Ok(IntrospectValue::Int(120)));
        assert_eq!(intro.query("node.0.y"), Ok(IntrospectValue::Int(90)));
        // An out-of-world request clamps to the WORLD extent (R877: the
        // canvas pans, so the clamp is the world edge, not the window).
        assert!(
            intro
                .intervene("node.0.x", IntrospectValue::Int(99999))
                .is_ok()
        );
        let x = intro.query("node.0.x");
        assert_eq!(x, Ok(IntrospectValue::Int(i64::from(clamp_node_x(99999)))));
    });
}

#[test]
fn r838_intervene_readonly_and_typed() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // R878 — `title` became the undoable rename write-twin; the
        // structural port arity stays read-only.
        assert_eq!(
            intro.intervene("node.0.inputs", IntrospectValue::Int(2)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            intro.intervene("node.0.x", IntrospectValue::Text("no".to_owned())),
            Err(InterveneError::TypeMismatch),
        );
        assert_eq!(
            intro.intervene("node.9.x", IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath),
        );
    });
}

#[test]
fn r838_add_edge_validates_and_dedups_input() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // Self-loop rejected.
        assert_eq!(
            intro.invoke("add_edge", IntrospectValue::Text("2,0,2,0".to_owned())),
            Ok(IntrospectValue::Bool(false)),
        );
        // Out-of-range port rejected.
        assert_eq!(
            intro.invoke("add_edge", IntrospectValue::Text("0,5,3,0".to_owned())),
            Ok(IntrospectValue::Bool(false)),
        );
        // A new valid edge into Output's only input replaces edge id 2's
        // target; the new wire mints a fresh id (3), edge id 2 is gone.
        assert_eq!(
            intro.invoke("add_edge", IntrospectValue::Text("0,0,3,0".to_owned())),
            Ok(IntrospectValue::Bool(true)),
        );
        // Input (3,0) now has exactly one wire — still 3 edges total.
        assert_eq!(intro.query("edge_count"), Ok(IntrospectValue::Int(3)));
        assert!(
            matches!(intro.query("edge.2"), Err(ReadRefusal::NoSuchMember(_))),
            "old wire id 2 was replaced"
        );
        assert_eq!(
            intro.query("edge.3"),
            Ok(IntrospectValue::Text("0:0->3:0".to_owned()))
        );
    });
}

#[test]
fn r898_port_type_lattice_is_a_strict_partial_order() {
    // Exact match always; a scalar `Float` broadcasts up to a `Vector`;
    // there is no narrowing, so the relation is asymmetric.
    assert!(PortType::Float.is_assignable_to(PortType::Float));
    assert!(PortType::Vector.is_assignable_to(PortType::Vector));
    assert!(
        PortType::Float.is_assignable_to(PortType::Vector),
        "scalar broadcast"
    );
    assert!(
        !PortType::Vector.is_assignable_to(PortType::Float),
        "no vector->scalar narrowing"
    );
}

#[test]
fn r898_add_edge_rejects_a_type_incompatible_wire() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // A `Scalar` (Float source, palette 5) and a `Lerp`
        // (`[Vector, Vector, Float]` in, palette 6) — the typed sources/ops.
        let scalar = coord.add_node(5).expect("Scalar is a valid kind");
        let lerp = coord.add_node(6).expect("Lerp is a valid kind");
        let before = coord.edges().len();
        // Float -> Float (Lerp's factor input): exact, accepted.
        assert!(
            coord.add_edge(scalar, 0, lerp, 2),
            "Float -> Float exact is accepted"
        );
        // Float -> Vector (Lerp's colour input): scalar broadcast, accepted.
        assert!(
            coord.add_edge(scalar, 0, lerp, 0),
            "Float -> Vector broadcast is accepted"
        );
        // Vector -> Float (Texture's colour into the factor input): narrowing,
        // REJECTED — the typed gate the pre-R898 arity check could not make.
        assert!(
            !coord.add_edge(NodeId(0), 0, lerp, 2),
            "Vector -> Float narrowing is rejected"
        );
        // Only the two accepted wires were added.
        assert_eq!(
            coord.edges().len(),
            before + 2,
            "exactly the compatible wires landed"
        );
    });
}

#[test]
fn r898_typed_ports_are_ai_readable_and_read_only() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // Multiply (node 2): two `Vector` inputs, one `Vector` output.
        assert_eq!(
            intro.query("node.2.input_types"),
            Ok(IntrospectValue::Text("Vector,Vector".to_owned())),
        );
        assert_eq!(
            intro.query("node.2.output_types"),
            Ok(IntrospectValue::Text("Vector".to_owned())),
        );
        // Texture (node 0): a source — no input types.
        assert_eq!(
            intro.query("node.0.input_types"),
            Ok(IntrospectValue::Text(String::new())),
        );
        // The typed-port lists are read-only (ports are the node kind's).
        assert_eq!(
            intro.intervene(
                "node.2.input_types",
                IntrospectValue::Text("Float".to_owned())
            ),
            Err(InterveneError::ReadOnly),
        );
    });
}

#[test]
fn r899_input_port_default_is_typed_by_port_type() {
    // A Vector port defaults to a colour, a Float port to a scalar.
    assert!(matches!(
        PortType::Vector.default_value(),
        CellValue::Color(_)
    ));
    assert_eq!(PortType::Float.default_value(), CellValue::Float(0.0));
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Multiply (node 2) input 0 is a Vector port -> a Color default.
        assert!(
            matches!(coord.port_default(NodeId(2), 0), Some(CellValue::Color(_))),
            "Vector input default is a Color"
        );
        // Lerp's input 2 is the Float factor -> a Float default.
        let lerp = coord.add_node(6).expect("Lerp");
        assert_eq!(
            coord.port_default(lerp, 2),
            Some(CellValue::Float(0.0)),
            "Float input default is 0.0"
        );
    });
}

#[test]
fn r899_input_default_is_ai_read_write_and_type_checked() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // The Vector input's default reads as a Color object.
        let Ok(IntrospectValue::Json(j)) = intro.query("node.2.input_default.0") else {
            panic!("Vector default reads as a JSON colour object");
        };
        assert_eq!(
            j.get("r").and_then(serde_json::Value::as_u64),
            Some(0x80),
            "default grey r=0x80"
        );
        // A typed write takes a hex string and reads back the parsed channels.
        assert_eq!(
            intro.intervene(
                "node.2.input_default.0",
                IntrospectValue::Text("#3366cc".to_owned())
            ),
            Ok(()),
        );
        let Ok(IntrospectValue::Json(j)) = intro.query("node.2.input_default.0") else {
            panic!("re-read after the typed write");
        };
        assert_eq!(
            j.get("r").and_then(serde_json::Value::as_u64),
            Some(0x33),
            "written r"
        );
        assert_eq!(
            j.get("b").and_then(serde_json::Value::as_u64),
            Some(0xcc),
            "written b"
        );
        // The wrong value type for a Color port is rejected (no float into a colour).
        assert_eq!(
            intro.intervene("node.2.input_default.0", IntrospectValue::Float(1.0)),
            Err(InterveneError::TypeMismatch),
        );
        // An out-of-range port: query is absent, write is UnknownPath.
        assert!(matches!(
            intro.query("node.2.input_default.9"),
            Err(ReadRefusal::NoSuchMember(_))
        ),);
        assert_eq!(
            intro.intervene(
                "node.2.input_default.9",
                IntrospectValue::Text("#000000".to_owned())
            ),
            Err(InterveneError::UnknownPath),
        );
    });
}

#[test]
fn r899_set_port_default_is_undoable() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let red = CellValue::Color(Color::rgb(0xff, 0x00, 0x00));
        let grey = coord.port_default(NodeId(2), 0).expect("default");
        assert!(apply_set_node_value(
            &use_graph_handle(),
            NodeId(2),
            NodeValueTarget::InputDefault(0),
            red.clone()
        ));
        assert_eq!(stack.len(), 1, "a default change is one undo step");
        assert_eq!(coord.port_default(NodeId(2), 0), Some(red.clone()));
        // Re-setting the same value is a no-op (no extra undo step).
        assert!(apply_set_node_value(
            &use_graph_handle(),
            NodeId(2),
            NodeValueTarget::InputDefault(0),
            red.clone()
        ));
        assert_eq!(stack.len(), 1, "an unchanged write journals nothing");
        assert!(stack.undo(), "undo restores the prior default");
        assert_eq!(coord.port_default(NodeId(2), 0), Some(grey.clone()));
        assert!(stack.redo(), "redo re-applies it");
        assert_eq!(coord.port_default(NodeId(2), 0), Some(red.clone()));
    });
}

#[test]
fn r900_setting_a_nan_float_default_twice_is_idempotent() {
    // R900 audit fix: the no-op guard compares by total order, so a repeat
    // write of the SAME value journals nothing — even for `Float(NaN)`,
    // which the derived IEEE `PartialEq` would have treated as `!=` itself
    // and journaled a spurious second undo step.
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let lerp = coord.add_node(6).expect("Lerp"); // input 2 is a Float port
        let nan = CellValue::Float(f64::NAN);
        assert!(
            apply_set_node_value(
                &use_graph_handle(),
                lerp,
                NodeValueTarget::InputDefault(2),
                nan.clone()
            ),
            "first NaN set journals"
        );
        let after_first = stack.len();
        assert!(
            apply_set_node_value(
                &use_graph_handle(),
                lerp,
                NodeValueTarget::InputDefault(2),
                nan
            ),
            "repeat NaN is a no-op"
        );
        assert_eq!(
            stack.len(),
            after_first,
            "an unchanged NaN write journals nothing"
        );
    });
}

// ─── R901 inline port-default editor ───────────────────────────

/// R901 — the inline editor opens on an input port default, seeds from the
/// port's `edit_text`, and commits the typed value through the SAME
/// `apply_set_default` SSOT the AI write uses (one undoable step), then
/// wipes the shared field. A Lerp's port 2 is a Float, so it is text-edited
/// inline (the R899-deferred axis, now landed).
#[test]
fn r901_port_default_inline_editor_begins_seeds_and_commits() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let lerp = coord.add_node(6).expect("Lerp"); // input 2 = Float port
        assert!(
            coord.begin_edit_default(lerp, 2),
            "the Float pin default opens for edit"
        );
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::PortDefault {
                node: lerp,
                port: 2
            }),
            "the editor targets the Float pin default",
        );
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "0",
            "seeded with the current default"
        );
        use_text_edit_state(EDIT_TF_TAG).set_text("0.75".to_owned());
        commit_edit(true);
        assert_eq!(use_active_edit().get(), None, "commit leaves edit mode");
        assert_eq!(
            coord.port_default(lerp, 2),
            Some(CellValue::Float(0.75)),
            "the typed value parsed and applied",
        );
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "",
            "field wiped for the next edit"
        );
        assert_eq!(
            stack.undo_label().as_deref(),
            Some("Set port default"),
            "journaled undoably"
        );
        assert!(stack.undo());
        assert_eq!(
            coord.port_default(lerp, 2),
            Some(CellValue::Float(0.0)),
            "undo restores the prior default",
        );
    });
}

/// R901 — the editor's keystroke gate is the TARGET's `CellKind`: text for
/// a title, the port's typed kind for a pin default (a Float pin accepts a
/// number, a Vector pin a `#RRGGBB[AA]` hex). The single funnel that keeps
/// the keyboard editor and the AI write from drifting on what a port holds.
#[test]
fn r901_port_default_editor_uses_the_port_typed_keystroke_gate() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let lerp = coord.add_node(6).expect("Lerp"); // ports [Vector, Vector, Float]
        assert_eq!(
            edit_target_kind(EditTarget::Title(NodeId(2))),
            CellKind::Text,
            "a title is plain text"
        );
        assert_eq!(
            edit_target_kind(EditTarget::PortDefault {
                node: lerp,
                port: 2
            }),
            CellKind::Float,
            "a Float pin is number-gated",
        );
        assert_eq!(
            edit_target_kind(EditTarget::PortDefault {
                node: lerp,
                port: 0
            }),
            CellKind::Color,
            "a Vector pin is hex-gated",
        );
    });
}

/// R901 — a Vector pin default is a `Color`, edited inline as a
/// `#RRGGBB[AA]` hex through the same `CellValue::edit_text` seed /
/// `CellKind::parse` commit the property-grid colour cell uses.
#[test]
fn r901_port_default_commit_parses_color_hex() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let lerp = coord.add_node(6).expect("Lerp"); // port 0 = Vector -> Color default
        assert!(coord.begin_edit_default(lerp, 0));
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "#808080",
            "seeded with the grey default's hex",
        );
        use_text_edit_state(EDIT_TF_TAG).set_text("#3366cc".to_owned());
        commit_edit(true);
        assert_eq!(
            coord.port_default(lerp, 0),
            Some(CellValue::Color(Color::rgb(0x33, 0x66, 0xcc))),
            "the typed hex parsed into the colour default",
        );
    });
}

/// R901 — a malformed numeric commit keeps the prior default (no data loss,
/// no spurious undo step — the `CellKind::parse` contract the data-grid
/// editor follows). The keystroke gate normally blocks such input, but the
/// commit parse is the backstop (e.g. an IME-pasted run).
#[test]
fn r901_malformed_port_default_commit_keeps_prior_value() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let lerp = coord.add_node(6).expect("Lerp");
        assert!(coord.begin_edit_default(lerp, 2)); // Float port
        // The `add_node` above is itself one undo step; the rejected commit
        // must add none.
        let before = stack.len();
        use_text_edit_state(EDIT_TF_TAG).set_text("abc".to_owned());
        commit_edit(true);
        assert_eq!(
            coord.port_default(lerp, 2),
            Some(CellValue::Float(0.0)),
            "a malformed commit keeps the prior default",
        );
        assert_eq!(stack.len(), before, "no undo step for a rejected parse");
    });
}

/// R901 — `query editing` is the honest generalised read (`{ kind, node,
/// port? }` or Null); `query renaming` survives as its title-only
/// projection — a port-default edit is honestly NOT a rename, so `renaming`
/// is Null then while `editing` reports the port-default target.
#[test]
fn r901_editing_read_and_renaming_degenerate_projection() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Idle: both reads are Null.
        assert_eq!(
            graph_intro(&scene).query("editing"),
            Ok(IntrospectValue::Null)
        );
        assert_eq!(
            graph_intro(&scene).query("renaming"),
            Ok(IntrospectValue::Null)
        );
        // A title edit: `editing` is a title object, `renaming` is the id.
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            assert_eq!(
                intro.invoke("begin_rename", IntrospectValue::Int(2)),
                Ok(IntrospectValue::Bool(true))
            );
        }
        let Ok(IntrospectValue::Json(j)) = graph_intro(&scene).query("editing") else {
            panic!("editing reads as a JSON object for a title edit");
        };
        assert_eq!(
            j.get("kind").and_then(serde_json::Value::as_str),
            Some("title")
        );
        assert_eq!(j.get("node").and_then(serde_json::Value::as_u64), Some(2));
        assert_eq!(
            graph_intro(&scene).query("renaming"),
            Ok(IntrospectValue::Int(2))
        );
        // A port-default edit: `editing` is a port_default object, but
        // `renaming` is Null (the degenerate projection). R901.1 — a wired
        // pin rejects the inline editor, so edit a fresh Lerp's UNWIRED
        // Float pin (node 2's pins are both wired in the seed graph).
        let lerp = {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            let Ok(IntrospectValue::Int(id)) =
                intro.invoke("add_node", IntrospectValue::Text("Lerp".to_owned()))
            else {
                panic!("add_node returns the new id");
            };
            assert_eq!(
                intro.invoke(
                    "begin_edit_default",
                    IntrospectValue::Text(format!("{id}.2"))
                ),
                Ok(IntrospectValue::Bool(true)),
            );
            id
        };
        let Ok(IntrospectValue::Json(j)) = graph_intro(&scene).query("editing") else {
            panic!("editing reads as a JSON object for a port-default edit");
        };
        assert_eq!(
            j.get("kind").and_then(serde_json::Value::as_str),
            Some("port_default")
        );
        assert_eq!(
            j.get("node").and_then(serde_json::Value::as_u64),
            u64::try_from(lerp).ok()
        );
        assert_eq!(j.get("port").and_then(serde_json::Value::as_u64), Some(2));
        assert_eq!(
            graph_intro(&scene).query("renaming"),
            Ok(IntrospectValue::Null),
            "a port-default edit is not a rename",
        );
    });
}

/// R901 — the port-default editor opens from BOTH the AI-first
/// `invoke begin_edit_default` (an unknown node / out-of-range port is
/// rejected, graph unchanged) and a double-click on the pin's default
/// label, and the open field paints + lowers to a "Port default" textbox.
#[test]
fn r901_begin_edit_default_via_invoke_and_double_click() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let coord = coordinator();
        let lerp = coord.add_node(6).expect("Lerp"); // a fresh node: its pins are unwired
        // Invoke entry: a valid pin opens, a bad node / port is rejected.
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            assert_eq!(
                intro.invoke(
                    "begin_edit_default",
                    IntrospectValue::Text(format!("{}.2", lerp.0))
                ),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                intro.invoke(
                    "begin_edit_default",
                    IntrospectValue::Text("99.0".to_owned())
                ),
                Ok(IntrospectValue::Bool(false)),
                "an unknown node is rejected",
            );
            assert_eq!(
                intro.invoke(
                    "begin_edit_default",
                    IntrospectValue::Text(format!("{}.9", lerp.0))
                ),
                Ok(IntrospectValue::Bool(false)),
                "an out-of-range port is rejected",
            );
        }
        // The open field paints over the pin and lowers to a textbox named
        // "Port default" (the paint gate == the a11y gate).
        let painted = view((TextFieldState::Editing, 0), &Frame::new());
        assert!(
            painted.contains_tag(EDIT_TF_TAG),
            "the shared field paints over the pin default"
        );
        let a11y = NodeEditorView::access_node(&(TextFieldState::Editing, 0), Some(EDIT_TF_TAG));
        let textbox = a11y
            .iter()
            .find(|n| n.tag == EDIT_TF_TAG)
            .expect("the pin default lowers to a textbox");
        assert_eq!(textbox.role, AriaRole::TextInput);
        assert_eq!(
            textbox.name.as_deref(),
            Some("Port default"),
            "named for the port-default edit kind"
        );
        // Cancel, then a double-click on the pin's default label re-opens it.
        cancel_edit();
        assert_eq!(use_active_edit().get(), None);
        send(&mut scene, &format!("idefault_{}_2:DoubleClick", lerp.0));
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::PortDefault {
                node: lerp,
                port: 2
            }),
            "double-clicking the pin default opens its editor",
        );
    });
}

/// R901 — beginning a *different* edit target commits the in-flight one first
/// (the toolkit item-view discipline), ACROSS kinds: a typed port default
/// commits when a title rename opens. The single `apply_edit_commit` funnel routes each target
/// to its field SSOT.
#[test]
fn r901_begin_edit_migration_commits_in_flight_across_targets() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let lerp = coord.add_node(6).expect("Lerp");
        assert!(coord.begin_edit_default(lerp, 2)); // open the Float pin
        use_text_edit_state(EDIT_TF_TAG).set_text("1.5".to_owned());
        // Opening a title rename commits the in-flight port default first.
        assert!(coord.begin_rename(NodeId(2)));
        assert_eq!(
            coord.port_default(lerp, 2),
            Some(CellValue::Float(1.5)),
            "the in-flight port default committed on migration",
        );
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::Title(NodeId(2))),
            "the editor migrated to the title target",
        );
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "Multiply",
            "reseeded from the new target"
        );
    });
}

/// R901.1 (session-review audit) — the inline editor must NOT open on a
/// wired port: its anchor is the default LABEL, which paints only for an
/// unwired pin (the edge supplies a wired pin's value). Opening it there
/// would paint nothing yet grab focus and advertise an a11y textbox with no
/// painted peer (the paint gate == a11y gate invariant, R873/R874).
#[test]
fn r901_1_begin_edit_default_rejects_a_wired_port() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        // Seed graph: node 2 (Multiply) input 0 is wired by edge 0 (0:0->2:0).
        assert!(
            coord.input_wired(NodeId(2), 0),
            "node 2 port 0 is wired in the seed graph"
        );
        assert!(
            !coord.begin_edit_default(NodeId(2), 0),
            "a wired port rejects the inline editor"
        );
        assert_eq!(
            use_active_edit().get(),
            None,
            "the rejected begin left no edit in flight"
        );
        // The a11y tree advertises no textbox while idle (the gate that the
        // wired-port begin must not falsely trip).
        let a11y = NodeEditorView::access_node(&IDLE_TF, Some(GRAPH_TAG));
        assert!(
            a11y.iter().all(|n| n.tag != EDIT_TF_TAG),
            "no unpainted textbox advertised"
        );
        // An UNWIRED port (a fresh Lerp's pin) still opens normally.
        let lerp = coord.add_node(6).expect("Lerp");
        assert!(
            !coord.input_wired(lerp, 2),
            "the fresh Lerp's pin is unwired"
        );
        assert!(
            coord.begin_edit_default(lerp, 2),
            "an unwired port opens the editor"
        );
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::PortDefault {
                node: lerp,
                port: 2
            }),
        );
    });
}

/// R918 — a Details-panel field key maps to the [`EditTarget`] it edits (the
/// inverse of the `detail_<key>` row tags). Unknown / malformed keys reject.
#[test]
fn r918_detail_edit_target_maps_field_keys() {
    let id = NodeId(7);
    assert_eq!(detail_edit_target(id, "title"), Some(EditTarget::Title(id)));
    assert_eq!(detail_edit_target(id, "x"), Some(EditTarget::PosX(id)));
    assert_eq!(detail_edit_target(id, "y"), Some(EditTarget::PosY(id)));
    assert_eq!(
        detail_edit_target(id, "in_3"),
        Some(EditTarget::PortDefault { node: id, port: 3 })
    );
    assert_eq!(
        detail_edit_target(id, "bogus"),
        None,
        "an unknown key rejects"
    );
    assert_eq!(
        detail_edit_target(id, "in_x"),
        None,
        "a non-numeric port rejects"
    );
}

/// R918 — a position edit is integer-gated and seeds from the node's current
/// coordinate; the begin sets surface = Panel.
#[test]
fn r918_pos_edit_target_kind_and_seed() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        assert_eq!(edit_target_kind(EditTarget::PosX(NodeId(2))), CellKind::Int);
        assert_eq!(edit_target_kind(EditTarget::PosY(NodeId(2))), CellKind::Int);
        coord.select_node(Some(NodeId(2))); // Multiply, at (250, 110)
        assert!(coord.begin_edit_detail("x"));
        assert_eq!(use_active_edit().get(), panel(EditTarget::PosX(NodeId(2))));
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "250",
            "seeded from node.x"
        );
        cancel_edit();
        assert!(coord.begin_edit_detail("y"));
        assert_eq!(use_active_edit().get(), panel(EditTarget::PosY(NodeId(2))));
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "110",
            "seeded from node.y"
        );
    });
}

/// R918 — a Details-panel row click (the `detail_<key>` wire) opens the inline
/// editor on the selected node's property; the field paints in the panel and
/// `query editing` reports the kind + the `panel` surface.
#[test]
fn r918_panel_row_click_opens_editor() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            intro
                .intervene("selected_ids", IntrospectValue::Text("2".to_owned()))
                .unwrap();
        }
        send(&mut scene, "detail_x:PointerDown");
        send(&mut scene, "detail_x:PointerUp");
        assert_eq!(
            use_active_edit().get(),
            panel(EditTarget::PosX(NodeId(2))),
            "the panel row opened a PosX edit"
        );
        let Ok(IntrospectValue::Json(j)) = graph_intro(&scene).query("editing") else {
            panic!("editing reads as json while an edit is in flight");
        };
        assert_eq!(j["kind"], serde_json::json!("pos_x"));
        assert_eq!(
            j["surface"],
            serde_json::json!("panel"),
            "the surface is the Details panel"
        );
        assert_eq!(j["node"], serde_json::json!(2));
        let painted = view((TextFieldState::Editing, 0), &Frame::new());
        assert!(
            painted.contains_tag(EDIT_TF_TAG),
            "the shared field paints (in the panel row)"
        );
    });
}

/// R918 — committing a panel position edit moves the node through the SAME
/// `apply_set_pos` funnel the `intervene node.<id>.{x,y}` arm uses: an
/// undoable move, and a panel `x` then `y` edit coalesce into ONE undo step.
#[test]
fn r918_panel_pos_commit_shares_move_funnel() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        coord.select_node(Some(NodeId(2)));
        assert!(coord.begin_edit_detail("x"));
        use_text_edit_state(EDIT_TF_TAG).set_text("400".to_owned());
        commit_edit(true);
        assert_eq!(
            coord.node_by_id(NodeId(2)).map(|n| n.x),
            Some(400),
            "the panel x edit moved the node"
        );
        assert_eq!(use_active_edit().get(), None, "commit leaves edit mode");
        assert_eq!(
            use_undo().undo_label().as_deref(),
            Some("Move node"),
            "the panel move is undoable"
        );
        assert!(coord.begin_edit_detail("y"));
        use_text_edit_state(EDIT_TF_TAG).set_text("200".to_owned());
        commit_edit(true);
        assert_eq!(
            coord.node_by_id(NodeId(2)).map(|n| (n.x, n.y)),
            Some((400, 200))
        );
        // One undo reverts BOTH axes — the two single-axis commits coalesced,
        // exactly like `intervene x` then `intervene y` (the shared funnel).
        assert!(use_undo().undo(), "undo the coalesced move");
        assert_eq!(
            coord.node_by_id(NodeId(2)).map(|n| (n.x, n.y)),
            Some((250, 110)),
            "x and y reverted in one undo step",
        );
        assert!(
            !use_undo().can_undo(),
            "the two panel edits coalesced into one undo step"
        );
    });
}

/// R918 — a malformed coordinate keeps the prior position (the `CellKind::Int`
/// no-data-loss discipline); the commit is a no-op with no undo step.
#[test]
fn r918_panel_pos_commit_malformed_keeps_prior() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        coord.select_node(Some(NodeId(2)));
        assert!(coord.begin_edit_detail("x"));
        use_text_edit_state(EDIT_TF_TAG).set_text("-".to_owned()); // a lone sign: not an i32
        commit_edit(true);
        assert_eq!(
            coord.node_by_id(NodeId(2)).map(|n| n.x),
            Some(250),
            "the position is unchanged"
        );
        assert!(!use_undo().can_undo(), "no undo step for a rejected value");
    });
}

/// R918 — the Details panel edits a node's title and (unlike the card) a
/// WIRED port's default: a panel row always paints its value, so the field has
/// a painted anchor even for a wired pin the card hides (no R901.1 risk).
#[test]
fn r918_panel_edits_title_and_wired_port_default() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        coord.select_node(Some(NodeId(2)));
        assert!(coord.begin_edit_detail("title"));
        assert_eq!(use_active_edit().get(), panel(EditTarget::Title(NodeId(2))));
        use_text_edit_state(EDIT_TF_TAG).set_text("Albedo".to_owned());
        commit_edit(true);
        assert_eq!(
            coord.node_by_id(NodeId(2)).map(|n| n.title()),
            Some("Albedo".to_owned())
        );
        // Node 2 port 0 is wired (edge 0: 0:0->2:0): the CARD rejects it...
        assert!(coord.input_wired(NodeId(2), 0));
        assert!(
            !coord.begin_edit_default(NodeId(2), 0),
            "the card rejects a wired pin"
        );
        // ...but the PANEL edits it (the row paints regardless of wiring).
        assert!(
            coord.begin_edit_detail("in_0"),
            "the panel edits a wired port default"
        );
        assert_eq!(
            use_active_edit().get(),
            panel(EditTarget::PortDefault {
                node: NodeId(2),
                port: 0
            }),
        );
    });
}

/// R918 — a panel edit needs a single selected node and a known field key.
#[test]
fn r918_begin_edit_detail_rejects_no_or_multi_selection() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        assert!(!coord.begin_edit_detail("title"), "no selection rejects");
        coord.select_node(Some(NodeId(2)));
        coord.add_node_to_selection(NodeId(0));
        assert!(
            !coord.begin_edit_detail("title"),
            "a multi-selection has no single node"
        );
        coord.select_node(Some(NodeId(2)));
        assert!(
            !coord.begin_edit_detail("bogus"),
            "an unknown field key rejects"
        );
        assert_eq!(use_active_edit().get(), None, "no edit left in flight");
    });
}

/// R918 — the `begin_edit_detail` RPC invoke opens the panel inline editor
/// (the twin of a panel-row click, symmetric with `begin_rename` /
/// `begin_edit_default` for the card): the `editing` read then reports the
/// `panel` surface, so the surface a human can reach by clicking is also
/// RPC-reachable ([[wire-form-read-write-symmetry]]).
#[test]
fn r918_begin_edit_detail_rpc_pair() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            intro
                .intervene("selected_ids", IntrospectValue::Text("2".to_owned()))
                .unwrap();
            assert_eq!(
                intro.invoke("begin_edit_detail", IntrospectValue::Text("y".to_owned())),
                Ok(IntrospectValue::Bool(true)),
                "the RPC opens the Position Y panel editor",
            );
        }
        assert_eq!(use_active_edit().get(), panel(EditTarget::PosY(NodeId(2))));
        let Ok(IntrospectValue::Json(j)) = graph_intro(&scene).query("editing") else {
            panic!("editing reads as json");
        };
        assert_eq!(
            j["surface"],
            serde_json::json!("panel"),
            "the RPC-opened edit reads as the panel surface"
        );
        // An unknown field key is Rejected (`false`); a non-Text arg is a type
        // mismatch — the `begin_rename` / `begin_edit_default` reject shape.
        let intro = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .unwrap()
            .handle
            .introspect_mut()
            .unwrap();
        assert_eq!(
            intro.invoke(
                "begin_edit_detail",
                IntrospectValue::Text("bogus".to_owned())
            ),
            Ok(IntrospectValue::Bool(false)),
            "an unknown field key is rejected",
        );
        assert_eq!(
            intro.invoke("begin_edit_detail", IntrospectValue::Int(2)),
            Err(InvokeError::TypeMismatch)
        );
    });
}

/// R918 — while a panel edit is in flight, the editor textbox lowers under the
/// panel ROW (not a node card) — the paint==a11y one-gate, re-parented by
/// surface (R873 / R901.1).
#[test]
fn r918_panel_edit_a11y_hosts_textbox_under_row_not_card() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        coord.select_node(Some(NodeId(2)));
        assert!(coord.begin_edit_detail("x"));
        let a11y = NodeEditorView::access_node(&(TextFieldState::Editing, 0), Some(EDIT_TF_TAG));
        let textbox = a11y
            .iter()
            .find(|n| n.tag == EDIT_TF_TAG)
            .expect("the panel hosts the editor textbox");
        assert_eq!(textbox.role, AriaRole::TextInput);
        let row_tag = format!("{GRAPH_TAG}#detail_x");
        let row = a11y
            .iter()
            .find(|n| n.tag == row_tag)
            .expect("the Position X row node");
        assert!(
            row.children.iter().any(|c| c.as_str() == EDIT_TF_TAG),
            "the editor is the panel row's child"
        );
        let card = a11y
            .iter()
            .find(|n| n.tag == format!("{GRAPH_TAG}#node_2"))
            .expect("node 2 card");
        assert!(
            !card.children.iter().any(|c| c.as_str() == EDIT_TF_TAG),
            "the card does not host the editor while the panel does",
        );
        let group = a11y
            .iter()
            .find(|n| n.tag == DETAIL_TAG)
            .expect("the Details group");
        assert!(group.children.contains(&row_tag), "the group lists the row");
    });
}

/// R920 (audit) — a selection change commits an in-flight PANEL edit (the
/// the engine commit-on-selection-change): otherwise the field paints nowhere
/// while `query editing` still advertises it. A card edit is selection-
/// independent and survives. Re-selecting the SAME node keeps the edit.
#[test]
fn r920_selection_change_commits_orphaned_panel_edit() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        coord.select_node(Some(NodeId(2)));
        assert!(coord.begin_edit_detail("x"));
        use_text_edit_state(EDIT_TF_TAG).set_text("333".to_owned());
        // Re-selecting the same node does NOT orphan the edit.
        coord.select_node(Some(NodeId(2)));
        assert_eq!(
            use_active_edit().get(),
            panel(EditTarget::PosX(NodeId(2))),
            "same selection keeps the edit"
        );
        // Changing selection commits the panel edit to node 2 and ends it.
        coord.select_node(Some(NodeId(0)));
        assert_eq!(
            use_active_edit().get(),
            None,
            "selection change ended the orphaned panel edit"
        );
        assert_eq!(
            coord.node_by_id(NodeId(2)).map(|n| n.x),
            Some(333),
            "the orphaned edit committed to node 2"
        );
        // A CARD edit survives a selection change (its card always paints).
        coord.select_node(Some(NodeId(2)));
        assert!(coord.begin_rename(NodeId(2)));
        coord.select_node(Some(NodeId(0)));
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::Title(NodeId(2))),
            "a card edit is selection-independent",
        );
    });
}

/// R920 (audit) — moving the SAME target between surfaces (card <-> panel) is
/// a migration that keeps the in-flight buffer; only a fresh / same-surface
/// open reseeds (the R878 restart UX, still covered by its own test).
#[test]
fn r920_cross_surface_migration_preserves_buffer() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        coord.select_node(Some(NodeId(2)));
        assert!(coord.begin_rename(NodeId(2))); // Card, Title(2), seeds "Multiply"
        use_text_edit_state(EDIT_TF_TAG).set_text("Foo".to_owned());
        assert!(coord.begin_edit_detail("title")); // Panel, Title(2) -> migration
        assert_eq!(
            use_active_edit().get(),
            panel(EditTarget::Title(NodeId(2))),
            "migrated to the panel surface"
        );
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "Foo",
            "the in-flight buffer survives a surface migration (no silent discard)",
        );
    });
}

/// R920 (audit) — deleting the edited node cancels the in-flight edit, so
/// `query editing` never advertises an edit on an absent node.
#[test]
fn r920_delete_cancels_edit_of_deleted_node() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        coord.select_node(Some(NodeId(2)));
        assert!(coord.begin_rename(NodeId(2)));
        assert!(coord.delete_node(NodeId(2)));
        assert_eq!(
            use_active_edit().get(),
            None,
            "deleting the edited node cancelled the edit"
        );
    });
}

#[test]
fn r838_remove_edge_by_id() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        assert_eq!(
            intro.invoke("remove_edge", IntrospectValue::Int(0)),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(intro.query("edge_count"), Ok(IntrospectValue::Int(2)));
        assert_eq!(
            intro.invoke("remove_edge", IntrospectValue::Int(9)),
            Ok(IntrospectValue::Bool(false)),
        );
    });
}

#[test]
fn r841_delete_node_keeps_stable_ids_over_rpc() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        let _ = intro.intervene("selected", IntrospectValue::Int(3)); // Output
        // Delete node id 1 (Color): drops only edge 1:0->2:1. NO reindex.
        assert_eq!(
            intro.invoke("delete_node", IntrospectValue::Int(1)),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(intro.query("node_count"), Ok(IntrospectValue::Int(3)));
        assert_eq!(intro.query("edge_count"), Ok(IntrospectValue::Int(2)));
        // Multiply is STILL id 2; edge id 0 still reads 0:0->2:0 (not renumbered).
        assert_eq!(
            intro.query("node.2.title"),
            Ok(IntrospectValue::Text("Multiply".to_owned()))
        );
        assert!(
            matches!(
                intro.query("node.1.title"),
                Err(ReadRefusal::NoSuchMember(_))
            ),
            "id 1 is gone, not reused"
        );
        assert_eq!(
            intro.query("edge.0"),
            Ok(IntrospectValue::Text("0:0->2:0".to_owned()))
        );
        // Selection (Output id 3) is untouched — it did not shift to 2.
        assert_eq!(intro.query("selected"), Ok(IntrospectValue::Int(3)));
    });
}

#[test]
fn r838_send_selects_node_on_release() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        let _ = intro.invoke(
            "send",
            IntrospectValue::Text("node_2:PointerDown".to_owned()),
        );
        let _ = intro.invoke("send", IntrospectValue::Text("node_2:PointerUp".to_owned()));
        assert_eq!(intro.query("selected"), Ok(IntrospectValue::Int(2)));
        // Background release deselects.
        let _ = intro.invoke("send", IntrospectValue::Text("PointerUp".to_owned()));
        assert_eq!(intro.query("selected"), Ok(IntrospectValue::Null));
    });
}

#[test]
fn r838_capture_drag_moves_grabbed_node() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Press node 0's body, then capture-move across the canvas.
        send(&mut scene, "node_0:PointerDown");
        let x0 = query_int(&scene, "node.0.x");
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            node.handle.pointer_move(0.10, 0.10); // anchor
            node.handle.pointer_move(0.30, 0.10); // +0.20 * WIN_W to the right
        }
        let x1 = query_int(&scene, "node.0.x");
        assert!(
            x1 > x0,
            "node moved right under the capture drag ({x0} -> {x1})"
        );
    });
}

#[test]
fn r838_port_drag_creates_edge() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Remove the existing wire into Multiply input 1, then re-make it
        // by dragging from Color's output port onto it.
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("remove_edge", IntrospectValue::Int(1));
        }
        assert_eq!(query_int(&scene, "edge_count"), 2);
        // Press Color's output port (node 1, port 0) → begin_drag arms.
        send(&mut scene, "oport_1_0:PointerDown");
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let payload = node
                .handle
                .begin_drag()
                .expect("output-port press arms a drag");
            assert_eq!(payload.kind.as_ref(), "node-edge");
            let drop = DropPoint {
                tag: format!("{GRAPH_TAG}#iport_2_1"),
                x_rel: 0.5,
                y_rel: 0.5,
            };
            node.handle.drag_release(&payload, Some(drop));
        }
        assert_eq!(query_int(&scene, "edge_count"), 3);
    });
}

#[test]
fn r1174_drag_reconnects_wired_input() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Free Multiply's input 1 so the reconnect target is unwired — a
        // clean target move, no single-wire displacement (that path is
        // r929's verb test). Edges left: 0 (Texture.0 -> Multiply.in0),
        // 2 (Multiply.0 -> Output.in0).
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("remove_edge", IntrospectValue::Int(1));
        }
        assert_eq!(query_int(&scene, "edge_count"), 2);
        assert_eq!(
            edge_str(&scene, 0).as_deref(),
            Some("0:0->2:0"),
            "edge 0 wires Texture.0 -> Multiply.in0",
        );
        // Grab Multiply's WIRED input 0 (edge 0) and pull it loose: the press
        // records the input's identity, and begin_drag arms a reconnect.
        send(&mut scene, "iport_2_0:PointerDown");
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let payload = node
                .handle
                .begin_drag()
                .expect("a wired-input press arms a reconnect drag");
            assert_eq!(payload.kind.as_ref(), "node-edge");
            // The loose end anchors at the grabbed edge's SOURCE output.
            assert!(
                matches!(&payload.value, IntrospectValue::Text(s) if s.as_str() == "0_0"),
                "the reconnect drag anchors at the grabbed edge's source (0_0), got {:?}",
                payload.value,
            );
            // Drop onto Multiply's now-free input 1.
            let drop = DropPoint {
                tag: format!("{GRAPH_TAG}#iport_2_1"),
                x_rel: 0.5,
                y_rel: 0.5,
            };
            node.handle.drag_release(&payload, Some(drop));
        }
        // A rewire keeps the edge count (remove old + add new).
        assert_eq!(query_int(&scene, "edge_count"), 2);
        // The grabbed edge's old id is retired; a fresh edge keeps the same
        // source and moves the target to in1 (the remove+add reconnect model).
        assert_eq!(
            edge_str(&scene, 0),
            None,
            "the grabbed edge's old id is retired"
        );
        assert_eq!(
            edge_str(&scene, 3).as_deref(),
            Some("0:0->2:1"),
            "the reconnected wire keeps the source, moves the target to in1",
        );
        // The gesture journals the same atomic Reconnect step the verb does:
        // one Ctrl+Z restores the original wiring (old id + old target).
        assert_eq!(use_undo().undo_label().as_deref(), Some("Reconnect"));
        assert!(use_undo().undo(), "undo the reconnect");
        assert_eq!(
            edge_str(&scene, 0).as_deref(),
            Some("0:0->2:0"),
            "undo restores the original edge in one step",
        );
    });
}

#[test]
fn r1174_unwired_input_press_arms_no_drag() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Free Multiply's input 1 (now unwired), then press it: there is no
        // edge to grab, so begin_drag arms nothing — the press is inert, not
        // a reconnect (and not an empty-canvas marquee either).
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("remove_edge", IntrospectValue::Int(1));
        }
        send(&mut scene, "iport_2_1:PointerDown");
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        assert!(
            node.handle.begin_drag().is_none(),
            "an unwired input has no edge to grab — no reconnect drag",
        );
    });
}

#[test]
fn r1174_reconnect_drop_off_input_is_a_noop() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Grab Multiply's wired input 0 (edge 0) and release in empty space
        // (no input port under the drop): the original wiring is untouched —
        // the connect gesture's "release nowhere cancels", shared by both.
        send(&mut scene, "iport_2_0:PointerDown");
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let payload = node.handle.begin_drag().expect("wired input arms a drag");
            node.handle.drag_release(&payload, None);
        }
        assert_eq!(
            query_int(&scene, "edge_count"),
            3,
            "no edge added or removed"
        );
        assert_eq!(
            edge_str(&scene, 0).as_deref(),
            Some("0:0->2:0"),
            "the grabbed edge is left exactly as it was",
        );
        assert_eq!(
            use_undo().undo_label(),
            None,
            "a cancelled reconnect journals nothing",
        );
    });
}

#[test]
fn r838_keyboard_nudges_and_deletes_selected() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let m = Modifiers::empty();
        // No selection → arrow keys are a no-op.
        assert!(!NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "ArrowRight",
            m
        ));
        // Select node 0, nudge right, verify it moved by NUDGE_STEP.
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let _ = node
                .handle
                .introspect_mut()
                .unwrap()
                .intervene("selected", IntrospectValue::Int(0));
        }
        let x0 = query_int(&scene, "node.0.x");
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "ArrowRight",
            m
        ));
        let x1 = query_int(&scene, "node.0.x");
        assert_eq!(x1 - x0, i64::from(NUDGE_STEP));
        // Delete removes the selected node.
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "Delete",
            m
        ));
        assert_eq!(
            graph_intro(&scene).query("node_count"),
            Ok(IntrospectValue::Int(3))
        );
        assert_eq!(
            graph_intro(&scene).query("selected"),
            Ok(IntrospectValue::Null)
        );
    });
}

#[test]
fn r838_escape_clears_selection() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let _ = node
            .handle
            .introspect_mut()
            .unwrap()
            .intervene("selected", IntrospectValue::Int(2));
        let m = Modifiers::empty();
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "Escape",
            m
        ));
        assert_eq!(
            graph_intro(&scene).query("selected"),
            Ok(IntrospectValue::Null)
        );
    });
}

/// A fresh coordinator over the shared Owner::cache holders (mutations
/// persist across instances within one Owner scope).
/// R852/R857 — an in-memory [`AppStorage`] cached under [`STORAGE_CACHE_KEY`],
/// so tests exercise `save` / `load` without touching the filesystem (the real
/// `FileStorage` path is covered by `tools/demos/r852_node_persist.py` via
/// `isolated_storage_dir`). Injects `InMemoryStorage` directly rather than
/// going through `use_app_storage` (which would hit the OS data dir).
fn mem_storage() -> Rc<AppStorage> {
    Owner::current()
        .expect("mem_storage requires an active Owner scope")
        .cache(STORAGE_CACHE_KEY, || {
            AppStorage::new(Box::new(pinion_core::storage::InMemoryStorage::new()))
        })
}

fn coordinator() -> NodeGraphExternal {
    NodeGraphExternal::new(
        use_document(),
        use_selection(),
        use_preview(),
        GraphServices {
            undo: use_undo(),
            storage: mem_storage(),
            zoom: use_zoom(),
            scroll: use_canvas_scroll(),
            editing: use_active_edit(),
            edit_buffer: use_text_edit_state(EDIT_TF_TAG),
            marquee_rect: use_marquee_rect(),
            node_drag: use_node_drag(),
            pin_create: use_pin_create(),
        },
    )
}

// ── R1220 pin-drop create menu (drag off a pin → typed menu → auto-wire) ──

/// The `query pin_create` JSON `candidates` array as owned strings.
fn menu_candidates(coord: &NodeGraphExternal) -> Vec<String> {
    match coord.query("pin_create") {
        Ok(IntrospectValue::Json(v)) => v["candidates"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        other => panic!("expected Json at pin_create, got {other:?}"),
    }
}

fn names(cands: &[usize]) -> Vec<&'static str> {
    cands.iter().map(|&k| PALETTE[k].0).collect()
}

#[test]
fn r1220_candidates_are_type_filtered_and_wire_target_exists() {
    // A Vector source feeds every input-bearing kind (all take a Vector); the
    // sourceless kinds (Texture/Color/Scalar) are excluded — nothing to wire.
    assert_eq!(
        names(&pin_create_candidates(PortType::Vector, "")),
        ["Multiply", "Add", "Output", "Lerp"],
        "input-bearing kinds only, in palette order"
    );
    // Every candidate resolves an auto-wire target (the open/commit gate).
    for &k in &pin_create_candidates(PortType::Vector, "") {
        assert!(
            first_compatible_input(k, PortType::Vector).is_some(),
            "candidate {k} has a compatible input"
        );
    }
    // A sourceless kind is never a candidate (a dangling wire is impossible).
    assert!(
        first_compatible_input(0, PortType::Vector).is_none(),
        "Texture: no input"
    );
    // The type-to-narrow filter: case-insensitive substring on the title.
    assert_eq!(
        names(&pin_create_candidates(PortType::Vector, "add")),
        ["Add"]
    );
    assert_eq!(
        names(&pin_create_candidates(PortType::Vector, "M")),
        ["Multiply"]
    );
    assert!(
        pin_create_candidates(PortType::Vector, "zzz").is_empty(),
        "no title matches -> empty"
    );
    // A Float source broadcasts into a Vector input, so it too reaches the
    // Vector-input kinds (the first compatible input is that Vector socket).
    assert_eq!(
        first_compatible_input(6, PortType::Float),
        Some(0),
        "Lerp: Float->Vector[0] broadcast"
    );
}

#[test]
fn r1220_graph_to_canvas_is_the_inverse_of_canvas_to_graph() {
    let scroll = ScrollState::new();
    scroll.set_max(1000, 1000);
    scroll.scroll_to(37, 84);
    // Round-trips every projection: canvas px -> graph -> canvas px.
    for &(cx, cy, zoom) in &[(120.0, 66.0, 1.0), (300.0, 210.0, 1.5), (10.0, 400.0, 0.75)] {
        let (gx, gy) = canvas_to_graph(&scroll, zoom, cx, cy);
        let (bx, by) = graph_to_canvas(&scroll, zoom, gx, gy);
        assert!(
            (bx - cx).abs() < 1e-6 && (by - cy).abs() < 1e-6,
            "round-trip at zoom {zoom}"
        );
    }
}

#[test]
fn r1220_open_commit_autowires_in_one_undo_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let nodes0 = coord.nodes().len();
        let edges0 = coord.edges().len();
        // Open from Texture(0) output 0 (Vector).
        assert!(coord.open_pin_create(NodeId(0), 0, (500, 300), (500, 300)));
        assert!(use_pin_create().get().is_some(), "menu open");
        assert_eq!(
            menu_candidates(&coord),
            ["Multiply", "Add", "Output", "Lerp"]
        );
        // Commit "Multiply" (kind 2): a node + an auto-wire edge, ONE step.
        let new_id = coord.commit_pin_create_kind(2).expect("commit a candidate");
        assert!(use_pin_create().get().is_none(), "menu closed on commit");
        assert_eq!(coord.nodes().len(), nodes0 + 1, "node added");
        assert_eq!(coord.edges().len(), edges0 + 1, "edge auto-wired");
        let e = coord
            .edges()
            .into_iter()
            .find(|e| e.to.node == new_id)
            .expect("a wire into the new node");
        assert_eq!(
            (e.from.node, e.from.port, e.to.port),
            (NodeId(0), 0, 0),
            "auto-wired source(0,0) -> new node's first compatible input"
        );
        assert_eq!(
            coord.selection.get().node(),
            Some(new_id),
            "new node selected"
        );
        assert_eq!(
            stack.undo_label().as_deref(),
            Some("Add Multiply + wire"),
            "one labelled undo step"
        );
        // One Ctrl+Z removes BOTH the node and its wire (atomic create+wire).
        assert!(stack.undo(), "undo the create");
        assert_eq!(coord.nodes().len(), nodes0, "undo removes the node");
        assert_eq!(
            coord.edges().len(),
            edges0,
            "... and its wire, in the same step"
        );
    });
}

#[test]
fn r1220_wire_dropped_on_empty_canvas_opens_menu_at_drop_point() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        // Press an output port, arm the drag, release on empty canvas.
        coord.handle_send("oport_0_0:PointerDown");
        let payload = coord.begin_drag().expect("drag armed from an output port");
        coord.drag_release(
            &payload,
            Some(DropPoint {
                tag: GRAPH_TAG.to_owned(),
                x_rel: 0.75,
                y_rel: 0.5,
            }),
        );
        let menu = use_pin_create()
            .get()
            .expect("empty-canvas drop opened the menu");
        assert_eq!((menu.from_node, menu.from_port), (NodeId(0), 0));
        // 0.75*640=480 px, 0.5*420=210 px; zoom 1, no pan -> graph (480, 210).
        assert_eq!(menu.at_graph, (480, 210), "node lands at the drop point");
    });
}

#[test]
fn r1220_port_drop_connects_and_reconnect_drop_never_opens_menu() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        // A fresh connect dropped ON an (unwired) input port connects (no
        // menu). Add an Output node so its lone Vector input starts unwired.
        let sink = coord.add_node(4).expect("Output node");
        coord.handle_send("oport_1_0:PointerDown");
        let payload = coord.begin_drag().expect("armed from Color output");
        let before = coord.edges().len();
        coord.drag_release(
            &payload,
            Some(DropPoint {
                tag: format!("node_graph#iport_{}_0", sink.0),
                x_rel: 0.5,
                y_rel: 0.5,
            }),
        );
        assert!(
            use_pin_create().get().is_none(),
            "a port drop opens no menu"
        );
        assert_eq!(coord.edges().len(), before + 1, "it connects instead");
        // A reconnect drag (a wired input pulled loose) dropped on empty canvas
        // cancels — it must NOT open a create menu (reconnect has no source pin
        // to spawn from). Input 2.0 is wired by default_edges (0 -> 2.0).
        coord.handle_send("iport_2_0:PointerDown");
        let payload = coord
            .begin_drag()
            .expect("reconnect drag armed from a wired input");
        coord.drag_release(
            &payload,
            Some(DropPoint {
                tag: GRAPH_TAG.to_owned(),
                x_rel: 0.9,
                y_rel: 0.2,
            }),
        );
        assert!(
            use_pin_create().get().is_none(),
            "a reconnect dropped in empty space cancels, no menu"
        );
    });
}

#[test]
fn r1411_palette_card_dragged_onto_canvas_instantiates_at_drop_point() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let stack = use_undo();
        let nodes0 = coord.nodes().len();
        // Press the Scalar palette card (kind 5): begin_drag arms a
        // drag-to-instantiate carrying the node TITLE as its value — the string
        // the shell's generic drag-image follower (R1113) surfaces as the chip
        // label with no per-binding wiring.
        coord.handle_send("palette_5:PointerDown");
        let payload = coord
            .begin_drag()
            .expect("a palette card press arms a drag");
        assert_eq!(
            payload.kind.as_ref(),
            PALETTE_DRAG_KIND,
            "the palette drag is its own kind, distinct from a node-edge drag",
        );
        assert!(
            matches!(&payload.value, IntrospectValue::Text(s) if s == "Scalar"),
            "the payload value is the node title (the follower chip label), got {:?}",
            payload.value,
        );
        // The id the drop is about to mint, so the new node is read by identity
        // (not by "the last one") — the position claim cannot alias a reorder.
        // R1596 — the tree mints, so the id about to be handed out is asked of
        // the document rather than read off a counter the editor maintained.
        let dropped = NodeId(coord.next_node_id());
        // Drop on the canvas at 0.75, 0.5: the R1220 pin-drop projection maps
        // that release fraction over GRAPH_TAG to graph (0.75*640, 0.5*420) =
        // (480, 210) at zoom 1 with no pan.
        coord.drag_release(
            &payload,
            Some(DropPoint {
                tag: GRAPH_TAG.to_owned(),
                x_rel: 0.75,
                y_rel: 0.5,
            }),
        );
        assert_eq!(
            coord.nodes().len(),
            nodes0 + 1,
            "the drop instantiates exactly one node",
        );
        let node = coord.node_by_id(dropped).expect("the dropped node exists");
        assert_eq!(
            node.title(),
            "Scalar",
            "the dropped kind is the pressed card"
        );
        assert_eq!(
            (node.x, node.y),
            (480, 210),
            "the node lands at the DROP point, not the fixed spawn point",
        );
        // One reversible step — the create+select delta, exactly like a click
        // add (the shared `add_node_at` funnel).
        assert_eq!(
            stack.undo_label().as_deref(),
            Some("Add Scalar"),
            "the drop is one labelled undo step",
        );
        assert!(stack.undo(), "undo removes the dropped node");
        assert_eq!(coord.nodes().len(), nodes0, "... in a single Ctrl+Z",);
    });
}

#[test]
fn r1411_palette_drag_released_off_canvas_instantiates_nothing() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let nodes0 = coord.nodes().len();
        // A palette drag that never reaches the canvas (released still over the
        // palette strip, or off-window: `over` is not GRAPH_TAG) adds nothing
        // through the drag path — the drop-point gate. In the live app a
        // press-release IN PLACE instead adds at the spawn point via the
        // trailing PointerUp click, which the router only fires when the gesture
        // did NOT become a drag (see the click test below).
        coord.handle_send("palette_5:PointerDown");
        let payload = coord.begin_drag().expect("palette press arms a drag");
        coord.drag_release(
            &payload,
            Some(DropPoint {
                tag: format!("{GRAPH_TAG}#palette_5"),
                x_rel: 0.5,
                y_rel: 0.5,
            }),
        );
        assert_eq!(
            coord.nodes().len(),
            nodes0,
            "a drag not dropped on the canvas creates no node",
        );
        // A release over nothing at all (off-window) is equally inert.
        coord.handle_send("palette_5:PointerDown");
        let payload = coord.begin_drag().expect("palette press arms a drag");
        coord.drag_release(&payload, None);
        assert_eq!(
            coord.nodes().len(),
            nodes0,
            "a drag released over no region creates no node",
        );
    });
}

#[test]
fn r1411_palette_click_in_place_still_adds_at_the_spawn_point() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let nodes0 = coord.nodes().len();
        // A press-release in place (the R849 click-to-add): PointerDown records
        // the Palette(kind) arm, PointerUp on the same card adds the node at the
        // spawn point. R1411 must leave this untouched — the drag path is
        // additive, and the spawn add stays keyed off the release TAG.
        let spawned = NodeId(coord.next_node_id());
        coord.handle_send("palette_5:PointerDown");
        coord.handle_send("palette_5:PointerUp");
        assert_eq!(
            coord.nodes().len(),
            nodes0 + 1,
            "a palette click still adds one node",
        );
        let node = coord.node_by_id(spawned).expect("the spawned node exists");
        assert_eq!(node.title(), "Scalar", "the clicked kind");
        // The spawn point is the fixed canvas SPAWN_X/SPAWN_Y projection — not a
        // drop point — so it is distinct from the (480, 210) the drag test lands.
        assert_ne!(
            (node.x, node.y),
            (480, 210),
            "a click lands at the spawn point, not a drop point",
        );
    });
}

#[test]
fn r1220_filter_narrows_and_highlight_roves_wrapping() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        assert!(coord.open_pin_create(NodeId(0), 0, (500, 300), (500, 300)));
        // Filter "add" narrows to a single candidate and clamps the highlight.
        assert!(coord.set_pin_filter("add"));
        assert_eq!(menu_candidates(&coord), ["Add"]);
        assert_eq!(
            coord
                .commit_pin_create_highlighted()
                .and_then(|id| coord.node_by_id(id))
                .map(|n| n.title()),
            Some("Add".to_owned()),
            "Enter commits the sole filtered candidate"
        );
        // Re-open and rove the highlight: wraps at the ends.
        assert!(coord.open_pin_create(NodeId(0), 0, (500, 300), (500, 300)));
        assert!(coord.move_pin_highlight(-1), "up from 0 wraps to the last");
        let last = pin_create_candidates(PortType::Vector, "").len() - 1;
        match coord.query("pin_create") {
            Ok(IntrospectValue::Json(v)) => {
                assert_eq!(
                    v["highlight"].as_u64(),
                    Some(last as u64),
                    "wrapped to last"
                );
            }
            other => panic!("expected Json, got {other:?}"),
        }
    });
}

#[test]
fn r1220_cancel_and_clickaway_close_the_menu() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        assert!(coord.open_pin_create(NodeId(0), 0, (500, 300), (500, 300)));
        assert!(coord.cancel_pin_create(), "cancel closes");
        assert!(use_pin_create().get().is_none());
        assert!(!coord.cancel_pin_create(), "cancel with no menu is false");
        // A background press (empty-canvas click) dismisses an open menu.
        assert!(coord.open_pin_create(NodeId(0), 0, (500, 300), (500, 300)));
        coord.handle_send(":PointerDown");
        assert!(
            use_pin_create().get().is_none(),
            "click-away on empty canvas dismisses"
        );
        // A press on a node also dismisses (click-away), before selecting it.
        assert!(coord.open_pin_create(NodeId(0), 0, (500, 300), (500, 300)));
        coord.handle_send("node_3:PointerDown");
        assert!(
            use_pin_create().get().is_none(),
            "click-away on a node dismisses"
        );
    });
}

#[test]
fn r1220_rpc_surface_open_filter_commit_cancel_and_schema() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        // The verbs are all schema-declared.
        let fields: Vec<&str> = coord.schema().fields.iter().map(|f| f.path).collect();
        for v in [
            "pin_create",
            "open_pin_create",
            "pin_create_filter",
            "pin_create_highlight",
            "commit_pin_create",
            "cancel_pin_create",
        ] {
            assert!(fields.contains(&v), "{v} must be schema-declared");
        }
        assert_eq!(
            coord.query("pin_create"),
            Ok(IntrospectValue::Null),
            "closed = Null"
        );
        // Open for Texture(0) output 0 via the RPC verb.
        assert_eq!(
            coord.invoke("open_pin_create", IntrospectValue::Text("0.0".to_owned())),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(
            menu_candidates(&coord),
            ["Multiply", "Add", "Output", "Lerp"]
        );
        // Filter, then commit a named candidate: returns the new node's id.
        assert_eq!(
            coord.invoke("pin_create_filter", IntrospectValue::Text("out".to_owned())),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(menu_candidates(&coord), ["Output"]);
        let edges0 = coord.edges().len();
        let Ok(IntrospectValue::Int(id)) = coord.invoke(
            "commit_pin_create",
            IntrospectValue::Text("Output".to_owned()),
        ) else {
            panic!("commit returns the new node id");
        };
        assert!(
            coord
                .node_by_id(NodeId(u32::try_from(id).unwrap()))
                .is_some(),
            "node created"
        );
        assert_eq!(coord.edges().len(), edges0 + 1, "auto-wired");
        assert_eq!(
            coord.query("pin_create"),
            Ok(IntrospectValue::Null),
            "closed after commit"
        );
        // cancel with no menu open is a benign false.
        assert_eq!(
            coord.invoke("cancel_pin_create", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(false)),
        );
    });
}

#[test]
fn r1220_noncandidate_commit_is_rejected_and_menu_stays_open() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        assert!(coord.open_pin_create(NodeId(0), 0, (500, 300), (500, 300)));
        let nodes0 = coord.nodes().len();
        // Kind 0 (Texture) has no input — it is not a candidate for any source,
        // so committing it is rejected and the menu stays open (the RPC gate).
        assert_eq!(
            coord.commit_pin_create_kind(0),
            None,
            "sourceless kind rejected"
        );
        assert!(
            use_pin_create().get().is_some(),
            "menu stays open on a rejected commit"
        );
        assert_eq!(coord.nodes().len(), nodes0, "graph unchanged");
    });
}

/// R1223 audit-clearance — a command chord (Ctrl+Z / Ctrl+A) while the menu
/// is open is SWALLOWED as a modal no-op and must NOT type into the
/// type-to-narrow filter (the pre-R1223 modal path passed only the bare key,
/// so Ctrl+Z appended `z`). A real character keypress still filters.
#[test]
fn r1223_menu_command_chord_does_not_leak_into_filter() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("open_pin_create", IntrospectValue::Text("0.0".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
        }
        let filter_now = |scene: &Scene| -> String {
            match scene
                .find_external_with_tag(GRAPH_TAG)
                .and_then(|n| n.handle.introspect())
                .and_then(|i| i.query("pin_create").ok())
            {
                Some(IntrospectValue::Json(v)) => v["filter"].as_str().unwrap_or("").to_owned(),
                other => panic!("expected open menu Json, got {other:?}"),
            }
        };
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert!(
            apply_key_graph(&mut scene, "z", ctrl),
            "Ctrl+Z swallowed by the modal menu"
        );
        assert_eq!(
            filter_now(&scene),
            "",
            "a command chord did NOT leak into the filter"
        );
        // A real character keypress (no modifier) types into the filter.
        assert!(apply_key_graph(&mut scene, "a", Modifiers::empty()));
        assert_eq!(
            filter_now(&scene),
            "a",
            "a plain character keypress filters"
        );
    });
}

/// R1223 audit-clearance — deleting the source node while the menu is open
/// (an RPC-reachable non-menu mutation) makes the menu read CLOSED (`Null`)
/// through the same validity gate paint + a11y apply, so `query pin_create`
/// is not a phantom-open introspection twin (§2 #2) and the keyboard is not
/// modal-trapped.
#[test]
fn r1223_menu_source_deleted_reads_closed_and_untraps_keyboard() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("open_pin_create", IntrospectValue::Text("0.0".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            assert!(
                matches!(intro.query("pin_create"), Ok(IntrospectValue::Json(_))),
                "menu opens",
            );
            // Delete the source node out from under the open menu.
            assert_eq!(
                intro.invoke("delete_node", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                intro.query("pin_create"),
                Ok(IntrospectValue::Null),
                "stale-source menu reads CLOSED (paint/a11y/introspect share the gate)",
            );
        }
        // The keyboard is NOT modal-trapped: Ctrl+A now reaches the graph
        // (the modal branch is bypassed because the query reads Null).
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert!(
            apply_key_graph(&mut scene, "a", ctrl),
            "Ctrl+A reaches select_all"
        );
        assert!(
            !selected_ids_of(&scene).is_empty(),
            "keyboard un-trapped: Ctrl+A selected the surviving nodes",
        );
    });
}

#[test]
fn r839_point_near_edge_is_curve_distance() {
    let from = (164, 114);
    let to = (256, 154);
    let thr = f64::from(EDGE_HIT_THRESHOLD);
    // The endpoints lie exactly on the curve.
    assert!(
        point_near_edge(f64::from(from.0), f64::from(from.1), from, to, thr),
        "start on curve",
    );
    assert!(
        point_near_edge(f64::from(to.0), f64::from(to.1), from, to, thr),
        "end on curve",
    );
    // A point far above the wire is not near it.
    assert!(
        !point_near_edge(210.0, 30.0, from, to, thr),
        "far point misses"
    );
}

#[test]
fn r839_hit_test_edge_finds_the_wire_under_a_click() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let nodes = default_nodes();
        // Midpoint of edge 0 (Texture.out0 -> Multiply.in0) sits in open space.
        let from = output_port_center(&nodes[0], 0);
        let to = input_port_center(&nodes[2], 0);
        let mid = cubic_at(
            (f64::from(from.0), f64::from(from.1)),
            {
                let (c1, _) = edge_curve(from, to);
                (f64::from(c1.0), f64::from(c1.1))
            },
            {
                let (_, c2) = edge_curve(from, to);
                (f64::from(c2.0), f64::from(c2.1))
            },
            (f64::from(to.0), f64::from(to.1)),
            0.5,
        );
        let coord = coordinator();
        assert_eq!(coord.hit_test_edge(mid.0, mid.1), Some(EdgeId(0)));
        assert_eq!(
            coord.hit_test_edge(10.0, 10.0),
            None,
            "empty corner hits nothing"
        );
    });
}

#[test]
fn r840_node_and_edge_selection_are_one_sum_type() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        coord.select_node(Some(NodeId(2)));
        assert_eq!(use_selection().get(), Selection::single(NodeId(2)));
        coord.select_edge(Some(EdgeId(1)));
        assert_eq!(
            use_selection().get(),
            Selection::Edge(EdgeId(1)),
            "selecting an edge replaces the node",
        );
        // The illegal "both selected" state is unrepresentable by construction.
    });
}

#[test]
fn r841_remove_edge_keeps_other_selections_stable() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // With stable ids, removing a *different* edge does NOT renumber the
        // selected one (the whole point — no index shift).
        coord.select_edge(Some(EdgeId(2)));
        assert!(coord.remove_edge(EdgeId(0)), "remove edge id 0");
        assert_eq!(
            use_selection().get(),
            Selection::Edge(EdgeId(2)),
            "id 2 still selected"
        );
        // Removing the selected edge itself prunes the selection.
        assert!(coord.remove_edge(EdgeId(2)), "remove the selected edge");
        assert_eq!(use_selection().get(), Selection::None);
        assert!(!coord.remove_edge(EdgeId(99)), "unknown edge id rejected");
    });
}

#[test]
fn r839_delete_selected_prefers_the_selected_edge() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        coord.select_edge(Some(EdgeId(0)));
        assert!(coord.delete_selected(), "delete the selected edge");
        assert_eq!(coord.edges().len(), 2, "one edge removed");
        assert_eq!(use_selection().get(), Selection::None);
    });
}

#[test]
fn r841_delete_node_keeps_survivor_ids_stable() {
    Owner::new().run(|| {
        let coord = coordinator();
        // Select Output (id 3); delete Color (id 1).
        coord.select_node(Some(NodeId(3)));
        assert!(coord.delete_node(NodeId(1)), "delete node id 1");
        // No reindex: Multiply is STILL id 2; its edges keep their ids.
        assert_eq!(
            coord.node_by_id(NodeId(2)).map(|n| n.title()),
            Some("Multiply".to_owned())
        );
        assert!(coord.node_by_id(NodeId(1)).is_none(), "Color is gone");
        let edges = coord.edges();
        assert_eq!(edges.len(), 2, "Color's incident edge dropped");
        assert!(
            edges
                .iter()
                .any(|e| e.id == EdgeId(0) && e.from.node == NodeId(0) && e.to.node == NodeId(2))
        );
        // The selection (Output id 3) is untouched — it did not shift.
        assert_eq!(use_selection().get(), Selection::single(NodeId(3)));
    });
}

#[test]
fn r842_dynamic_edge_id_seed_is_derived_from_defaults() {
    // The mint seed must be one past the highest default edge id, derived
    // (not a hand-maintained const) so adding a seed edge can never collide.
    let max_default = default_edges().iter().map(|e| e.id.0).max().unwrap();
    assert_eq!(first_dynamic_edge_id(), max_default + 1);
    // A freshly minted edge id is distinct from every default edge id.
    Owner::new().run(|| {
        let coord = coordinator();
        assert!(coord.add_edge(NodeId(0), 0, NodeId(3), 0), "add a new edge");
        let default_ids: Vec<u32> = default_edges().iter().map(|e| e.id.0).collect();
        let live_ids = live_edge_ids(&coord);
        let minted: Vec<u32> = live_ids
            .iter()
            .copied()
            .filter(|id| !default_ids.contains(id))
            .collect();
        assert_eq!(minted.len(), 1, "exactly one minted id");
        assert!(
            minted[0] > max_default,
            "minted id is above all default ids"
        );
    });
}

#[test]
fn r849_first_dynamic_node_id_is_derived_from_defaults() {
    // Mirrors the edge-id seed: one past the highest default node id, derived
    // so adding a seed node can never collide a minted id.
    let max_default = default_nodes().iter().map(|n| n.id.0).max().unwrap();
    assert_eq!(first_dynamic_node_id(), max_default + 1);
}

#[test]
fn r849_add_node_mints_a_fresh_stable_id_selects_it_and_guards_kind() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        assert_eq!(coord.node_count(), 4);
        // Add a Multiply (palette index 2): id = first dynamic, count 5.
        let id = coord.add_node(2).expect("Multiply is a valid kind");
        assert_eq!(id, NodeId(first_dynamic_node_id()));
        assert_eq!(coord.node_count(), 5);
        assert_eq!(
            use_selection().get(),
            Selection::single(id),
            "the new node is selected"
        );
        let n = coord.node_by_id(id).expect("new node present");
        assert_eq!(n.title(), "Multiply");
        assert_eq!(coord.arity(id), (2, 1));
        // An out-of-range kind adds nothing.
        assert_eq!(coord.add_node(99), None);
        assert_eq!(coord.node_count(), 5);
    });
}

#[test]
fn r849_added_node_ids_are_monotonic_never_reused_after_delete() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let a = coord.add_node(0).expect("Texture"); // first dynamic id
        assert!(coord.delete_node(a), "remove the just-added node");
        let b = coord.add_node(0).expect("Texture again");
        assert!(b.0 > a.0, "a deleted id is never reused (monotonic mint)");
    });
}

#[test]
fn r849_add_node_rpc_returns_the_new_id_and_rejects_unknown_kinds() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspect");
        // Create by kind NAME; returns the new stable id (AI-first one-shot).
        let id = match intro.invoke("add_node", IntrospectValue::Text("Add".to_owned())) {
            Ok(IntrospectValue::Int(i)) => i,
            other => panic!("expected the new id, got {other:?}"),
        };
        assert_eq!(id, i64::from(first_dynamic_node_id()));
        assert_eq!(intro.query("node_count"), Ok(IntrospectValue::Int(5)));
        assert_eq!(
            intro.query(&format!("node.{id}.title")),
            Ok(IntrospectValue::Text("Add".to_owned())),
        );
        assert_eq!(
            intro.query(&format!("node.{id}.inputs")),
            Ok(IntrospectValue::Int(2))
        );
        // An unknown kind is Rejected; the graph is unchanged.
        assert_refused_saying(
            &intro.invoke("add_node", IntrospectValue::Text("Bogus".to_owned())),
            "\"Bogus\" is not a node kind",
        );
        assert_eq!(intro.query("node_count"), Ok(IntrospectValue::Int(5)));
        // node_ids enumerates the new sparse id (read/write symmetry).
        match intro.query("node_ids") {
            Ok(IntrospectValue::Text(s)) => {
                assert!(
                    s.split(',').any(|t| t == id.to_string()),
                    "node_ids lists the added id: {s}"
                );
            }
            other => panic!("expected node_ids string, got {other:?}"),
        }
    });
}

#[test]
fn r849_palette_card_release_adds_a_node() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        assert_eq!(coord.node_count(), 4);
        // A palette card press+release creates the node (activation on release);
        // the press alone does not.
        coord.handle_send("palette_2:PointerDown");
        assert_eq!(coord.node_count(), 4, "the press alone adds nothing");
        coord.handle_send("palette_2:PointerUp");
        assert_eq!(
            coord.node_count(),
            5,
            "releasing the palette card created a node"
        );
        let id = use_selection().get().node().expect("new node selected");
        assert_eq!(coord.node_by_id(id).expect("present").title(), "Multiply");
    });
}

/// Test helper — the live edge id set via the RPC enumeration.
fn live_edge_ids(coord: &NodeGraphExternal) -> Vec<u32> {
    match coord.query("edge_ids") {
        Ok(IntrospectValue::Text(s)) if !s.is_empty() => {
            s.split(',').map(|x| x.parse().unwrap()).collect()
        }
        _ => Vec::new(),
    }
}

#[test]
fn r841_node_ids_and_edge_ids_enumerate_the_sparse_space() {
    Owner::new().run(|| {
        let coord = coordinator();
        assert_eq!(
            coord.query("node_ids"),
            Ok(IntrospectValue::Text("0,1,2,3".to_owned()))
        );
        assert_eq!(
            coord.query("edge_ids"),
            Ok(IntrospectValue::Text("0,1,2".to_owned()))
        );
        // Delete node id 1 → the id space stays sparse, no renumber.
        coord.delete_node(NodeId(1));
        assert_eq!(
            coord.query("node_ids"),
            Ok(IntrospectValue::Text("0,2,3".to_owned())),
            "sparse, no renumber",
        );
    });
}

#[test]
fn r839_background_press_probe_selects_a_wire() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // A bare background press, a capture-seed move onto edge 0's
        // midpoint, then a bare release selects that wire.
        send(&mut scene, "PointerDown");
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            node.handle.pointer_move(0.328_125, 0.319); // ~ (210, 134) = edge 0 midpoint
        }
        send(&mut scene, "PointerUp");
        assert_eq!(query_int(&scene, "selected_edge"), 0, "wire selected");
        assert_eq!(
            graph_intro(&scene).query("selected"),
            Ok(IntrospectValue::Null)
        );
        // A bare press on empty space deselects.
        send(&mut scene, "PointerDown");
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            node.handle.pointer_move(0.95, 0.95); // empty corner
        }
        send(&mut scene, "PointerUp");
        assert_eq!(
            graph_intro(&scene).query("selected_edge"),
            Ok(IntrospectValue::Null)
        );
    });
}

/// R880 — forward a background capture move at graph-space `(gx, gy)`
/// (zoom 1, pan 0: rel = graph / canvas).
#[allow(clippy::cast_possible_truncation)]
fn bg_move(scene: &mut Scene, gx: f64, gy: f64) {
    let node = scene
        .find_external_with_tag_mut(GRAPH_TAG)
        .expect("present");
    node.handle.pointer_move(
        (gx / f64::from(WIN_W)) as f32,
        (gy / f64::from(WIN_H)) as f32,
    );
}

fn selected_ids_of(scene: &Scene) -> String {
    match graph_intro(scene).query("selected_ids") {
        Ok(IntrospectValue::Text(t)) => t,
        other => panic!("expected Text at selected_ids, got {other:?}"),
    }
}

/// R1243 — the graph-space point at edge `from -> to`'s bezier midpoint
/// (t = 0.5): the on-wire point an edge-click / double-click probe must land on
/// (the same `edge_curve` + `cubic_at` SSOT the paint + `hit_test_edge` share).
fn edge_mid(from: (i32, i32), to: (i32, i32)) -> (f64, f64) {
    let (c1, c2) = edge_curve(from, to);
    cubic_at(
        (f64::from(from.0), f64::from(from.1)),
        (f64::from(c1.0), f64::from(c1.1)),
        (f64::from(c2.0), f64::from(c2.1)),
        (f64::from(to.0), f64::from(to.1)),
        0.5,
    )
}

#[test]
fn r880_marquee_replaces_selection_with_rect_hit_set() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Background press at an empty corner of the sweep, drag down
        // over nodes 0 (40, 70) and 1 (40, 210) — node 2 (x 250) and
        // node 3 (x 470) stay outside.
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, 20.0, 50.0); // capture seed (the press point)
        bg_move(&mut scene, 200.0, 260.0); // past the dead zone -> live
        assert!(
            use_marquee_rect().get().is_some(),
            "live marquee publishes the rubber-band rect",
        );
        send(&mut scene, "PointerUp");
        assert_eq!(selected_ids_of(&scene), "0,1", "rect-hit set replaces");
        assert_eq!(use_marquee_rect().get(), None, "band cleared on release");
        // An empty sweep (no nodes inside) clears the selection — the
        // background-click deselect generalised to an area.
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, 600.0, 320.0);
        bg_move(&mut scene, 700.0, 400.0);
        send(&mut scene, "PointerUp");
        assert_eq!(selected_ids_of(&scene), "", "empty sweep clears");
    });
}

#[test]
fn r880_ctrl_marquee_toggles_and_shift_marquee_unions() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        use_selection().set(Selection::single(NodeId(0)));
        // Ctrl-marquee over nodes 0 + 1: 0 toggles out, 1 toggles in.
        // The release rides the R880 empty-key bare modifier wire.
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, 20.0, 50.0);
        bg_move(&mut scene, 200.0, 260.0);
        send(&mut scene, ":PointerUp:c");
        assert_eq!(selected_ids_of(&scene), "1", "ctrl toggles membership");
        // Shift-marquee over node 2 only: unions in, 1 stays.
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, 240.0, 100.0);
        bg_move(&mut scene, 390.0, 180.0);
        send(&mut scene, ":PointerUp:s");
        assert_eq!(selected_ids_of(&scene), "1,2", "shift unions the hit set");
    });
}

#[test]
fn r880_jittery_background_click_stays_a_click() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Press on edge 0's midpoint, wobble 2px (inside the
        // DRAG_CLICK_THRESHOLD_PX dead zone), release: still the R839
        // edge-click, never a marquee.
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, 210.0, 134.0);
        bg_move(&mut scene, 212.0, 134.0);
        assert_eq!(use_marquee_rect().get(), None, "dead zone: no band");
        send(&mut scene, "PointerUp");
        assert_eq!(query_int(&scene, "selected_edge"), 0, "wire selected");
        // A *moved* background gesture must NOT consume the edge probe:
        // press near the wire, sweep away over empty space, release —
        // the marquee applies (empty -> clear), the edge stays
        // unselected.
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, 210.0, 134.0);
        bg_move(&mut scene, 600.0, 400.0);
        send(&mut scene, "PointerUp");
        assert_eq!(
            graph_intro(&scene).query("selected_edge"),
            Ok(IntrospectValue::Null),
            "moved gesture skips the edge-click probe",
        );
    });
}

#[test]
fn r880_select_all_via_invoke_and_ctrl_a() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let ctrl = Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        };
        assert!(apply_key_graph(&mut scene, "a", ctrl), "Ctrl+A consumed");
        assert_eq!(selected_ids_of(&scene), "0,1,2,3", "every node selected");
        // Escape clears (the existing single-selection escape).
        assert!(apply_key_graph(&mut scene, "Escape", Modifiers::empty()));
        assert_eq!(selected_ids_of(&scene), "");
        // The invoke twin answers false on an empty graph.
        use_document().set(Graph::new("material"));
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        assert_eq!(
            intro.invoke("select_all", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(false)),
            "empty graph: nothing to select",
        );
    });
}

#[test]
fn r880_1_pointer_cancel_reverts_drag_and_clears_marquee() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // A live marquee revoked by the system: band + anchor cleared,
        // nothing journaled, selection untouched.
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, 20.0, 50.0);
        bg_move(&mut scene, 200.0, 260.0);
        assert!(use_marquee_rect().get().is_some(), "band live");
        send(&mut scene, "PointerCancel");
        assert_eq!(use_marquee_rect().get(), None, "cancel clears the band");
        assert_eq!(selected_ids_of(&scene), "", "cancel applies nothing");
        // A live node drag revoked mid-move: members revert to their
        // press positions, no undo step (a cancelled gesture never
        // happened), and the latches drop.
        let coord = coordinator();
        let stack = use_undo();
        let before = pos_of(&coord, NodeId(0));
        coord.grabbed_node.set(Some(NodeId(0)));
        *coord.node_drag.borrow_mut() = Some(NodeDragStart {
            members: vec![(NodeId(0), 0.0, 0.0, before.0, before.1)],
            latch: live_latch(),
            cursor: Cell::new((0.0, 0.0)),
        });
        coord.set_node_pos(NodeId(0), before.0 + 40, before.1 + 20);
        let mut coord = coord;
        let _ = coord.handle_send("node_0:PointerCancel");
        assert_eq!(pos_of(&coord, NodeId(0)), before, "cancel reverts the move");
        assert_eq!(stack.len(), 0, "a cancelled drag never journals");
        assert_eq!(coord.grabbed_node.get(), None, "latches dropped");
    });
}

#[test]
fn r880_1_marquee_rect_clamps_to_the_world() {
    // A captured cursor straying off-canvas produces negative graph
    // coords; the published rect clamps so the painted band and the
    // applied area stay one value (upx floors negatives at paint).
    let rect = corner_rect((-40.0, 100.0), (60.0, 5000.0));
    assert_eq!(rect, (0, 100, 60, WORLD), "corners clamp to [0, WORLD]");
}

#[test]
fn r880_view_paints_the_live_marquee_band() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let marquee_tag = format!("{GRAPH_TAG}#marquee");
        let idle = view(IDLE_TF, &Frame::new());
        assert!(!idle.contains_tag(&marquee_tag), "no band while idle");
        use_marquee_rect().set(Some((100, 100, 220, 200)));
        let live = view(IDLE_TF, &Frame::new());
        assert!(live.contains_tag(&marquee_tag), "live band painted");
    });
}

#[test]
fn r838_view_carries_graph_and_node_and_edge_tags() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let scene = view(IDLE_TF, &Frame::new());
        assert!(scene.contains_tag(GRAPH_TAG), "graph root painted");
        assert!(
            scene.contains_tag(&format!("{GRAPH_TAG}#node_0")),
            "node 0 painted"
        );
        assert!(
            scene.contains_tag(&format!("{GRAPH_TAG}#oport_0_0")),
            "node 0 output port painted"
        );
        assert!(
            scene.contains_tag(&format!("{GRAPH_TAG}#iport_2_0")),
            "node 2 input port painted"
        );
        assert!(
            scene.contains_tag(&format!("{GRAPH_TAG}#edge_0")),
            "edge 0 painted"
        );
    });
}

#[test]
fn r840_access_node_emits_group_not_ordered_list() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        use_selection().set(Selection::single(NodeId(2)));
        let nodes = NodeEditorView::access_node(&IDLE_TF, Some(GRAPH_TAG));
        // R849 / R918 — root + palette toolbar + 5 palette buttons + graph
        // group + one generic per node (4) + the Details group + its 5 rows
        // (title / x / y + node 2's 2 input ports; no editor — idle).
        assert_eq!(
            nodes.len(),
            1 + 1 + PALETTE.len() + 1 + 4 + 1 + 5,
            "root + palette + graph + details"
        );
        // R918 — the Details panel lowers to a `group` listing the selected
        // node's property rows.
        let details = nodes
            .iter()
            .find(|n| n.tag == DETAIL_TAG)
            .expect("Details group present");
        assert_eq!(details.role, AriaRole::Group);
        assert_eq!(details.name.as_deref(), Some("Details"));
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == format!("{GRAPH_TAG}#detail_x")),
            "the Position X row lowers to a node",
        );
        // The root wraps the palette + the canvas; the focusable canvas is
        // the graph group (found by tag, not position).
        assert_eq!(nodes[0].role, AriaRole::Group, "editor root is a group");
        assert_eq!(nodes[0].tag, ROOT_TAG);
        let palette = nodes
            .iter()
            .find(|n| n.tag == PALETTE_TAG)
            .expect("palette toolbar present");
        assert_eq!(palette.role, AriaRole::Toolbar);
        let add_texture = nodes
            .iter()
            .find(|n| n.tag == format!("{GRAPH_TAG}#palette_0"))
            .expect("Texture palette button present");
        assert_eq!(add_texture.role, AriaRole::Button);
        assert_eq!(add_texture.name.as_deref(), Some("Add Texture"));
        // R850 — via the toolbar_button_nodes SSOT, palette buttons carry
        // roving-set metadata the hand-rolled version lacked.
        assert_eq!(add_texture.position_in_set, Some(1), "1-based posinset");
        assert_eq!(
            add_texture.size_of_set,
            Some(u32::try_from(PALETTE.len()).unwrap()),
            "setsize = palette length",
        );
        let graph = nodes
            .iter()
            .find(|n| n.tag == GRAPH_TAG)
            .expect("graph group present");
        // R840 audit fix: a graph is an unordered set, so Group/Generic —
        // never List/ListItem with a false aria-posinset.
        assert_eq!(graph.role, AriaRole::Group);
        assert!(graph.state.focused, "the canvas is the focused tab stop");
        let multiply = nodes
            .iter()
            .find(|n| n.tag == format!("{GRAPH_TAG}#node_2"))
            .expect("Multiply node present");
        assert_eq!(multiply.role, AriaRole::Generic);
        assert_eq!(multiply.name.as_deref(), Some("Multiply (2 in, 1 out)"));
        assert_eq!(multiply.selected, Some(true));
        assert_eq!(
            multiply.position_in_set, None,
            "no false ordered-set position"
        );
    });
}

#[test]
fn r838_view_contains_paint_tag() {
    pinion_core::test_fixtures::assert_widget_view_carries_tag::<NodeEditorView>(
        IDLE_TF,
        &Frame::default(),
    );
}

// ── R851 undo / redo (structural edits) ────────────────────────

/// Modifier state for a held-`Ctrl` (optionally `Shift`) keystroke.
fn mods(ctrl: bool, shift: bool) -> Modifiers {
    Modifiers {
        shift,
        ctrl,
        alt: false,
        meta: false,
    }
}

/// R1703 — one wheel event as the router hands it over.
///
/// The phase is `Update`, a notched mouse wheel: this canvas transforms
/// continuously and keeps no sub-notch remainder, so the bracket changes
/// nothing here and saying so is better than each call site inventing one.
fn wheel_at(x_rel: f32, y_rel: f32, dx: f32, dy: f32, modifiers: Modifiers) -> WheelReading {
    WheelReading::new(
        (x_rel, y_rel),
        (dx, dy),
        pinion_core::GesturePhase::Update,
        modifiers,
    )
}

/// A scene with the primary coordinator **and** the [`UndoStackExternal`]
/// extra, both sharing the one `use_undo()` stack — exactly what
/// `create_external` + `create_extra_externals` wire, so the keyboard /
/// RPC undo path (which finds [`UNDO_TAG`]) can be exercised in a unit test.
fn boot_full_scene() -> Scene {
    let primary = Scene::External(
        ExternalNode::new(Box::new(coordinator()) as Box<dyn External>).with_tag(GRAPH_TAG),
    );
    let undo = Scene::External(
        ExternalNode::new(Box::new(UndoStackExternal::new(use_undo()))).with_tag(UNDO_TAG),
    );
    Scene::Container(ContainerNode::new(vec![primary, undo]))
}

fn undo_ext_query(scene: &Scene, slot: &str) -> Option<IntrospectValue> {
    scene
        .find_external_with_tag(UNDO_TAG)
        .and_then(|n| n.handle.introspect())
        .expect("undo external present")
        .query(slot)
        .ok()
}

#[test]
fn r851_r878_create_extra_externals_wires_undo_surface_and_rename_field() {
    Owner::new().run(|| {
        let extras = NodeEditorView::create_extra_externals();
        assert_eq!(
            extras.len(),
            2,
            "the undo surface + the shared rename field"
        );
        assert_eq!(extras[0].tag, UNDO_TAG, "the undo-history surface");
        assert_eq!(extras[1].tag, EDIT_TF_TAG, "the R878 inline rename field");
    });
}

#[test]
fn r851_add_node_undo_removes_it_redo_restores_same_id() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        assert!(!stack.can_undo(), "boot: clean history");
        let id = coord.add_node(2).expect("Multiply"); // first dynamic id
        assert_eq!(coord.node_count(), 5);
        assert_eq!(use_selection().get(), Selection::single(id));
        assert!(stack.can_undo(), "the add is journaled");
        assert_eq!(stack.undo_label().as_deref(), Some("Add Multiply"));

        assert!(stack.undo(), "undo the add");
        assert_eq!(coord.node_count(), 4, "the node is gone");
        assert_eq!(use_selection().get(), Selection::None, "selection reverts");
        assert!(coord.node_by_id(id).is_none(), "the added id is gone");

        assert!(stack.redo(), "redo the add");
        assert_eq!(coord.node_count(), 5, "the node is back");
        assert_eq!(
            use_selection().get(),
            Selection::single(id),
            "and re-selected"
        );
        assert_eq!(
            coord.node_by_id(id).expect("present").id,
            id,
            "with the SAME stable id"
        );
    });
}

#[test]
fn r851_delete_node_undo_restores_node_and_all_incident_edges() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Node 2 (Multiply) is incident to all three seed edges (0, 1, 2).
        assert!(coord.delete_node(NodeId(2)), "delete the central node");
        assert_eq!(coord.node_count(), 3, "node removed");
        assert_eq!(coord.edges().len(), 0, "all three incident edges removed");

        assert!(stack.undo(), "undo the delete");
        assert_eq!(coord.node_count(), 4, "the node is restored");
        assert_eq!(coord.edges().len(), 3, "every incident edge is restored");
        assert_eq!(
            coord.query("edge.0"),
            Ok(IntrospectValue::Text("0:0->2:0".to_owned())),
            "a restored edge keeps its stable id + endpoints",
        );

        assert!(stack.redo(), "redo the delete");
        assert_eq!(coord.node_count(), 3);
        assert_eq!(coord.edges().len(), 0);
    });
}

#[test]
fn r851_connect_and_disconnect_round_trip_through_undo() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Disconnect seed edge 1 (1:0 -> 2:1), then undo restores it.
        assert!(coord.remove_edge(EdgeId(1)), "disconnect edge 1");
        assert_eq!(coord.edges().len(), 2);
        assert_eq!(stack.undo_label().as_deref(), Some("Disconnect"));
        assert!(stack.undo(), "undo the disconnect");
        assert_eq!(coord.edges().len(), 3, "the wire is back");
        assert_eq!(
            coord.query("edge.1"),
            Ok(IntrospectValue::Text("1:0->2:1".to_owned())),
            "edge 1 is restored verbatim",
        );
        // Now re-make a connection and undo it.
        assert!(stack.redo(), "redo the disconnect");
        assert_eq!(coord.edges().len(), 2);
        let before = coord.edges().len();
        assert!(
            coord.add_edge(NodeId(1), 0, NodeId(2), 1),
            "reconnect 1:0 -> 2:1"
        );
        assert_eq!(coord.edges().len(), before + 1);
        assert_eq!(stack.undo_label().as_deref(), Some("Connect"));
        assert!(stack.undo(), "undo the connect");
        assert_eq!(coord.edges().len(), before, "the new wire is gone");
    });
}

#[test]
fn r851_connect_displacing_a_wire_undo_restores_the_displaced_wire() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Seed edge 0 = 0:0 -> 2:0. Connecting 1:0 -> 2:0 (single-wire input
        // rule) displaces edge 0; one undo restores it.
        assert!(
            coord.add_edge(NodeId(1), 0, NodeId(2), 0),
            "connect into an occupied input"
        );
        assert_eq!(coord.edges().len(), 3, "one in, one out: count unchanged");
        assert!(
            matches!(coord.query("edge.0"), Err(ReadRefusal::NoSuchMember(_))),
            "edge 0 was displaced"
        );

        assert!(stack.undo(), "undo the displacing connect");
        assert_eq!(coord.edges().len(), 3);
        assert_eq!(
            coord.query("edge.0"),
            Ok(IntrospectValue::Text("0:0->2:0".to_owned())),
            "the displaced wire is restored",
        );
    });
}

#[test]
fn r929_reconnect_moves_target_keeps_source_one_undo_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let scalar = coord.add_node(5).expect("Scalar (Float source)");
        let lerp = coord
            .add_node(6)
            .expect("Lerp ([Vector, Vector, Float] in)");
        // Wire scalar.0 (Float) -> lerp.2 (Float factor).
        assert!(
            coord.add_edge(scalar, 0, lerp, 2),
            "seed the wire to reconnect"
        );
        let edge = coord
            .edges()
            .iter()
            .copied()
            .find(|e| e.from.node == scalar)
            .unwrap();
        let count = coord.edges().len();
        // Reconnect its target to lerp.0 (Vector; Float broadcasts -> valid).
        assert!(
            coord.reconnect_edge(edge.id, lerp, 0),
            "reconnect to a valid input"
        );
        assert_eq!(coord.edges().len(), count, "a rewire keeps the edge count");
        let now = coord
            .edges()
            .iter()
            .copied()
            .find(|e| e.from.node == scalar)
            .unwrap();
        assert_eq!(
            (now.from.node, now.from.port),
            (scalar, 0),
            "the source output is preserved"
        );
        assert_eq!(
            (now.to.node, now.to.port),
            (lerp, 0),
            "the target moved to the new input"
        );
        assert_ne!(
            now.id, edge.id,
            "a reconnect mints a fresh edge id (remove+add model)"
        );
        assert_eq!(
            stack.undo_label().as_deref(),
            Some("Reconnect"),
            "one Reconnect undo step"
        );
        // One Ctrl+Z restores the original wiring verbatim (old id + old target).
        assert!(stack.undo(), "undo the reconnect");
        let back = coord
            .edges()
            .iter()
            .copied()
            .find(|e| e.from.node == scalar)
            .unwrap();
        assert_eq!(
            (back.id, back.to.node, back.to.port),
            (edge.id, lerp, 2),
            "the original wire is restored in one step",
        );
        assert!(stack.redo(), "redo re-wires it");
        assert_eq!(
            coord
                .edges()
                .iter()
                .find(|e| e.from.node == scalar)
                .unwrap()
                .to
                .port,
            0,
        );
    });
}

#[test]
fn r929_reconnect_rejects_self_loop_and_type_mismatch_and_noops_on_same() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let lerp = coord.add_node(6).expect("Lerp (input 2 is Float)");
        // Boot edge 0 = 0:0 -> 2:0; node 0 (Texture) outputs a Vector.
        let edge0 = "0:0->2:0";
        assert_eq!(
            coord.query("edge.0"),
            Ok(IntrospectValue::Text(edge0.to_owned()))
        );
        // Vector source -> lerp.2 (Float): narrowing, rejected; edge unchanged.
        assert!(
            !coord.reconnect_edge(EdgeId(0), lerp, 2),
            "Vector -> Float reconnect rejected"
        );
        assert_eq!(
            coord.query("edge.0"),
            Ok(IntrospectValue::Text(edge0.to_owned()))
        );
        // Self-loop: reconnect onto an input of the edge's own source node.
        assert!(
            !coord.reconnect_edge(EdgeId(0), NodeId(0), 0),
            "self-loop reconnect rejected"
        );
        assert_eq!(
            coord.query("edge.0"),
            Ok(IntrospectValue::Text(edge0.to_owned()))
        );
        // Re-dropping on its own input is a no-op success (no graph change).
        assert!(
            coord.reconnect_edge(EdgeId(0), NodeId(2), 0),
            "same target is a no-op true"
        );
        assert_eq!(
            coord.query("edge.0"),
            Ok(IntrospectValue::Text(edge0.to_owned())),
            "still the same wire"
        );
        // An unknown edge id is a no-op false.
        assert!(
            !coord.reconnect_edge(EdgeId(999), NodeId(2), 1),
            "unknown edge id rejected"
        );
    });
}

#[test]
fn r929_reconnect_displacing_a_wire_undo_restores_both() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let s1 = coord.add_node(5).expect("Scalar 1");
        let s2 = coord.add_node(5).expect("Scalar 2");
        let lerp = coord.add_node(6).expect("Lerp");
        assert!(coord.add_edge(s1, 0, lerp, 0), "s1 -> lerp.0 (broadcast)");
        assert!(coord.add_edge(s2, 0, lerp, 2), "s2 -> lerp.2 (exact)");
        let e2 = coord
            .edges()
            .iter()
            .copied()
            .find(|e| e.from.node == s2)
            .unwrap();
        let count = coord.edges().len();
        // Reconnect e2 onto lerp.0 (occupied by s1's wire) -> displaces it.
        assert!(
            coord.reconnect_edge(e2.id, lerp, 0),
            "reconnect onto an occupied input"
        );
        assert_eq!(
            coord.edges().len(),
            count - 1,
            "old e2 + displaced s1 wire removed, one added"
        );
        assert!(
            !coord.edges().iter().any(|e| e.from.node == s1),
            "s1's wire was displaced"
        );
        // One undo restores BOTH the reconnected wire's old target and the displaced wire.
        assert!(stack.undo(), "one undo reverses the whole reconnect");
        assert_eq!(coord.edges().len(), count);
        let s1e = coord
            .edges()
            .iter()
            .copied()
            .find(|e| e.from.node == s1)
            .unwrap();
        assert_eq!(
            (s1e.to.node, s1e.to.port),
            (lerp, 0),
            "displaced wire restored"
        );
        let s2e = coord
            .edges()
            .iter()
            .copied()
            .find(|e| e.from.node == s2)
            .unwrap();
        assert_eq!(
            (s2e.to.node, s2e.to.port),
            (lerp, 2),
            "reconnected wire restored to its original input"
        );
    });
}

#[test]
fn r851_a_new_edit_after_undo_truncates_the_redo_branch() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let a = coord.add_node(0).expect("Texture");
        let b = coord.add_node(1).expect("Color");
        assert_eq!(coord.node_count(), 6);
        assert!(stack.undo(), "undo node b");
        assert!(stack.can_redo(), "b is redoable");
        assert!(coord.node_by_id(b).is_none());
        // A fresh add truncates the redo branch (single-branch undo stack).
        let c = coord.add_node(3).expect("Add");
        assert!(!stack.can_redo(), "the redo branch was dropped");
        assert_eq!(coord.node_count(), 6, "default 4 + a + c");
        assert!(coord.node_by_id(a).is_some() && coord.node_by_id(c).is_some());
        assert!(c.0 > b.0, "ids stay monotonic across the truncation");
    });
}

#[test]
fn r851_undo_external_query_and_invoke_round_trip() {
    Owner::new().run(|| {
        let mut scene = boot_full_scene();
        assert_eq!(
            undo_ext_query(&scene, "can_undo"),
            Some(IntrospectValue::Bool(false))
        );
        // Add a node through the primary coordinator.
        send(&mut scene, "palette_2:PointerDown");
        send(&mut scene, "palette_2:PointerUp");
        assert_eq!(query_int(&scene, "node_count"), 5);
        // The undo surface observes the history as data.
        assert_eq!(
            undo_ext_query(&scene, "can_undo"),
            Some(IntrospectValue::Bool(true))
        );
        assert_eq!(
            undo_ext_query(&scene, "index"),
            Some(IntrospectValue::Int(1))
        );
        assert_eq!(
            undo_ext_query(&scene, "count"),
            Some(IntrospectValue::Int(1))
        );
        assert_eq!(
            undo_ext_query(&scene, "undo_label"),
            Some(IntrospectValue::Text("Add Multiply".to_owned())),
        );
        // invoke undo on the external reverts the graph the coordinator reads.
        {
            let node = scene
                .find_external_with_tag_mut(UNDO_TAG)
                .expect("undo external");
            let intro = node.handle.introspect_mut().expect("introspect");
            assert_eq!(
                intro.invoke("undo", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true))
            );
        }
        assert_eq!(
            query_int(&scene, "node_count"),
            4,
            "RPC undo reverted the add"
        );
        assert_eq!(
            undo_ext_query(&scene, "can_undo"),
            Some(IntrospectValue::Bool(false))
        );
        assert_eq!(
            undo_ext_query(&scene, "can_redo"),
            Some(IntrospectValue::Bool(true))
        );
    });
}

#[test]
fn r851_ctrl_z_undoes_and_ctrl_shift_z_ctrl_y_redo() {
    Owner::new().run(|| {
        let mut scene = boot_full_scene();
        // Add a node, then Ctrl+Z removes it (the editor consumes the key).
        send(&mut scene, "palette_0:PointerDown");
        send(&mut scene, "palette_0:PointerUp");
        assert_eq!(query_int(&scene, "node_count"), 5);
        assert!(
            NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "z", mods(true, false)),
            "Ctrl+Z is handled",
        );
        assert_eq!(query_int(&scene, "node_count"), 4, "Ctrl+Z undid the add");
        // Ctrl+Y redoes.
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "y",
            mods(true, false)
        ));
        assert_eq!(query_int(&scene, "node_count"), 5, "Ctrl+Y redid the add");
        // Ctrl+Shift+Z undoes again (the redo-pairing alternative is undo's twin).
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "z",
            mods(true, false)
        ));
        assert_eq!(query_int(&scene, "node_count"), 4, "Ctrl+Z undid once more");
        assert!(
            NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "Z", mods(true, true)),
            "Ctrl+Shift+Z is handled",
        );
        assert_eq!(
            query_int(&scene, "node_count"),
            5,
            "Ctrl+Shift+Z redid the add"
        );
        // A plain 'z' (no Ctrl) is not an undo gesture — falls through.
        assert!(!NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "z",
            mods(false, false)
        ));
    });
}

#[test]
fn r851_undo_redo_at_boundaries_are_noops() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let stack = use_undo();
        assert!(!stack.undo(), "undo on empty history is a no-op");
        assert!(!stack.redo(), "redo on empty history is a no-op");
        assert_eq!(stack.len(), 0);
    });
}

// ── R852 serialization + persistence ───────────────────────────

#[test]
fn r852_serialized_query_is_json_with_schema_version_and_model() {
    Owner::new().run(|| {
        let coord = coordinator();
        let json = coord.serialized_json();
        let opening = Archive::<MaterialOp, EditorState>::read(&json);
        assert!(opening.opens(), "the editor writes what it can read");
        assert_eq!(opening.dropped(), []);
        let g = opening.take().expect("it opens");
        assert_eq!(
            g.companion(),
            Some(&EditorState {
                schema_version: PERSISTED_SCHEMA_VERSION
            })
        );
        assert_eq!(
            kind_nodes(g.document()).count(),
            4,
            "the seed nodes serialize"
        );
        assert_eq!(edges(g.document()).len(), 3, "the seed edges serialize");
        // R1596 — the id counters ride INSIDE the document (a tree mints its
        // own), so a snapshot cannot carry one that disagrees with the ids it
        // holds. The editor kept three beside the lists and gated a load on each
        // leading its own ids; that whole class of blob is now unrepresentable.
        let mut resumed = g.document().clone();
        assert_eq!(
            resumed
                .add_node(TREE, NodeBody::Kind(MaterialOp::Add), 0, 0)
                .expect("root tree"),
            NodeId(first_dynamic_node_id()),
            "the restored document mints where the seed left off"
        );
    });
}

#[test]
fn r852_serialized_round_trips_through_set_graph() {
    Owner::new().run(|| {
        let coord = coordinator();
        // Snapshot the seed graph, mutate, then restore via set_graph.
        let snap = coord.serialized_json();
        coord.add_node(2);
        coord.add_node(0);
        assert_eq!(coord.node_count(), 6, "two nodes added");
        assert!(coord.load_json(&snap), "set_graph applies the snapshot");
        assert_eq!(coord.node_count(), 4, "the graph reverted to the snapshot");
        assert_eq!(coord.edges().len(), 3);
        assert_eq!(
            use_selection().get(),
            Selection::None,
            "selection dropped on load"
        );
    });
}

#[test]
fn r852_save_then_load_restores_the_graph_via_storage() {
    Owner::new().run(|| {
        let coord = coordinator();
        let a = coord.add_node(2).expect("Multiply"); // id 4
        assert_eq!(coord.node_count(), 5);
        assert!(coord.save(), "save the 5-node graph");
        // Mutate after the save.
        let b = coord.add_node(0).expect("Texture"); // id 5
        assert_eq!(coord.node_count(), 6);
        assert!(coord.load(), "load restores the saved graph");
        assert_eq!(coord.node_count(), 5, "back to the saved 5 nodes");
        assert!(coord.node_by_id(a).is_some(), "the saved node survives");
        assert!(coord.node_by_id(b).is_none(), "the post-save node is gone");
    });
}

#[test]
fn r852_load_clears_the_undo_history() {
    Owner::new().run(|| {
        let coord = coordinator();
        let stack = use_undo();
        coord.add_node(2);
        assert!(stack.can_undo(), "the add is journaled");
        assert!(coord.save());
        coord.add_node(0);
        assert!(coord.load(), "load restores + clears undo");
        assert!(!stack.can_undo(), "the opened document is a fresh baseline");
        assert!(!stack.can_redo());
        assert_eq!(stack.len(), 0);
    });
}

#[test]
fn r852_load_with_nothing_stored_is_a_noop() {
    Owner::new().run(|| {
        let coord = coordinator();
        assert!(!coord.load(), "nothing stored yet -> false");
        assert_eq!(coord.node_count(), 4, "the graph is unchanged");
    });
}

/// ★★★ R1689 — the same three refusals, and now they are **three different
/// sentences**. That is the whole point of the archive: `false` told a caller
/// that something went wrong and nothing about which thing, so a person with a
/// file from last year and a person with a truncated download got the same
/// answer and neither could act on it.
#[test]
fn r852_set_graph_rejects_malformed_and_version_mismatch() {
    Owner::new().run(|| {
        let coord = coordinator();
        let mut whys = Vec::new();

        whys.push(
            coord
                .open_json("not json at all")
                .expect_err("malformed JSON rejected"),
        );
        assert_eq!(coord.node_count(), 4, "graph unchanged on malformed");

        whys.push(coord.open_json("").expect_err("an empty file is rejected"));

        // A file from an older build of THIS editor: the archive envelope is
        // current and the editor's own taxonomy version is not.
        let stale = Archive::<MaterialOp, EditorState>::of(Graph::new("material"))
            .with_companion(EditorState {
                schema_version: PERSISTED_SCHEMA_VERSION - 1,
            })
            .write()
            .expect("written");
        whys.push(
            coord
                .open_json(&stale)
                .expect_err("version mismatch rejected"),
        );
        assert_eq!(coord.node_count(), 4, "graph unchanged on version mismatch");

        // A file whose ARCHIVE revision is not this one.
        let other = stale.replace("\"revision\": 1", "\"revision\": 99");
        whys.push(
            coord
                .open_json(&other)
                .expect_err("a foreign archive revision is rejected"),
        );

        let distinct: std::collections::BTreeSet<&String> = whys.iter().collect();
        assert_eq!(
            distinct.len(),
            whys.len(),
            "four refusals, four sentences: {whys:?}"
        );
        assert!(
            whys[2].contains(&(PERSISTED_SCHEMA_VERSION - 1).to_string())
                && whys[2].contains(&PERSISTED_SCHEMA_VERSION.to_string()),
            "and the stale-file sentence names BOTH versions: {}",
            whys[2]
        );
        assert!(
            whys[3].contains("99"),
            "as does the foreign-revision one: {}",
            whys[3]
        );
        assert_eq!(coord.node_count(), 4, "and nothing loaded");
    });
}

#[test]
fn r852_loaded_counters_resume_monotonic_mint() {
    Owner::new().run(|| {
        let coord = coordinator();
        let a = coord.add_node(2).expect("Multiply"); // id 4, counter -> 5
        assert!(coord.save());
        let b = coord.add_node(0).expect("Texture"); // id 5, counter -> 6
        assert!(b.0 > a.0);
        assert!(coord.load(), "restore the counter to the saved value");
        // The next mint resumes at the saved counter: id b (the post-save
        // node) was discarded by the load, so reusing its number is correct
        // monotonic-from-the-saved-state, never an id live in the graph.
        let c = coord.add_node(0).expect("Texture");
        assert_eq!(c.0, a.0 + 1, "next id resumes at the saved next_node_id");
    });
}

#[test]
fn r852_save_load_set_graph_over_rpc_invoke() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // serialized query returns JSON.
        let json = match graph_intro(&scene).query("serialized") {
            Ok(IntrospectValue::Text(s)) => s,
            other => panic!("expected serialized JSON, got {other:?}"),
        };
        assert!(
            json.contains("schema_version"),
            "serialized is the snapshot JSON"
        );
        // Mutate, save, mutate again, load -> reverts.
        send(&mut scene, "palette_2:PointerDown");
        send(&mut scene, "palette_2:PointerUp");
        assert_eq!(query_int(&scene, "node_count"), 5);
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspect");
            assert_eq!(
                intro.invoke("save", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true))
            );
        }
        send(&mut scene, "palette_0:PointerDown");
        send(&mut scene, "palette_0:PointerUp");
        assert_eq!(query_int(&scene, "node_count"), 6);
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspect");
            assert_eq!(
                intro.invoke("load", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true))
            );
        }
        assert_eq!(
            query_int(&scene, "node_count"),
            5,
            "RPC load reverted to the saved graph"
        );
        // set_graph with the boot snapshot reverts all the way to 4 nodes.
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspect");
            assert_eq!(
                intro.invoke("set_graph", IntrospectValue::Text(json)),
                Ok(IntrospectValue::Bool(true)),
            );
        }
        assert_eq!(
            query_int(&scene, "node_count"),
            4,
            "set_graph restored the boot snapshot"
        );
        // A malformed set_graph is Rejected — and R1689 makes the refusal say
        // WHICH of the things that can be wrong was wrong, over the wire, so an
        // agent can act on it instead of retrying the same payload.
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspect");
            assert_refused_saying(
                &intro.invoke("set_graph", IntrospectValue::Text("garbage".to_owned())),
                "this is not a saved graph",
            );
        }
    });
}

// ── R853 node move undo (drag = one step, nudge burst coalesces) ─

/// Position of node `id` (panics if absent).
fn pos_of(coord: &NodeGraphExternal, id: NodeId) -> (i32, i32) {
    coord
        .node_by_id(id)
        .map(|n| (n.x, n.y))
        .expect("node present")
}

/// R880 — a [`DragLatch`] already past its dead zone (the synthetic
/// drags below model an in-flight *moved* gesture).
fn live_latch() -> DragLatch {
    let mut latch = DragLatch::new((0.0, 0.0));
    let _ = latch.advance((2.0 * pinion_core::DRAG_CLICK_THRESHOLD_PX, 0.0));
    latch
}

/// Arm + tear down a synthetic node-body drag from `before` to a `+delta`
/// position (the real `pointer_move` rel-math is exercised by the demo's
/// `tf.drag`; here we drive the latches directly to test the recording).
fn synth_drag(coord: &NodeGraphExternal, id: NodeId, before: (i32, i32), dx: i32, dy: i32) {
    coord.grabbed_node.set(Some(id));
    *coord.node_drag.borrow_mut() = Some(NodeDragStart {
        members: vec![(id, 0.0, 0.0, before.0, before.1)],
        latch: live_latch(),
        cursor: Cell::new((0.0, 0.0)),
    });
    coord.set_node_pos(id, before.0 + dx, before.1 + dy); // live preview write
    coord.end_gesture(); // commits one non-coalescable move
}

#[test]
fn r853_nudge_burst_coalesces_to_one_undo_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let id = NodeId(0);
        coord.select_node(Some(id));
        let start = pos_of(&coord, id);
        assert!(coord.nudge_selected(NUDGE_STEP, 0));
        assert!(coord.nudge_selected(NUDGE_STEP, 0));
        assert!(coord.nudge_selected(NUDGE_STEP, 0));
        assert_eq!(stack.len(), 1, "the nudge burst is one coalesced undo step");
        assert_eq!(
            pos_of(&coord, id),
            (start.0 + 3 * NUDGE_STEP, start.1),
            "moved 3 steps"
        );
        assert!(stack.undo(), "one undo reverts the whole burst");
        assert_eq!(pos_of(&coord, id), start, "back to the start");
        assert!(stack.redo(), "one redo re-applies the whole burst");
        assert_eq!(pos_of(&coord, id), (start.0 + 3 * NUDGE_STEP, start.1));
    });
}

#[test]
fn r853_drag_records_one_move_at_gesture_end() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let id = NodeId(0);
        let before = pos_of(&coord, id);
        coord.grabbed_node.set(Some(id));
        *coord.node_drag.borrow_mut() = Some(NodeDragStart {
            members: vec![(id, 0.0, 0.0, before.0, before.1)],
            latch: live_latch(),
            cursor: Cell::new((0.0, 0.0)),
        });
        coord.set_node_pos(id, before.0 + 50, before.1 + 30);
        assert_eq!(stack.len(), 0, "nothing is journaled mid-drag");
        coord.end_gesture();
        assert_eq!(stack.len(), 1, "the whole drag is one move at gesture end");
        assert!(stack.undo());
        assert_eq!(pos_of(&coord, id), before, "undo reverts the drag");
        assert!(stack.redo());
        assert_eq!(
            pos_of(&coord, id),
            (before.0 + 50, before.1 + 30),
            "redo re-applies it"
        );
    });
}

#[test]
fn r853_a_drag_does_not_coalesce_with_a_nudge() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let id = NodeId(0);
        coord.select_node(Some(id));
        assert!(coord.nudge_selected(NUDGE_STEP, 0), "a coalescable nudge");
        let pos = pos_of(&coord, id);
        synth_drag(&coord, id, pos, 40, 0); // a non-coalescable drag
        assert_eq!(
            stack.len(),
            2,
            "the drag is a fresh step, not folded into the nudge"
        );
    });
}

#[test]
fn r856_load_mid_drag_does_not_journal_a_spurious_move() {
    // R856 audit fix: a load / set_graph issued while a node drag is in
    // flight must clear the undo history AND not record the in-flight move
    // (apply_snapshot resets the gesture latches without journaling).
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let snap = coord.serialized_json();
        let id = NodeId(0);
        let before = pos_of(&coord, id);
        // Arm an in-flight drag (grab + a live move), no release.
        coord.grabbed_node.set(Some(id));
        *coord.node_drag.borrow_mut() = Some(NodeDragStart {
            members: vec![(id, 0.0, 0.0, before.0, before.1)],
            latch: live_latch(),
            cursor: Cell::new((0.0, 0.0)),
        });
        coord.set_node_pos(id, before.0 + 40, before.1);
        assert!(coord.load_json(&snap), "load the snapshot mid-drag");
        assert!(
            !stack.can_undo(),
            "the opened document has a clean undo history"
        );
        assert_eq!(
            stack.len(),
            0,
            "no spurious MoveNodesCmd was journaled across the load"
        );
    });
}

#[test]
fn r856_non_contiguous_moves_do_not_coalesce() {
    // R856 audit fix: the merge contiguity guard. Two coalescable moves of
    // the same node whose positions do not chain (m1.after != m2.before)
    // stay two undo steps.
    Owner::new().run(|| {
        let _ = boot_scene();
        let stack = use_undo();
        // R1596 — one `GraphEdit` replaced the four commands, and contiguity is
        // now DOCUMENT equality (`self.after == next.before`) rather than a
        // per-node position pair. Two edits whose states do not meet are two
        // steps, which is the same rule stated over the whole value.
        let handle = use_graph_handle();
        let id = NodeId(0);
        let move_to = |x| {
            handle.edit("Move node", Some(BTreeSet::from([id])), move |graph, _| {
                if let Some(node) = graph.tree_mut(TREE).and_then(|t| t.node_mut(id)) {
                    node.x = x;
                }
            })
        };
        assert!(move_to(100), "the first move journals");
        // Rewrite the document underneath so the next edit's `before` is not the
        // previous edit's `after` — the non-contiguous case.
        handle.document.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(node) = next.tree_mut(TREE).and_then(|t| t.node_mut(id)) {
                node.x = 200;
            }
            next
        });
        assert!(move_to(300), "the second move journals");
        assert_eq!(stack.len(), 2, "non-contiguous moves do not coalesce");
    });
}

#[test]
fn r853_intervene_x_then_y_coalesce_to_one_move() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let stack = use_undo();
        let before_x = query_int(&scene, "node.0.x");
        let before_y = query_int(&scene, "node.0.y");
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspect");
            intro
                .intervene("node.0.x", IntrospectValue::Int(200))
                .expect("x ok");
            intro
                .intervene("node.0.y", IntrospectValue::Int(150))
                .expect("y ok");
        }
        assert_eq!(
            stack.len(),
            1,
            "x then y on the same node coalesce to one move"
        );
        assert_eq!(query_int(&scene, "node.0.x"), 200);
        assert!(stack.undo(), "one undo reverts both axes");
        assert_eq!(query_int(&scene, "node.0.x"), before_x, "x restored");
        assert_eq!(query_int(&scene, "node.0.y"), before_y, "y restored");
    });
}

#[test]
fn r853_move_after_add_is_a_separate_redoable_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let id = coord.add_node(0).expect("Texture"); // structural step
        let add_pos = pos_of(&coord, id);
        assert!(
            coord.nudge_selected(NUDGE_STEP, NUDGE_STEP),
            "move the new node"
        );
        let moved_pos = pos_of(&coord, id);
        assert_ne!(add_pos, moved_pos);
        assert_eq!(
            stack.len(),
            2,
            "add + move are two steps (move not folded into add)"
        );
        assert!(stack.undo(), "undo the move");
        assert_eq!(
            pos_of(&coord, id),
            add_pos,
            "node back at its add-time position"
        );
        assert!(stack.undo(), "undo the add");
        assert!(coord.node_by_id(id).is_none(), "the node is gone");
        assert!(stack.redo(), "redo the add");
        assert_eq!(
            pos_of(&coord, id),
            add_pos,
            "re-added at the add-time position"
        );
        assert!(stack.redo(), "redo the move");
        assert_eq!(
            pos_of(&coord, id),
            moved_pos,
            "redo restores the moved position"
        );
    });
}

#[test]
fn r852_ctrl_s_saves_and_ctrl_o_loads() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        send(&mut scene, "palette_2:PointerDown");
        send(&mut scene, "palette_2:PointerUp");
        assert_eq!(query_int(&scene, "node_count"), 5);
        // Ctrl+S saves the 5-node graph.
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "s",
            mods(true, false)
        ));
        // Add another, then Ctrl+O reverts to the saved graph.
        send(&mut scene, "palette_0:PointerDown");
        send(&mut scene, "palette_0:PointerUp");
        assert_eq!(query_int(&scene, "node_count"), 6);
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "o",
            mods(true, false)
        ));
        assert_eq!(
            query_int(&scene, "node_count"),
            5,
            "Ctrl+O loaded the saved graph"
        );
        // Plain 's' (no Ctrl) is not a save gesture.
        assert!(!NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "s",
            mods(false, false)
        ));
    });
}

// ── R877 viewport (pan = ScrollAxis::Both scroll, zoom = Signal) ─

#[test]
fn r877_ctrl_wheel_zooms_anchored_at_the_cursor() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        // Anchor at canvas (160, 105) = rel (0.25, 0.25); the graph point
        // under that cursor before the zoom must still be under it after.
        let (ax, ay) = (0.25_f32, 0.25_f32);
        let before = coord.cursor_graph(f64::from(ax), f64::from(ay));
        // One notch in: dy = -16 px -> factor ZOOM_STEP.
        assert!(
            coord.wheel(&wheel_at(ax, ay, 0.0, -16.0, mods(true, false))),
            "ctrl-wheel consumed"
        );
        let zoom = coord.zoom.get();
        assert!(
            (zoom - ZOOM_STEP).abs() < 1e-9,
            "one notch = one ZOOM_STEP, got {zoom}"
        );
        let after = coord.cursor_graph(f64::from(ax), f64::from(ay));
        // The scroll offset quantises to whole px, so the anchor holds to
        // sub-pixel-per-zoom tolerance (< 1 graph unit).
        assert!(
            (after.0 - before.0).abs() < 1.0 && (after.1 - before.1).abs() < 1.0,
            "graph point under the cursor is pinned: {before:?} -> {after:?}",
        );
    });
}

#[test]
fn r877_plain_wheel_is_declined_so_the_scroll_substrate_pans() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        // No modifiers: the External declines and the router's native
        // Scroll fallback owns the pan (zero canvas code).
        assert!(!coord.wheel(&wheel_at(0.5, 0.5, 0.0, 32.0, mods(false, false))));
        assert!(
            (coord.zoom.get() - 1.0).abs() < f64::EPSILON,
            "zoom untouched"
        );
    });
}

#[test]
fn r877_shift_wheel_pans_horizontally() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        // Give the pan some range first (the layout pass does this in the
        // running app; unit tests write the world maxima directly).
        coord.scroll.set_max(WORLD, WORLD);
        assert!(
            coord.wheel(&wheel_at(0.5, 0.5, 0.0, 48.0, mods(false, true))),
            "shift-wheel consumed"
        );
        assert_eq!(
            coord.scroll.offset(),
            (48, 0),
            "vertical notches drive the x offset"
        );
    });
}

#[test]
fn r877_wheel_outside_the_canvas_rect_is_declined() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        // A palette-card wheel routes here via the shared primary but
        // normalises outside [0, 1] (the palette is left of the canvas).
        assert!(!coord.wheel(&wheel_at(-0.2, 0.5, 0.0, -16.0, mods(true, false))));
        // ★ R1703 — and the surface SAYS so at that point, which is what stops
        // the router offering the event at all. Before this the two facts were
        // independent: the guard above could be deleted and nothing would have
        // reported that the published answer no longer matched.
        assert!(
            pinion_core::external::External::wheel_intent(&coord, (-0.2, 0.5)).is_none(),
            "a point outside the canvas declares no wheel"
        );
        assert_eq!(
            pinion_core::external::External::wheel_intent(&coord, (0.5, 0.5)),
            Some(pinion_core::widgets::wheel::WheelIntent::Zoom),
            "and a point inside it declares the zoom"
        );
        assert!(
            (coord.zoom.get() - 1.0).abs() < f64::EPSILON,
            "zoom untouched"
        );
    });
}

#[test]
fn r877_viewport_zoom_intervene_clamps_and_round_trips() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        assert!(
            intro
                .intervene("viewport.zoom", IntrospectValue::Float(2.0))
                .is_ok()
        );
        assert_eq!(
            intro.query("viewport.zoom"),
            Ok(IntrospectValue::Float(2.0))
        );
        // Out-of-range writes clamp (the setter-returns-outcome read-back).
        assert!(
            intro
                .intervene("viewport.zoom", IntrospectValue::Float(99.0))
                .is_ok()
        );
        assert_eq!(
            intro.query("viewport.zoom"),
            Ok(IntrospectValue::Float(ZOOM_MAX))
        );
        assert!(
            intro
                .intervene("viewport.zoom", IntrospectValue::Float(0.01))
                .is_ok()
        );
        assert_eq!(
            intro.query("viewport.zoom"),
            Ok(IntrospectValue::Float(ZOOM_MIN))
        );
        // Type mismatch is rejected.
        assert_eq!(
            intro.intervene("viewport.zoom", IntrospectValue::Text("big".into())),
            Err(InterveneError::TypeMismatch),
        );
    });
}

#[test]
fn r877_viewport_pan_intervene_is_graph_units_and_clamps() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // Zoom to 2x first: set_zoom writes the world maxima, exactly as
        // the layout pass does on every painted frame.
        assert!(
            intro
                .intervene("viewport.zoom", IntrospectValue::Float(2.0))
                .is_ok()
        );
        // Pan to graph (100, 50): the query twin reads back in graph units
        // (zoom-independent), the wire shape an AI client can reason in.
        assert!(
            intro
                .intervene("viewport.x", IntrospectValue::Float(100.0))
                .is_ok()
        );
        assert!(
            intro
                .intervene("viewport.y", IntrospectValue::Float(50.0))
                .is_ok()
        );
        assert_eq!(intro.query("viewport.x"), Ok(IntrospectValue::Float(100.0)));
        assert_eq!(intro.query("viewport.y"), Ok(IntrospectValue::Float(50.0)));
        // A huge pan clamps against the world maxima.
        assert!(
            intro
                .intervene("viewport.x", IntrospectValue::Float(1.0e9))
                .is_ok()
        );
        let clamped = match intro.query("viewport.x") {
            Ok(IntrospectValue::Float(v)) => v,
            other => panic!("expected Float, got {other:?}"),
        };
        assert!(
            clamped < 2.0 * f64::from(WORLD),
            "pan clamped to the world extent, got {clamped}",
        );
        // An Int payload is a TypeMismatch — the slot is declared `float`
        // and `as_f64` deliberately does not coerce (R51.155).
        assert_eq!(
            intro.intervene("viewport.x", IntrospectValue::Int(0)),
            Err(InterveneError::TypeMismatch),
        );
    });
}

#[test]
fn r1191_graph_anchor_offset_is_the_forward_inverse() {
    // R1191 — the forward twin computes the pan offset that places graph
    // (gx,gy) under canvas (cx,cy); it is the exact algebraic inverse of
    // canvas_to_graph's affine (canvas = graph·zoom − offset).
    let approx = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let (zoom, gx, gy, cx, cy) = (1.5, 400.0, 300.0, 100.0, 50.0);
    let (ox, oy) = graph_anchor_offset(gx, gy, zoom, cx, cy);
    assert!(approx(ox, gx * zoom - cx), "x offset = gx*zoom - cx");
    assert!(approx(oy, gy * zoom - cy), "y offset = gy*zoom - cy");
    // forward then the manual inverse affine = identity at the anchor px.
    assert!(approx((ox + cx) / zoom, gx), "x round-trips to gx");
    assert!(approx((oy + cy) / zoom, gy), "y round-trips to gy");
    // another declarative toolkit with the REAL inverse SSOT via a ScrollState
    // (integer-clean offsets, so no scroll rounding) — the two projection
    // SSOTs are provably inverse, not just the hand-written affine.
    let scroll = ScrollState::new();
    scroll.set_max(10_000, 10_000);
    scroll.scroll_to(round_i32(ox), round_i32(oy));
    let back = canvas_to_graph(&scroll, zoom, cx, cy);
    assert!(
        approx(back.0, gx) && approx(back.1, gy),
        "SSOTs compose to identity"
    );
    // The RPC-pan degenerate case (anchor at canvas 0) = pure graph->world,
    // matching the `viewport.x` pan write's v*zoom.
    assert!(
        approx(graph_anchor_offset(gx, 0.0, zoom, 0.0, 0.0).0, gx * zoom),
        "canvas-0 anchor = graph*zoom (the viewport.x pan write)"
    );
}

#[test]
fn r1183_autopan_axis_can_move_headroom() {
    // Negative push moves only off a non-zero offset (else pinned at 0).
    assert!(
        autopan_axis_can_move(-1.0, 5, 100),
        "-push with offset>0 moves"
    );
    assert!(
        !autopan_axis_can_move(-1.0, 0, 100),
        "-push pinned at 0 is at rest"
    );
    // Positive push moves only below max (else pinned at the far edge).
    assert!(autopan_axis_can_move(1.0, 5, 100), "+push below max moves");
    assert!(
        !autopan_axis_can_move(1.0, 100, 100),
        "+push pinned at max is at rest"
    );
    // No push never moves.
    assert!(!autopan_axis_can_move(0.0, 50, 100), "no push = no move");
}

#[test]
fn r1183_autopan_rests_at_clamp_and_in_dead_zone() {
    use pinion_core::animation::Tickable;
    // Build an AutoPan whose drag holds the RIGHT rim (+x push, no y push),
    // with a chosen latch + scroll offset (max = 100 on both axes).
    let make = |latch: DragLatch, offset: (i32, i32)| {
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(100, 100);
        scroll.scroll_to(offset.0, offset.1);
        AutoPan {
            document: Rc::new(Signal::new(default_graph())),
            scroll,
            zoom: Rc::new(Signal::new(1.0)),
            node_drag: Rc::new(RefCell::new(Some(NodeDragStart {
                members: vec![],
                latch,
                cursor: Cell::new((0.99, 0.5)),
            }))),
        }
    };
    // Live drag, x has headroom -> auto-pans (not at rest).
    let ap = make(live_latch(), (40, 40));
    assert!(
        ap.active().is_some(),
        "a live rim drag with headroom auto-pans"
    );
    assert!(!ap.is_at_rest(0.0), "... so it keeps requesting frames");
    // Live drag, x pinned at the world edge -> the +x rim makes no progress
    // (SMELL-1 fix): the driver rests instead of spinning the frame loop.
    let ap = make(live_latch(), (100, 40));
    assert!(
        ap.active().is_none(),
        "a +x rim pinned at the world edge is at rest"
    );
    assert!(
        ap.is_at_rest(0.0),
        "... so the backend can idle the surface"
    );
    // Not-yet-latched (dead-zone) press at the rim -> must NOT auto-pan.
    let ap = make(DragLatch::new((0.0, 0.0)), (40, 40));
    assert!(
        ap.active().is_none(),
        "a dead-zone (unlatched) rim press never auto-pans"
    );
}

#[test]
fn r1182_autopan_push_ramps_and_clamps_per_rim() {
    let approx = |a: f64, b: f64| (a - b).abs() < 1e-9;
    // Dead centre: no push anywhere outside the two margins.
    assert!(approx(autopan_push(0.5), 0.0), "centre = no push");
    assert!(
        approx(autopan_push(AUTOPAN_MARGIN), 0.0),
        "the low margin line is the zero point"
    );
    assert!(
        approx(autopan_push(1.0 - AUTOPAN_MARGIN), 0.0),
        "the high margin line is the zero point"
    );
    // Low rim: ramps 0 -> -1 as the cursor nears (and passes) the edge.
    assert!(
        autopan_push(AUTOPAN_MARGIN / 2.0) < 0.0,
        "inside the low margin pushes -"
    );
    assert!(
        approx(autopan_push(0.0), -1.0),
        "the low edge is full negative push"
    );
    assert!(
        approx(autopan_push(-0.5), -1.0),
        "past the low edge saturates at -1 (no reversal)"
    );
    // High rim: ramps 0 -> +1 symmetrically.
    assert!(
        autopan_push(1.0 - AUTOPAN_MARGIN / 2.0) > 0.0,
        "inside the high margin pushes +"
    );
    assert!(
        approx(autopan_push(1.0), 1.0),
        "the high edge is full positive push"
    );
    assert!(
        approx(autopan_push(1.5), 1.0),
        "past the high edge saturates at +1"
    );
    // The ramp is monotone within a margin (deeper = stronger).
    assert!(
        autopan_push(0.02) < autopan_push(0.08),
        "a deeper low-rim cursor pushes harder (more negative)"
    );
}

#[test]
fn r877_frame_all_fits_the_node_bbox() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Pan + zoom somewhere unhelpful first.
        coord.set_zoom_centered(4.0);
        assert!(coord.frame_all(), "frame_all on a non-empty graph");
        let zoom = coord.zoom.get();
        // Boot bbox: x 40..600, y 70..~318 -> fit is width-bound:
        // (640 - 48) / 560 ~= 1.057.
        assert!(
            zoom > 1.0 && zoom < 1.2,
            "fit zoom in the expected band, got {zoom}"
        );
        // Every node's projected position lies inside the canvas.
        let (ox, oy) = coord.scroll.offset();
        for n in &coord.nodes() {
            let sx = wpx(n.x, zoom) - ox;
            let sy = wpx(n.y, zoom) - oy;
            assert!(
                sx >= 0 && sy >= 0,
                "node {} on-canvas, got ({sx}, {sy})",
                n.id
            );
            assert!(
                sx + wpx(NODE_W, zoom) <= i32::try_from(WIN_W).unwrap_or(0)
                    && sy + wpx(n.height(), zoom) <= i32::try_from(WIN_H).unwrap_or(0),
                "node {} fully visible",
                n.id,
            );
        }
    });
}

/// ★★★★ R1688 — **`frame_all` keeps a SCREEN margin**, and this is the check
/// that says which of the two kinds it is.
///
/// Written because a counterfactual passed. R1688 moved this fit onto
/// `pinion_node_graph::Fit`, whose [`Margin`] declares whether the clear space
/// is canvas units or screen pixels — the distinction exists because this
/// editor wants one and the node-lab's behaviour canon wants the other. Swapping
/// the arm here left every test in this file green and the R877 demo green too:
/// the fit above only asserts a zoom BAND, and the two kinds differ by half a
/// percent on the boot graph.
///
/// So the margin is pinned as what it means: on whichever axis binds, the gap
/// between the node bounding box and the canvas edge is `FRAME_MARGIN`
/// **pixels**, at whatever zoom came out. A canvas margin would leave
/// `FRAME_MARGIN * zoom` instead — 2.7 px more here, which is why one pixel of
/// tolerance is enough to tell them apart and why the tolerance is not larger.
#[test]
fn r1688_frame_all_keeps_a_margin_in_screen_pixels() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        coord.set_zoom_centered(4.0);
        assert!(coord.frame_all());
        let zoom = coord.zoom.get();
        let (ox, oy) = coord.scroll.offset();
        let graph = coord.graph();
        let (min_x, min_y, max_x, max_y) =
            node_bounds(kind_nodes(&graph).map(|(n, _)| n)).expect("a non-empty graph");
        // The four gutters, in screen pixels.
        let left = wpx(min_x, zoom) - ox;
        let top = wpx(min_y, zoom) - oy;
        let right = i32::try_from(WIN_W).unwrap_or(0) - (wpx(max_x, zoom) - ox);
        let bottom = i32::try_from(WIN_H).unwrap_or(0) - (wpx(max_y, zoom) - oy);
        let tightest = left.min(top).min(right).min(bottom);
        // ★ EXACTLY, not within a pixel. Measured: a canvas margin leaves
        // `FRAME_MARGIN * zoom` = 25 px here against the screen margin's 24, so
        // a tolerance of one pixel lets the wrong arm through — which is what
        // the first draft of this assertion did, and the counterfactual it was
        // written for walked straight past it. There is no noise to absorb: the
        // arithmetic is deterministic integers, so a change that moves this by
        // a pixel SHOULD be re-measured by whoever makes it.
        assert_eq!(
            tightest,
            FRAME_MARGIN,
            "the binding axis leaves {tightest} px and FRAME_MARGIN is \
             {FRAME_MARGIN}: gutters l={left} t={top} r={right} b={bottom} at \
             zoom {zoom}. A CANVAS margin would leave {} instead",
            f64::from(FRAME_MARGIN) * zoom
        );
        assert!(
            (left - right).abs() <= 1,
            "and it is centred across: l={left} r={right}"
        );
        // ★★★ **Vertically it is NOT centred, and the reason is this editor's
        // own scroll model rather than the fit.** Measured while writing this:
        // the fit asks for a pan of about -35 px, `apply_viewport` writes it
        // through `ScrollState::scroll_to`, and a scroll offset floors at zero —
        // so a graph whose fitted position sits above the world origin is
        // pushed down against that floor and lands 26 px high (t=74 b=126).
        //
        // Stated as an assertion rather than as a comment, because a limit
        // written in prose is one nobody reads again
        // ([[debt-a-stated-limit-is-not-checked-by-anything]]). The graph is
        // fully visible either way — `r877_frame_all_fits_the_node_bbox` is what
        // holds that — and if the scroll ever learns to go negative, THIS is
        // what will say so.
        assert_eq!(
            oy, 0,
            "the vertical pan is against the scroll floor, which is why the \
             gutters differ: t={top} b={bottom}"
        );
        assert!(
            top >= FRAME_MARGIN && bottom >= FRAME_MARGIN,
            "and the margin still holds on both sides: t={top} b={bottom}"
        );
    });
}

#[test]
fn r877_frame_all_on_an_empty_graph_is_a_noop() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        coord.document.set(Graph::new("material"));
        assert!(!coord.frame_all(), "nothing to frame");
        assert!(
            (coord.zoom.get() - 1.0).abs() < f64::EPSILON,
            "viewport untouched"
        );
    });
}

#[test]
fn r877_keyboard_zoom_steps_and_resets() {
    Owner::new().run(|| {
        let mut scene = boot_full_scene();
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "=",
            mods(true, false)
        ));
        let intro = graph_intro(&scene);
        let zoomed = match intro.query("viewport.zoom") {
            Ok(IntrospectValue::Float(v)) => v,
            other => panic!("expected Float, got {other:?}"),
        };
        assert!(
            (zoomed - ZOOM_STEP).abs() < 1e-9,
            "Ctrl+= one step in, got {zoomed}"
        );
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "0",
            mods(true, false)
        ));
        assert_eq!(
            graph_intro(&scene).query("viewport.zoom"),
            Ok(IntrospectValue::Float(1.0)),
            "Ctrl+0 resets to 100%",
        );
        // 'f' frames the graph (zoom moves off 1.0).
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "f",
            mods(false, false)
        ));
        let framed = match graph_intro(&scene).query("viewport.zoom") {
            Ok(IntrospectValue::Float(v)) => v,
            other => panic!("expected Float, got {other:?}"),
        };
        assert!(
            (framed - 1.0).abs() > 1e-3,
            "f framed the graph, got {framed}"
        );
        // Plain '=' (no Ctrl) is not a zoom gesture.
        assert!(!NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "=",
            mods(false, false)
        ));
    });
}

#[test]
fn r877_drag_at_zoom_moves_in_graph_units() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        coord.set_zoom_centered(2.0);
        coord.scroll.scroll_to(0, 0);
        let id = NodeId(0);
        let before = pos_of(&coord, id);
        coord.grabbed_node.set(Some(id));
        // Press at rel (0.1, 0.1), move to rel (0.2, 0.1): 64 canvas px =
        // 32 graph units at 2x zoom.
        coord.pointer_move(0.1, 0.1);
        coord.pointer_move(0.2, 0.1);
        let after = pos_of(&coord, id);
        assert_eq!(
            after.0 - before.0,
            32,
            "64 screen px / 2x zoom = 32 graph units"
        );
        assert_eq!(after.1, before.1);
        coord.end_gesture();
    });
}

#[test]
fn r877_edge_hit_halo_is_screen_constant_across_zoom() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let nodes = default_nodes();
        let from = output_port_center(&nodes[0], 0);
        let to = input_port_center(&nodes[2], 0);
        let mid = cubic_at(
            (f64::from(from.0), f64::from(from.1)),
            {
                let (c1, _) = edge_curve(from, to);
                (f64::from(c1.0), f64::from(c1.1))
            },
            {
                let (_, c2) = edge_curve(from, to);
                (f64::from(c2.0), f64::from(c2.1))
            },
            (f64::from(to.0), f64::from(to.1)),
            0.5,
        );
        // 6 graph units off the wire: inside the 8-unit halo at zoom 1,
        // outside the (8 / 2 = 4)-unit halo at zoom 2 — the on-screen
        // tolerance stays 8 px in both cases.
        let probe = (mid.0, mid.1 - 6.0);
        assert_eq!(
            coord.hit_test_edge(probe.0, probe.1),
            Some(EdgeId(0)),
            "hit at zoom 1"
        );
        coord.set_zoom_centered(2.0);
        assert_eq!(
            coord.hit_test_edge(probe.0, probe.1),
            None,
            "missed at zoom 2"
        );
    });
}

#[test]
fn r877_add_node_spawns_inside_the_panned_view() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Pan deep into the world, then add: the new node must land in
        // the visible region, not at the boot-view spawn point.
        coord.scroll.set_max(WORLD, WORLD);
        coord.scroll.scroll_to(900, 700);
        let id = coord.add_node(0).expect("kind 0 exists");
        let pos = pos_of(&coord, id);
        assert!(
            pos.0 >= 900 && pos.1 >= 700,
            "spawn follows the viewport, got {pos:?}",
        );
    });
}

/// The canvas's world Scroll, if the view built one.
fn find_scroll(scene: &Scene) -> Option<&ScrollNode> {
    match scene {
        Scene::Scroll(s) => Some(s),
        Scene::Container(c) => c.children.iter().find_map(find_scroll),
        _ => None,
    }
}

/// Whether any `Scene::Text` under `scene` contains `needle`.
fn text_in(scene: &Scene, needle: &str) -> bool {
    match scene {
        Scene::Text(t) => t.content.contains(needle),
        Scene::Container(c) => c.children.iter().any(|ch| text_in(ch, needle)),
        Scene::Scroll(s) => text_in(s.content.as_ref(), needle),
        _ => false,
    }
}

#[test]
fn r877_view_world_scroll_is_both_axis_and_chrome_stays_outside() {
    Owner::new().run(|| {
        let scene = NodeEditorView::view(IDLE_TF, &Frame::new());
        // The canvas contains a Both-axis Scroll (the pannable world).
        let scroll = find_scroll(&scene).expect("the canvas hosts a world Scroll");
        assert_eq!(scroll.axis, ScrollAxis::Both, "2-D pan needs both axes");
        assert!(
            scroll.state.is_some(),
            "the pan state is wired for wheel routing"
        );
        // The status line (chrome) is NOT inside the scroll content.
        assert!(
            !text_in(scroll.content.as_ref(), "zoom"),
            "status chrome must not pan away with the world",
        );
        assert!(
            text_in(&scene, "zoom 100%"),
            "the status line surfaces the zoom"
        );
    });
}

// ─── R878 inline node rename ───────────────────────────────────

#[test]
fn r878_double_click_send_begins_rename_and_seeds_editor() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        send(&mut scene, "node_2:PointerDown");
        send(&mut scene, "node_2:DoubleClick");
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::Title(NodeId(2))),
            "rename armed on node 2"
        );
        let editor = use_text_edit_state(EDIT_TF_TAG);
        assert_eq!(editor.text(), "Multiply", "seeded with the current title");
        assert_eq!(editor.caret(), "Multiply".len(), "caret parked at the end");
        // The trailing PointerUp (the second click's release) still selects.
        send(&mut scene, "node_2:PointerUp");
        assert_eq!(use_selection().get(), Selection::single(NodeId(2)));
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::Title(NodeId(2))),
            "selection does not cancel the rename"
        );
        // A background double-click begins nothing.
        send(&mut scene, "DoubleClick");
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::Title(NodeId(2))),
            "background dblclick is inert"
        );
    });
}

#[test]
fn r878_begin_rename_rpc_targets_id_or_selection_and_validates() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Unknown id → false, nothing armed.
            assert_eq!(
                intro.invoke("begin_rename", IntrospectValue::Int(99)),
                Ok(IntrospectValue::Bool(false))
            );
            // Null with no selection → false.
            assert_eq!(
                intro.invoke("begin_rename", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(false))
            );
            assert_eq!(
                intro.invoke("begin_rename", IntrospectValue::Text("x".to_owned())),
                Err(InvokeError::TypeMismatch)
            );
            // Explicit id → armed.
            assert_eq!(
                intro.invoke("begin_rename", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true))
            );
        }
        assert_eq!(use_active_edit().get(), card(EditTarget::Title(NodeId(0))));
        assert_eq!(
            graph_intro(&scene).query("renaming"),
            Ok(IntrospectValue::Int(0)),
            "the read twin reports the in-flight target",
        );
        end_edit_mode(false);
        assert_eq!(
            graph_intro(&scene).query("renaming"),
            Ok(IntrospectValue::Null)
        );
        // Null with a selection → the F2 path.
        use_selection().set(Selection::single(NodeId(3)));
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        assert_eq!(
            intro.invoke("begin_rename", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true))
        );
        assert_eq!(use_active_edit().get(), card(EditTarget::Title(NodeId(3))));
    });
}

#[test]
fn r878_commit_edit_applies_and_journals_one_undo_step() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        assert!(coord.begin_rename(NodeId(2)));
        use_text_edit_state(EDIT_TF_TAG).set_text("Mix".to_owned());
        commit_edit(true);
        assert_eq!(use_active_edit().get(), None, "commit leaves rename mode");
        assert_eq!(
            coord.node_by_id(NodeId(2)).expect("present").title(),
            "Mix",
            "the title is applied",
        );
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "",
            "editor wiped for the next rename"
        );
        assert_eq!(
            stack.undo_label().as_deref(),
            Some("Rename node"),
            "journaled undoably"
        );
        assert!(stack.undo());
        assert_eq!(
            coord.node_by_id(NodeId(2)).expect("present").title(),
            "Multiply",
            "undo restores"
        );
        assert!(stack.redo());
        assert_eq!(
            coord.node_by_id(NodeId(2)).expect("present").title(),
            "Mix",
            "redo re-applies"
        );
    });
}

#[test]
fn r878_empty_whitespace_or_unchanged_commit_keeps_title_and_journals_nothing() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Empty commit: title kept, no undo step, rename mode left.
        assert!(coord.begin_rename(NodeId(1)));
        use_text_edit_state(EDIT_TF_TAG).set_text("   ".to_owned());
        commit_edit(false);
        assert_eq!(
            coord.node_by_id(NodeId(1)).expect("present").title(),
            "Color",
            "whitespace kept"
        );
        assert_eq!(use_active_edit().get(), None);
        assert!(!stack.can_undo(), "no spurious undo step");
        // Unchanged commit: successful no-op, still no undo step.
        assert!(coord.begin_rename(NodeId(1)));
        commit_edit(false);
        assert_eq!(
            coord.node_by_id(NodeId(1)).expect("present").title(),
            "Color"
        );
        assert!(!stack.can_undo(), "an unchanged title journals nothing");
    });
}

#[test]
fn r878_intervene_title_is_the_undoable_write_twin() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let stack = use_undo();
        {
            let node = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.intervene(
                    "node.2.title",
                    IntrospectValue::Text("  Blend  ".to_owned())
                ),
                Ok(()),
                "a Text write renames (trimmed)",
            );
            assert_out_of_range_saying(
                &intro.intervene("node.2.title", IntrospectValue::Text("  ".to_owned())),
                "a node title cannot be blank",
            );
            assert_eq!(
                intro.intervene("node.2.title", IntrospectValue::Int(7)),
                Err(InterveneError::TypeMismatch)
            );
            assert_eq!(
                intro.intervene("node.99.title", IntrospectValue::Text("X".to_owned())),
                Err(InterveneError::UnknownPath)
            );
            assert_eq!(
                intro.intervene("node.2.inputs", IntrospectValue::Int(3)),
                Err(InterveneError::ReadOnly),
                "port arity stays read-only",
            );
        }
        assert_eq!(
            graph_intro(&scene).query("node.2.title"),
            Ok(IntrospectValue::Text("Blend".to_owned())),
            "the query twin reads the trimmed rename back",
        );
        assert_eq!(stack.undo_label().as_deref(), Some("Rename node"));
        assert!(stack.undo());
        assert_eq!(
            graph_intro(&scene).query("node.2.title"),
            Ok(IntrospectValue::Text("Multiply".to_owned())),
            "the RPC rename undoes like an interactive one",
        );
    });
}

#[test]
fn r878_begin_rename_migration_commits_the_in_flight_rename() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        assert!(coord.begin_rename(NodeId(0)));
        use_text_edit_state(EDIT_TF_TAG).set_text("Albedo".to_owned());
        // Double-clicking node 1 while node 0's editor is open commits
        // node 0's typed text first (the toolkit item-view discipline).
        assert!(coord.begin_rename(NodeId(1)));
        assert_eq!(
            coord.node_by_id(NodeId(0)).expect("present").title(),
            "Albedo"
        );
        assert_eq!(use_active_edit().get(), card(EditTarget::Title(NodeId(1))));
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "Color",
            "the editor reseeds from the new target",
        );
        // Re-beginning the SAME node reseeds without committing (the
        // todomvc restart-editing UX).
        use_text_edit_state(EDIT_TF_TAG).set_text("Tint".to_owned());
        assert!(coord.begin_rename(NodeId(1)));
        assert_eq!(
            coord.node_by_id(NodeId(1)).expect("present").title(),
            "Color",
            "no self-commit"
        );
        assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "Color", "reseeded");
    });
}

#[test]
fn r878_rename_keymap_enter_commits_escape_cancels() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let coord = coordinator();
        assert!(coord.begin_rename(NodeId(3)));
        use_text_edit_state(EDIT_TF_TAG).set_text("Result".to_owned());
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(EDIT_TF_TAG),
            "Enter",
            Modifiers::empty()
        ));
        assert_eq!(
            coord.node_by_id(NodeId(3)).expect("present").title(),
            "Result",
            "Enter commits"
        );
        assert_eq!(use_active_edit().get(), None);
        // Escape cancels without touching the title.
        assert!(coord.begin_rename(NodeId(3)));
        use_text_edit_state(EDIT_TF_TAG).set_text("Scrap".to_owned());
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(EDIT_TF_TAG),
            "Escape",
            Modifiers::empty()
        ));
        assert_eq!(
            coord.node_by_id(NodeId(3)).expect("present").title(),
            "Result",
            "Escape cancels"
        );
        assert_eq!(use_active_edit().get(), None);
    });
}

#[test]
fn r878_blur_intent_commits_without_restoring_focus() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        assert!(coord.begin_rename(NodeId(0)));
        use_text_edit_state(EDIT_TF_TAG).set_text("Diffuse".to_owned());
        let intent = pinion_core::Intent::new_owned(
            EDIT_TF_BLUR_INTENT_TAG.to_owned(),
            IntrospectValue::Null,
        );
        let _ = NodeEditorView::update(IDLE_TF, &intent);
        assert_eq!(
            coord.node_by_id(NodeId(0)).expect("present").title(),
            "Diffuse",
            "blur commits"
        );
        assert_eq!(use_active_edit().get(), None);
        // A blur with no rename in flight is a no-op (the post-commit blur).
        let _ = NodeEditorView::update(IDLE_TF, &intent);
        assert_eq!(use_active_edit().get(), None);
    });
}

#[test]
fn r878_view_paints_the_shared_field_only_while_renaming() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let idle = view(IDLE_TF, &Frame::new());
        assert!(
            !idle.contains_tag(EDIT_TF_TAG),
            "no editor painted while idle"
        );
        assert!(coord.begin_rename(NodeId(2)));
        let editing = view((TextFieldState::Editing, 0), &Frame::new());
        assert!(
            editing.contains_tag(EDIT_TF_TAG),
            "the shared field paints over the title"
        );
    });
}

#[test]
fn r878_a11y_textbox_is_gated_on_the_same_renaming_predicate_as_paint() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let idle = NodeEditorView::access_node(&IDLE_TF, Some(GRAPH_TAG));
        assert!(
            idle.iter().all(|n| n.tag != EDIT_TF_TAG),
            "no textbox advertised while idle (paint gate == a11y gate)",
        );
        assert!(coord.begin_rename(NodeId(2)));
        let editing = NodeEditorView::access_node(&(TextFieldState::Editing, 0), Some(EDIT_TF_TAG));
        let textbox = editing
            .iter()
            .find(|n| n.tag == EDIT_TF_TAG)
            .expect("the rename field lowers to a textbox while renaming");
        assert_eq!(textbox.role, AriaRole::TextInput);
        let host = editing
            .iter()
            .find(|n| n.tag == format!("{GRAPH_TAG}#node_2"))
            .expect("renamed node present");
        assert!(
            host.children.iter().any(|c| c == EDIT_TF_TAG),
            "the textbox is the renamed node's child",
        );
    });
}
// ─── R879 multi-select ─────────────────────────────────────────

fn sel_set(ids: &[u32]) -> Selection {
    Selection::from_nodes(ids.iter().map(|&i| NodeId(i)).collect())
}

#[test]
fn r879_modifier_clicks_toggle_add_replace() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Plain click replaces.
        send(&mut scene, "node_0:PointerDown");
        send(&mut scene, "node_0:PointerUp");
        assert_eq!(use_selection().get(), Selection::single(NodeId(0)));
        // Ctrl+click adds a second member (the R781 wire token).
        send(&mut scene, "node_2:PointerDown:c");
        send(&mut scene, "node_2:PointerUp:c");
        assert_eq!(use_selection().get(), sel_set(&[0, 2]), "Ctrl toggles in");
        // Ctrl+click on a member toggles it back out.
        send(&mut scene, "node_0:PointerDown:c");
        send(&mut scene, "node_0:PointerUp:c");
        assert_eq!(
            use_selection().get(),
            Selection::single(NodeId(2)),
            "Ctrl toggles out"
        );
        // Shift+click adds (an unordered graph has no range).
        send(&mut scene, "node_1:PointerDown:s");
        send(&mut scene, "node_1:PointerUp:s");
        assert_eq!(use_selection().get(), sel_set(&[1, 2]), "Shift adds");
        // Shift+click on a member is idempotent (add, not toggle).
        send(&mut scene, "node_1:PointerDown:s");
        send(&mut scene, "node_1:PointerUp:s");
        assert_eq!(
            use_selection().get(),
            sel_set(&[1, 2]),
            "Shift re-add is a no-op"
        );
        // Plain click collapses back to a single.
        send(&mut scene, "node_3:PointerDown");
        send(&mut scene, "node_3:PointerUp");
        assert_eq!(
            use_selection().get(),
            Selection::single(NodeId(3)),
            "plain replaces"
        );
        // Toggling the last member out empties to None.
        send(&mut scene, "node_3:PointerDown:c");
        send(&mut scene, "node_3:PointerUp:c");
        assert_eq!(
            use_selection().get(),
            Selection::None,
            "empty set collapses to None"
        );
    });
}

#[test]
fn r879_delete_selected_multi_is_one_undo_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Nodes 0 and 2: node 2 (Multiply) is incident to all 3 seed
        // edges, node 0 (Texture) to edge 0 — union = all 3 edges.
        use_selection().set(sel_set(&[0, 2]));
        assert!(coord.delete_selected());
        assert_eq!(coord.node_count(), 2, "both nodes gone");
        assert_eq!(coord.edges().len(), 0, "all incident edges gone");
        assert_eq!(use_selection().get(), Selection::None, "selection pruned");
        assert_eq!(stack.undo_label().as_deref(), Some("Delete 2 nodes"));
        assert_eq!(stack.len(), 1, "ONE journal entry for the whole group");
        assert!(stack.undo());
        assert_eq!(coord.node_count(), 4, "undo restores both nodes");
        assert_eq!(coord.edges().len(), 3, "and every incident edge");
        assert_eq!(use_selection().get(), sel_set(&[0, 2]), "and the selection");
    });
}

#[test]
fn r879_multi_nudge_is_one_coalescing_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        let a0 = pos_of(&coord, NodeId(0));
        let a1 = pos_of(&coord, NodeId(1));
        use_selection().set(sel_set(&[0, 1]));
        assert!(coord.nudge_selected(NUDGE_STEP, 0));
        assert!(coord.nudge_selected(NUDGE_STEP, 0));
        assert_eq!(stack.len(), 1, "the burst coalesces to one step");
        assert_eq!(stack.undo_label().as_deref(), Some("Move 2 nodes"));
        assert_eq!(pos_of(&coord, NodeId(0)), (a0.0 + 2 * NUDGE_STEP, a0.1));
        assert_eq!(pos_of(&coord, NodeId(1)), (a1.0 + 2 * NUDGE_STEP, a1.1));
        assert!(stack.undo());
        assert_eq!(pos_of(&coord, NodeId(0)), a0, "one undo restores member 0");
        assert_eq!(pos_of(&coord, NodeId(1)), a1, "and member 1");
        // A different selection starts a fresh step (no cross-set fold).
        assert!(stack.redo());
        use_selection().set(sel_set(&[0]));
        assert!(coord.nudge_selected(0, NUDGE_STEP));
        assert_eq!(stack.len(), 2, "a different member list never folds");
    });
}

#[test]
fn r879_grabbing_a_selected_node_drags_the_group() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let stack = use_undo();
        let a0 = pos_of(&coord, NodeId(0));
        let a1 = pos_of(&coord, NodeId(1));
        use_selection().set(sel_set(&[0, 1]));
        coord.grabbed_node.set(Some(NodeId(0)));
        // First capture move snapshots the member set (both selected).
        coord.pointer_move(0.5, 0.5);
        assert_eq!(
            coord.node_drag.borrow().as_ref().map(|d| d.members.len()),
            Some(2),
            "grabbing a selected node snapshots the whole selection",
        );
        // Second move drags the group rigidly.
        coord.pointer_move(0.6, 0.5);
        let b0 = pos_of(&coord, NodeId(0));
        let b1 = pos_of(&coord, NodeId(1));
        assert_eq!(
            (b0.0 - a0.0, b0.1 - a0.1),
            (b1.0 - a1.0, b1.1 - a1.1),
            "both members move by the same delta",
        );
        assert!(b0 != a0, "the drag moved the group");
        coord.handle_send("node_0:PointerUp");
        assert_eq!(stack.len(), 1, "the whole group drag is ONE journal entry");
        assert_eq!(stack.undo_label().as_deref(), Some("Move 2 nodes"));
        assert!(stack.undo());
        assert_eq!(pos_of(&coord, NodeId(0)), a0, "undo restores member 0");
        assert_eq!(pos_of(&coord, NodeId(1)), a1, "and member 1");
    });
}

#[test]
fn r879_grabbing_an_unselected_node_drags_only_it() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let a1 = pos_of(&coord, NodeId(1));
        use_selection().set(sel_set(&[0, 1]));
        // Grab node 2 — NOT a member.
        coord.grabbed_node.set(Some(NodeId(2)));
        coord.pointer_move(0.5, 0.5);
        assert_eq!(
            coord.node_drag.borrow().as_ref().map(|d| d.members.len()),
            Some(1),
            "an unselected grab drags just the grabbed node",
        );
        coord.pointer_move(0.6, 0.6);
        assert_eq!(
            pos_of(&coord, NodeId(1)),
            a1,
            "members of the selection stay put"
        );
        assert_eq!(
            use_selection().get(),
            sel_set(&[0, 1]),
            "the selection is untouched"
        );
        coord.handle_send("PointerUp");
    });
}

#[test]
fn r879_selected_is_exact_one_and_selected_ids_is_the_set() {
    Owner::new().run(|| {
        let scene = boot_scene();
        use_selection().set(sel_set(&[1, 3]));
        let intro = graph_intro(&scene);
        assert_eq!(
            intro.query("selected"),
            Ok(IntrospectValue::Null),
            "a multi-selection has no single `selected`",
        );
        assert_eq!(
            intro.query("selected_ids"),
            Ok(IntrospectValue::Text("1,3".to_owned())),
            "the set reads back as an id-ordered CSV",
        );
        use_selection().set(Selection::single(NodeId(2)));
        assert_eq!(intro.query("selected"), Ok(IntrospectValue::Int(2)));
        assert_eq!(
            intro.query("selected_ids"),
            Ok(IntrospectValue::Text("2".to_owned()))
        );
    });
}

#[test]
fn r879_intervene_selected_ids_is_the_strict_write_twin() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        assert_eq!(
            intro.intervene("selected_ids", IntrospectValue::Text("0, 2".to_owned())),
            Ok(())
        );
        assert_eq!(
            use_selection().get(),
            sel_set(&[0, 2]),
            "CSV writes the set"
        );
        assert_out_of_range_saying(
            &intro.intervene("selected_ids", IntrospectValue::Text("0,99".to_owned())),
            "no node 99 in this graph",
        );
        assert_eq!(
            use_selection().get(),
            sel_set(&[0, 2]),
            "the rejected write changed nothing"
        );
        assert_eq!(
            intro.intervene("selected_ids", IntrospectValue::Int(3)),
            Err(InterveneError::TypeMismatch)
        );
        assert_eq!(
            intro.intervene("selected_ids", IntrospectValue::Text(String::new())),
            Ok(())
        );
        assert_eq!(
            use_selection().get(),
            Selection::None,
            "an empty CSV clears"
        );
    });
}

#[test]
fn r879_partial_delete_prunes_only_removed_members() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        use_selection().set(sel_set(&[0, 1]));
        assert!(coord.delete_node(NodeId(0)));
        assert_eq!(
            use_selection().get(),
            Selection::single(NodeId(1)),
            "the surviving member stays selected",
        );
    });
}

#[test]
fn r879_multi_selection_has_no_single_rename_target() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        use_selection().set(sel_set(&[0, 1]));
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        assert_eq!(
            intro.invoke("begin_rename", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(false)),
            "F2 on a multi-selection is ambiguous and refuses",
        );
        assert_eq!(use_active_edit().get(), None);
    });
}

#[test]
fn r879_jitter_click_neither_moves_nor_suppresses_select() {
    // The dead zone (the framework DRAG_CLICK_THRESHOLD_PX contract): a
    // press that wiggles under the threshold is a CLICK — the node does
    // not move, nothing is journaled, and the release still selects.
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let stack = use_undo();
        let p0 = pos_of(&coord, NodeId(0));
        coord.grabbed_node.set(Some(NodeId(0)));
        coord.pointer_move(0.5, 0.5); // capture seed (press point)
        coord.pointer_move(0.503, 0.5); // ~1.9 px — inside the dead zone
        assert_eq!(
            pos_of(&coord, NodeId(0)),
            p0,
            "a jitter never displaces the node"
        );
        assert!(
            !coord.gesture_moved(),
            "inside the dead zone = still a click"
        );
        coord.handle_send("node_0:PointerUp");
        assert_eq!(stack.len(), 0, "no move was journaled");
        assert_eq!(
            use_selection().get(),
            Selection::single(NodeId(0)),
            "the click selects"
        );
    });
}

#[test]
fn r879_a11y_flags_every_selected_member() {
    Owner::new().run(|| {
        let _ = boot_scene();
        use_selection().set(sel_set(&[1, 3]));
        let nodes = NodeEditorView::access_node(&IDLE_TF, Some(GRAPH_TAG));
        let flag = |i: u32| {
            nodes
                .iter()
                .find(|n| n.tag == format!("{GRAPH_TAG}#node_{i}"))
                .map(|n| n.selected)
                .expect("node entry present")
        };
        assert_eq!(flag(1), Some(true), "member 1 flagged");
        assert_eq!(flag(3), Some(true), "member 3 flagged");
        assert_eq!(flag(0), Some(false), "non-member unflagged");
    });
}

// ─── R948 align / distribute ───────────────────────────────────

/// Place the named nodes at explicit positions (the low-level, non-journaling
/// `set_node_pos`) and select exactly them — the shared setup so the
/// assertions read from known geometry, not the seed layout.
fn place_and_select(coord: &NodeGraphExternal, placements: &[(u32, i32, i32)]) {
    for &(id, x, y) in placements {
        assert!(coord.set_node_pos(NodeId(id), x, y), "node {id} present");
    }
    let ids: Vec<u32> = placements.iter().map(|&(id, ..)| id).collect();
    use_selection().set(sel_set(&ids));
}

#[test]
fn r949_1_walled_nudge_stays_handled_so_it_does_not_pan() {
    // R949.1 regression: a nudge of a selection pinned at the world edge
    // clamps to a no-op, but must still report HANDLED — else the shell
    // falls the unhandled arrow through to scroll_key and pans the canvas.
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Pin node 0 at the right wall, then nudge further right (clamped).
        assert!(coord.set_node_pos(NodeId(0), WORLD - NODE_W, 50));
        use_selection().set(sel_set(&[0]));
        let before = pos_of(&coord, NodeId(0));
        assert!(
            coord.nudge_selected(NUDGE_STEP, 0),
            "a clamped nudge of a selection is still handled (must not fall through to pan)",
        );
        assert_eq!(
            pos_of(&coord, NodeId(0)),
            before,
            "the node did not actually move (walled)"
        );
        // With no selection the arrow is unhandled -> the shell pans.
        use_selection().set(Selection::None);
        assert!(
            !coord.nudge_selected(NUDGE_STEP, 0),
            "no selection -> unhandled (canvas pans)"
        );
    });
}

#[test]
fn r948_align_horizontal_snaps_x_to_left_centre_right() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Left edges 100 / 300 / 200; equal NODE_W (130) -> right edges
        // 230 / 430 / 330 -> bbox left = 100, right = 430.
        let setup = |c: &NodeGraphExternal| {
            place_and_select(c, &[(0, 100, 50), (1, 300, 80), (2, 200, 120)]);
        };
        setup(&coord);
        assert!(coord.align_selected(AlignSpec::Left));
        for id in [0u32, 1, 2] {
            assert_eq!(pos_of(&coord, NodeId(id)).0, 100, "left: x -> bbox left");
        }
        setup(&coord);
        assert!(coord.align_selected(AlignSpec::Right));
        for id in [0u32, 1, 2] {
            assert_eq!(
                pos_of(&coord, NodeId(id)).0,
                300,
                "right: x -> bbox right - w (430-130)"
            );
        }
        setup(&coord);
        assert!(coord.align_selected(AlignSpec::CenterH));
        for id in [0u32, 1, 2] {
            // midpoint(100, 430) - NODE_W/2 = 265 - 65 = 200.
            assert_eq!(
                pos_of(&coord, NodeId(id)).0,
                200,
                "centre_h: centres on the bbox mid"
            );
        }
        assert_eq!(
            pos_of(&coord, NodeId(0)).1,
            50,
            "a horizontal align never touches y"
        );
    });
}

#[test]
fn r948_align_vertical_respects_each_node_height() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Node 0 (Texture, 1 port row) is short; node 2 (Multiply, 2 rows)
        // is tall — so vertical centre / bottom must use each node's own
        // height(), not a shared constant.
        let h0 = coord.node_by_id(NodeId(0)).unwrap().height();
        let h2 = coord.node_by_id(NodeId(2)).unwrap().height();
        assert!(h2 > h0, "the 2-port card is taller than the 1-port card");
        let setup = |c: &NodeGraphExternal| {
            place_and_select(c, &[(0, 40, 50), (2, 300, 200)]);
        };
        setup(&coord);
        assert!(coord.align_selected(AlignSpec::Top));
        assert_eq!(
            pos_of(&coord, NodeId(0)).1,
            50,
            "top: both y -> bbox top (min)"
        );
        assert_eq!(
            pos_of(&coord, NodeId(2)).1,
            50,
            "top: both y -> bbox top (min)"
        );
        setup(&coord);
        assert!(coord.align_selected(AlignSpec::Bottom));
        let (b0, b2) = (pos_of(&coord, NodeId(0)), pos_of(&coord, NodeId(2)));
        assert_eq!(
            b0.1 + h0,
            b2.1 + h2,
            "bottom: bottom EDGES align (per-node height)"
        );
        assert_eq!(
            b2.1, 200,
            "the lowest card is the anchor (it does not move)"
        );
        assert_eq!(
            b0.1,
            200 + h2 - h0,
            "the short card drops so its bottom matches"
        );
        setup(&coord);
        assert!(coord.align_selected(AlignSpec::CenterV));
        let (c0, c2) = (pos_of(&coord, NodeId(0)), pos_of(&coord, NodeId(2)));
        assert_eq!(
            c0.1 + h0 / 2,
            c2.1 + h2 / 2,
            "centre_v: vertical centres coincide"
        );
        assert_eq!(c0.0, 40, "a vertical align never touches x");
    });
}

#[test]
fn r948_align_needs_two_and_a_noop_journals_nothing() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        use_selection().set(sel_set(&[0]));
        assert!(
            !coord.align_selected(AlignSpec::Left),
            "one node has nothing to align to"
        );
        use_selection().set(Selection::None);
        assert!(
            !coord.align_selected(AlignSpec::Top),
            "an empty selection -> false"
        );
        assert_eq!(stack.len(), 0, "no undo step from a too-small align");
        // An already-aligned selection moves nothing and journals nothing.
        place_and_select(&coord, &[(0, 100, 50), (1, 100, 200)]);
        assert!(
            !coord.align_selected(AlignSpec::Left),
            "already left-aligned -> no move"
        );
        assert_eq!(stack.len(), 0, "an idempotent align journals nothing");
    });
}

#[test]
fn r948_align_is_one_discrete_undo_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        place_and_select(&coord, &[(0, 100, 50), (1, 300, 80), (2, 200, 120)]);
        let (p0, p1, p2) = (
            pos_of(&coord, NodeId(0)),
            pos_of(&coord, NodeId(1)),
            pos_of(&coord, NodeId(2)),
        );
        assert!(coord.align_selected(AlignSpec::Right));
        assert_eq!(stack.len(), 1, "one discrete (non-coalescing) undo step");
        // A second, different align does NOT fold into the first.
        assert!(coord.align_selected(AlignSpec::Top));
        assert_eq!(stack.len(), 2, "discrete aligns never coalesce");
        assert!(stack.undo());
        assert!(stack.undo());
        assert_eq!(pos_of(&coord, NodeId(0)), p0, "two undos restore node 0");
        assert_eq!(pos_of(&coord, NodeId(1)), p1, "and node 1");
        assert_eq!(pos_of(&coord, NodeId(2)), p2, "and node 2");
    });
}

#[test]
fn r948_distribute_h_equalises_centres_holding_extremes() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Centres (x + NODE_W/2 = x + 65): 165 / 215 / 565 (uneven).
        place_and_select(&coord, &[(0, 100, 50), (1, 150, 50), (2, 500, 50)]);
        assert!(coord.distribute_selected(DistributeAxis::Horizontal));
        assert_eq!(
            pos_of(&coord, NodeId(0)).0,
            100,
            "leftmost extreme stays fixed"
        );
        assert_eq!(
            pos_of(&coord, NodeId(2)).0,
            500,
            "rightmost extreme stays fixed"
        );
        // Middle centre -> midpoint(165, 565) = 365 -> x = 365 - 65 = 300.
        assert_eq!(
            pos_of(&coord, NodeId(1)).0,
            300,
            "middle centre evenly spaced"
        );
        assert_eq!(stack.len(), 1, "one undo step");
    });
}

#[test]
fn r948_distribute_v_equalises_centres() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Nodes 0 / 1 / 3 are all 1-row cards -> equal height, so the
        // centre spacing is clean y arithmetic.
        place_and_select(&coord, &[(0, 40, 100), (1, 40, 150), (3, 40, 500)]);
        assert!(coord.distribute_selected(DistributeAxis::Vertical));
        assert_eq!(
            pos_of(&coord, NodeId(0)).1,
            100,
            "topmost extreme stays fixed"
        );
        assert_eq!(
            pos_of(&coord, NodeId(3)).1,
            500,
            "bottommost extreme stays fixed"
        );
        assert_eq!(
            pos_of(&coord, NodeId(1)).1,
            300,
            "middle centre evenly spaced (y)"
        );
    });
}

#[test]
fn r948_distribute_needs_three_selected() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        place_and_select(&coord, &[(0, 100, 50), (1, 400, 50)]);
        assert!(
            !coord.distribute_selected(DistributeAxis::Horizontal),
            "two nodes have no middle to space",
        );
        use_selection().set(sel_set(&[0]));
        assert!(
            !coord.distribute_selected(DistributeAxis::Vertical),
            "one node -> false"
        );
        assert_eq!(stack.len(), 0, "no undo step from a too-small distribute");
    });
}

#[test]
fn r948_layout_verbs_dispatch_and_are_schema_declared() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        place_and_select(&coord, &[(0, 100, 50), (1, 300, 80), (2, 200, 120)]);
        // The invoke dispatch routes through invoke_layout -> Bool.
        assert_eq!(
            coord.invoke("align_left", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true)),
            "align_left moves the selection",
        );
        // After align_left the x are collinear, so distribute_h is a no-op.
        assert_eq!(
            coord.invoke("distribute_h", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(false)),
            "distribute_h over a same-x set moves nothing",
        );
        // Every layout verb is schema-declared (AI-discoverable).
        let fields: Vec<&str> = coord.schema().fields.iter().map(|f| f.path).collect();
        for v in [
            "align_left",
            "align_center_h",
            "align_right",
            "align_top",
            "align_center_v",
            "align_bottom",
            "distribute_h",
            "distribute_v",
        ] {
            assert!(fields.contains(&v), "{v} must be schema-declared");
        }
        // An unknown verb still falls through to UnknownPath.
        assert_eq!(
            coord.invoke("align_nope", IntrospectValue::Null),
            Err(InvokeError::UnknownPath),
        );
    });
}

// ── R1383 auto-layout (layered / Sugiyama) ────────────────────────

#[test]
fn r1383_auto_layout_tidies_the_graph_in_one_undo_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Scramble the seed graph: sink far left, sources far right (reversed).
        place_and_select(
            &coord,
            &[(3, 40, 60), (2, 240, 300), (1, 500, 60), (0, 520, 260)],
        );
        let before0 = pos_of(&coord, NodeId(0));
        assert!(
            coord.auto_layout(),
            "auto_layout rearranged the scrambled graph"
        );
        assert_eq!(
            stack.len(),
            1,
            "a whole re-layout is ONE discrete undo step"
        );
        let x = |id: u32| pos_of(&coord, NodeId(id)).0;
        // Texture(0) x Color(1) -> Multiply(2) -> Output(3).
        assert!(x(0) < x(2), "source 0 lands left of its consumer");
        assert!(x(1) < x(2), "source 1 lands left of its consumer");
        assert!(x(2) < x(3), "Multiply lands left of Output");
        assert_eq!(x(0), x(1), "the two sources share the layer-0 column");
        assert!(stack.undo(), "one undo reverts the whole arrangement");
        assert_eq!(
            pos_of(&coord, NodeId(0)),
            before0,
            "undo restores the pre-layout position"
        );
    });
}

#[test]
fn r1383_auto_layout_is_idempotent_and_needs_two_nodes() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        assert!(coord.auto_layout(), "the first pass tidies the seed graph");
        let snapshot: Vec<(i32, i32)> = [0u32, 1, 2, 3]
            .map(|id| pos_of(&coord, NodeId(id)))
            .to_vec();
        assert!(
            !coord.auto_layout(),
            "a second pass moves nothing (idempotent)"
        );
        for (id, before) in [0u32, 1, 2, 3].iter().zip(snapshot) {
            assert_eq!(pos_of(&coord, NodeId(*id)), before, "node {id} unchanged");
        }
    });
}

// ── R1390 force-directed (organic) auto-layout ────────────────────

#[test]
fn r1390_force_layout_tidies_in_one_undo_step_and_reverts() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Scramble the seed graph, then relax it.
        place_and_select(
            &coord,
            &[(3, 40, 60), (2, 240, 300), (1, 500, 60), (0, 520, 260)],
        );
        let before0 = pos_of(&coord, NodeId(0));
        assert!(
            coord.force_layout(),
            "force_layout relaxed the scrambled graph"
        );
        assert_eq!(
            stack.len(),
            1,
            "a whole relaxation is ONE discrete undo step"
        );
        assert!(stack.undo(), "one undo reverts the whole relaxation");
        assert_eq!(
            pos_of(&coord, NodeId(0)),
            before0,
            "undo restores the pre-layout position"
        );
    });
}

#[test]
fn r1390_force_layout_is_idempotent_and_needs_two_nodes() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        assert!(
            coord.force_layout(),
            "the first pass relaxes the seed graph"
        );
        let snapshot: Vec<(i32, i32)> = [0u32, 1, 2, 3]
            .map(|id| pos_of(&coord, NodeId(id)))
            .to_vec();
        assert!(
            !coord.force_layout(),
            "a second pass moves nothing (idempotent)"
        );
        for (id, before) in [0u32, 1, 2, 3].iter().zip(snapshot) {
            assert_eq!(pos_of(&coord, NodeId(*id)), before, "node {id} unchanged");
        }
    });
}

#[test]
fn r1390_force_layout_dispatches_via_invoke_and_is_schema_declared() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        // Schema-declared (AI-discoverable) beside the layered `auto_layout`.
        let fields: Vec<&str> = coord.schema().fields.iter().map(|f| f.path).collect();
        assert!(
            fields.contains(&"force_layout"),
            "force_layout must be schema-declared"
        );
        // Routes through invoke_layout -> Bool, relaxing the seed graph.
        assert_eq!(
            coord.invoke("force_layout", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true)),
            "force_layout dispatches and relaxes the seed graph",
        );
    });
}

// ── R1226 wire knife (cut_wires) ──────────────────────────────────

#[test]
fn r1226_segment_and_edge_cross_geometry() {
    // A horizontal segment crossed by a vertical one -> proper cross.
    assert!(segments_cross(
        (0.0, 0.0),
        (10.0, 0.0),
        (5.0, -5.0),
        (5.0, 5.0)
    ));
    // Parallel segments never cross.
    assert!(!segments_cross(
        (0.0, 0.0),
        (10.0, 0.0),
        (0.0, 5.0),
        (10.0, 5.0)
    ));
    // A vertical line past the horizontal segment's end -> miss.
    assert!(!segments_cross(
        (0.0, 0.0),
        (10.0, 0.0),
        (20.0, -5.0),
        (20.0, 5.0)
    ));
    // A wire from (0,50)->(100,50) is a flat curve at y=50; a vertical cut at
    // x=50 spanning it crosses, one offset below it does not.
    assert!(edge_crosses_segment((0, 50), (100, 50), (50, 0), (50, 100)));
    assert!(!edge_crosses_segment(
        (0, 50),
        (100, 50),
        (50, 60),
        (50, 100)
    ));
}

#[test]
fn r1226_cut_wires_removes_only_the_crossed_edges() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        assert_eq!(coord.edges().len(), 3, "boot: 3 wires");
        // A vertical knife at graph-x=200 (between the left column's right
        // edge x=170 and Multiply's left x=250) crosses edges 0 + 1 (into
        // Multiply) but not edge 2 (Multiply -> Output, x in [374,476]).
        let cut = coord.cut_wires((200, 20), (200, 380));
        assert_eq!(cut, vec![EdgeId(0), EdgeId(1)], "edges 0 and 1 are cut");
        assert_eq!(coord.edges().len(), 1, "only Multiply -> Output survives");
        assert!(
            coord.edges().iter().any(|e| e.id == EdgeId(2)),
            "edge 2 (past the knife) is untouched"
        );
    });
}

#[test]
fn r1226_cut_wires_is_one_undo_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        assert!(!stack.can_undo(), "boot: clean history");
        let cut = coord.cut_wires((200, 20), (200, 380));
        assert_eq!(cut.len(), 2, "two wires cut");
        assert_eq!(coord.edges().len(), 1);
        assert_eq!(stack.undo_label().as_deref(), Some("Cut wires"));
        assert!(stack.undo(), "one undo restores BOTH cut wires");
        assert_eq!(coord.edges().len(), 3, "the whole cut reverts in one step");
        assert!(!stack.can_undo(), "the cut was a single journal entry");
    });
}

#[test]
fn r1226_cut_wires_miss_is_a_noop_with_no_undo_entry() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // A stroke in empty space (far below every wire) crosses nothing.
        let cut = coord.cut_wires((600, 500), (700, 520));
        assert!(cut.is_empty(), "no wire crossed -> nothing cut");
        assert_eq!(coord.edges().len(), 3, "graph unchanged");
        assert!(!stack.can_undo(), "a no-op cut records no undo entry");
    });
}

#[test]
fn r1226_cut_wires_verb_schema_and_wire_form() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let fields: Vec<&str> = coord.schema().fields.iter().map(|f| f.path).collect();
        assert!(fields.contains(&"cut_wires"), "cut_wires schema-declared");
        // The verb returns the CSV of cut ids (mirrors `edge_ids`).
        assert_eq!(
            coord.invoke(
                "cut_wires",
                IntrospectValue::Text("200,20,200,380".to_owned())
            ),
            Ok(IntrospectValue::Text("0,1".to_owned())),
            "the verb cuts edges 0+1 and returns their id CSV",
        );
        // A malformed spec Rejects (never a silent empty cut); a non-string
        // arg is a TypeMismatch.
        assert_refused_saying(
            &coord.invoke("cut_wires", IntrospectValue::Text("bad".to_owned())),
            "malformed cut spec \"bad\"",
        );
        assert_eq!(
            coord.invoke("cut_wires", IntrospectValue::Int(1)),
            Err(InvokeError::TypeMismatch),
        );
    });
}

// ── R1227 comment frames ──────────────────────────────────────────

#[test]
fn r1227_add_frame_encloses_the_selection_only() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        assert!(coord.frames().is_empty(), "boot: no frames");
        // Frame the two left-column nodes (Texture id0 @ (40,70), Color id1 @ (40,210)).
        coord.set_selection(Selection::Nodes(BTreeSet::from([NodeId(0), NodeId(1)])));
        let id = coord.add_frame().expect("framed the selection");
        // R1596 — a frame IS a node, so it mints from the node counter: the seed
        // graph holds four, and the frame is the fifth thing in the tree.
        assert_eq!(id, NodeId(4), "the frame mints the next NODE id");
        assert_eq!(coord.frames().len(), 1);
        let f = coord.frame_by_id(id).unwrap();
        assert_eq!(f.title(), "Comment 1");
        // The rect encloses both framed nodes with the FRAME_PAD margin.
        assert!(f.x <= 40 - FRAME_PAD, "left margin");
        assert!(
            f.y <= 70 - FRAME_PAD - FRAME_HEADER_H,
            "top margin + header"
        );
        assert!(f.right() >= 40 + NODE_W + FRAME_PAD, "right margin");
        // R1596 — membership is a FACT the document maintains (`Node::parent`),
        // set by `enframe`, not a rectangle re-tested on every read.
        assert_eq!(
            coord.graph().members(TREE, id),
            vec![NodeId(0), NodeId(1)],
            "the framed selection is what the frame holds"
        );
        assert!(
            !coord.graph().members(TREE, id).contains(&NodeId(3)),
            "far-right Output is not a member"
        );
        // And the geometry agrees with it, which is what the gesture asks.
        let nodes = coord.nodes();
        let by = |id: NodeId| nodes.iter().find(|n| n.id == id).unwrap().clone();
        assert!(frame_contains(&f, &by(NodeId(0))), "Texture sits inside");
        assert!(frame_contains(&f, &by(NodeId(1))), "Color sits inside");
        assert!(!frame_contains(&f, &by(NodeId(3))), "Output sits outside");
        // Frames are a separate axis — the node selection is untouched.
        assert_eq!(
            coord.selection.get(),
            Selection::Nodes(BTreeSet::from([NodeId(0), NodeId(1)])),
        );
    });
}

#[test]
fn r1227_add_frame_with_no_node_selection_is_none() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        coord.set_selection(Selection::None);
        assert!(coord.add_frame().is_none(), "no selection -> no frame");
        assert!(coord.frames().is_empty());
    });
}

#[test]
fn r1227_add_remove_frame_undo_redo_one_step_each() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        coord.select_all();
        let id = coord.add_frame().unwrap();
        assert_eq!(coord.frames().len(), 1);
        assert_eq!(stack.undo_label().as_deref(), Some("Add frame"));
        assert!(stack.undo(), "undo the add");
        assert!(coord.frames().is_empty(), "frame gone");
        assert!(stack.redo(), "redo restores it");
        assert_eq!(
            coord.frame_by_id(id).map(|f| f.id),
            Some(id),
            "same stable id"
        );
        // remove_frame is its own undo step; the nodes are untouched.
        assert!(coord.remove_frame(id));
        assert!(coord.frames().is_empty());
        assert_eq!(coord.node_count(), 4, "removing a frame keeps the nodes");
        assert_eq!(stack.undo_label().as_deref(), Some("Remove frame"));
        assert!(stack.undo(), "undo the remove restores the frame");
        assert_eq!(coord.frames().len(), 1);
    });
}

#[test]
fn r1227_frame_rename_undoable() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        coord.select_all();
        let id = coord.add_frame().unwrap();
        let title_path = format!("frame.{}.title", id.0);
        coord
            .intervene(&title_path, IntrospectValue::Text("Lighting".to_owned()))
            .unwrap();
        assert_eq!(coord.frame_by_id(id).unwrap().title(), "Lighting");
        assert!(use_undo().undo(), "undo the rename");
        assert_eq!(coord.frame_by_id(id).unwrap().title(), "Comment 1");
        // R1234 — the rect is now writable (move / resize); a non-`Int` value on
        // a rect field is a `TypeMismatch`, not `ReadOnly`.
        assert_eq!(
            coord.intervene(
                &format!("frame.{}.x", id.0),
                IntrospectValue::Text("nope".to_owned()),
            ),
            Err(InterveneError::TypeMismatch),
        );
        // An empty title is rejected (the frame keeps its name).
        assert_out_of_range_saying(
            &coord.intervene(&title_path, IntrospectValue::Text("  ".to_owned())),
            "a frame title cannot be blank",
        );
        // An unknown frame id is UnknownPath.
        assert_eq!(
            coord.intervene("frame.99.title", IntrospectValue::Text("x".to_owned())),
            Err(InterveneError::UnknownPath),
        );
    });
}

#[test]
fn r1227_frame_introspection_contains_and_verb_schema() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        coord.set_selection(Selection::Nodes(BTreeSet::from([NodeId(0), NodeId(1)])));
        let id = coord.add_frame().unwrap();
        assert_eq!(coord.query("frame_count"), Ok(IntrospectValue::Int(1)));
        // R1596 — a frame is a NODE, so it mints from the node counter: the
        // seed graph holds four, and the frame is the fifth thing in the tree.
        assert_eq!(
            coord.query("frame_ids"),
            Ok(IntrospectValue::Text("4".to_owned()))
        );
        assert_eq!(
            coord.query(&format!("frame.{}.title", id.0)),
            Ok(IntrospectValue::Text("Comment 1".to_owned()))
        );
        // R1596 — `contains` is the RELATION (`Node::parent`) the framing wrote,
        // not a rectangle re-tested on every read.
        assert_eq!(
            coord.query(&format!("frame.{}.contains", id.0)),
            Ok(IntrospectValue::Text("0,1".to_owned()))
        );
        // The AI-first verbs + read handles are schema-declared.
        let fields: Vec<&str> = coord.schema().fields.iter().map(|f| f.path).collect();
        for v in ["add_frame", "remove_frame", "frame_count", "frame_ids"] {
            assert!(fields.contains(&v), "{v} schema-declared");
        }
    });
}

#[test]
fn r1227_frame_persists_and_paints_behind() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        coord.select_all();
        coord.add_frame().unwrap();
        // Persistence: the frame round-trips through the current-schema blob.
        let json = coord.serialized_json();
        // R1599 — read off the constant rather than a literal. A hand-written
        // number here was a SECOND source of truth for the version, and it went
        // stale the moment the shape changed; the gate that now forces the bump
        // is `r1599_the_persisted_shape_cannot_change_without_the_version`.
        assert!(
            json.contains(&format!("\"schema_version\": {PERSISTED_SCHEMA_VERSION}")),
            "the blob carries the schema version it was written at"
        );
        assert!(
            json.contains(&format!("\"revision\": {}", pinion_node_graph::REVISION)),
            "and R1689 — the archive FORMAT's own revision, which is a different fact"
        );
        assert!(json.contains("Comment 1"), "the frame is in the blob");
        coord.document.set(Graph::new("material"));
        assert!(coord.load_json(&json), "load the snapshot");
        assert_eq!(coord.frames().len(), 1, "frame restored from persistence");
        // Paint: the frame's tagged rect is present in the scene.
        let scene = view(IDLE_TF, &Frame::new());
        let painted = coord
            .frames()
            .first()
            .map(|f| format!("{GRAPH_TAG}#frame_{}", f.id))
            .expect("a restored frame");
        assert!(scene.contains_tag(&painted), "the comment frame is painted");
    });
}

#[test]
fn r1227_a11y_frame_is_a_labeled_group_child_of_the_canvas() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        coord.set_selection(Selection::Nodes(BTreeSet::from([NodeId(0), NodeId(1)])));
        coord.add_frame().unwrap();
        let a11y = NodeEditorView::access_node(&IDLE_TF, None);
        let frame_tag = format!("{GRAPH_TAG}#frame_{}", NodeId(4));
        let frame = a11y
            .iter()
            .find(|n| n.tag == frame_tag)
            .expect("frame a11y node");
        assert_eq!(frame.role, AriaRole::Group);
        let name = frame.name.as_deref().unwrap_or_default();
        assert!(name.contains("Comment 1"), "named by title");
        assert!(name.contains("2 nodes"), "announces the membership count");
        // The canvas group references it (not an orphan in the AT tree).
        let graph = a11y.iter().find(|n| n.tag == GRAPH_TAG).unwrap();
        assert!(
            graph.children.iter().any(|c| c == &frame_tag),
            "canvas group references the frame"
        );
    });
}

/// R1234 — a frame around {0,1}, moved via `intervene frame.<id>.x`, and one
/// coordinator (`select {0,1}` → add_frame). Shared setup for the move tests.
fn framed_pair() -> (NodeGraphExternal, FrameId) {
    let _ = boot_scene();
    let coord = coordinator();
    coord.set_selection(Selection::Nodes(BTreeSet::from([NodeId(0), NodeId(1)])));
    let id = coord.add_frame().unwrap();
    (coord, id)
}

#[test]
fn r1234_frame_move_carries_contents_as_one_undo_step() {
    Owner::new().run(|| {
        let (mut coord, id) = framed_pair();
        let fx0 = coord.frame_by_id(id).unwrap().x;
        let (n0x, n1x, n2x) = (
            coord.node_by_id(NodeId(0)).unwrap().x,
            coord.node_by_id(NodeId(1)).unwrap().x,
            coord.node_by_id(NodeId(2)).unwrap().x,
        );
        // Move the frame right by 60 graph units.
        coord
            .intervene(
                &format!("frame.{}.x", id.0),
                IntrospectValue::Int(i64::from(fx0 + 60)),
            )
            .unwrap();
        assert_eq!(coord.frame_by_id(id).unwrap().x, fx0 + 60, "frame moved");
        assert_eq!(
            coord.node_by_id(NodeId(0)).unwrap().x,
            n0x + 60,
            "framed node 0 moved with the frame"
        );
        assert_eq!(
            coord.node_by_id(NodeId(1)).unwrap().x,
            n1x + 60,
            "framed node 1 moved with the frame"
        );
        assert_eq!(
            coord.node_by_id(NodeId(2)).unwrap().x,
            n2x,
            "node 2 (outside the frame) is untouched"
        );
        // ONE undo step restores the frame AND both members together.
        assert_eq!(
            use_undo().undo_label().as_deref(),
            Some("Move frame"),
            "the move is a single labelled step"
        );
        assert!(use_undo().undo(), "one undo reverts the whole move");
        assert_eq!(coord.frame_by_id(id).unwrap().x, fx0, "frame restored");
        assert_eq!(
            coord.node_by_id(NodeId(0)).unwrap().x,
            n0x,
            "node 0 restored"
        );
        assert_eq!(
            coord.node_by_id(NodeId(1)).unwrap().x,
            n1x,
            "node 1 restored"
        );
        // Redo re-applies it in one step.
        assert!(use_undo().redo(), "redo the move");
        assert_eq!(
            coord.node_by_id(NodeId(0)).unwrap().x,
            n0x + 60,
            "node 0 re-moved"
        );
    });
}

#[test]
fn r1234_frame_move_is_a_rigid_group_clamp_at_the_world_edge() {
    Owner::new().run(|| {
        let (mut coord, id) = framed_pair();
        // A y-move carries the contents on the other axis too.
        let fy0 = coord.frame_by_id(id).unwrap().y;
        let start_y = coord.node_by_id(NodeId(0)).unwrap().y;
        coord
            .intervene(
                &format!("frame.{}.y", id.0),
                IntrospectValue::Int(i64::from(fy0 + 40)),
            )
            .unwrap();
        assert_eq!(
            coord.frame_by_id(id).unwrap().y,
            fy0 + 40,
            "frame moved down"
        );
        assert_eq!(
            coord.node_by_id(NodeId(0)).unwrap().y,
            start_y + 40,
            "member moved down with it"
        );
        // Push x far past the world: the group clamps rigidly — every member
        // stays on-world and the frame→node offset is preserved (no slide-out).
        let rel = coord.frame_by_id(id).unwrap().x - coord.node_by_id(NodeId(0)).unwrap().x;
        coord
            .intervene(
                &format!("frame.{}.x", id.0),
                IntrospectValue::Int(1_000_000),
            )
            .unwrap();
        let first = coord.node_by_id(NodeId(0)).unwrap().x;
        let second = coord.node_by_id(NodeId(1)).unwrap().x;
        assert!(
            first <= WORLD - NODE_W,
            "node 0 stayed on the world surface"
        );
        assert!(
            second <= WORLD - NODE_W,
            "node 1 stayed on the world surface"
        );
        assert_eq!(
            coord.frame_by_id(id).unwrap().x - first,
            rel,
            "the frame→member offset is preserved (rigid group move)"
        );
    });
}

#[test]
fn r1234_frame_resize_changes_size_not_positions_and_recomputes_membership() {
    Owner::new().run(|| {
        let (mut coord, id) = framed_pair();
        let contains = format!("frame.{}.contains", id.0);
        assert_eq!(
            coord.query(&contains),
            Ok(IntrospectValue::Text("0,1".to_owned())),
            "only the two framed nodes to start"
        );
        let (n0, n2) = (
            coord.node_by_id(NodeId(0)).unwrap(),
            coord.node_by_id(NodeId(2)).unwrap(),
        );
        // Grow the box wide enough to swallow the whole graph.
        coord
            .intervene(&format!("frame.{}.w", id.0), IntrospectValue::Int(800))
            .unwrap();
        assert_eq!(coord.frame_by_id(id).unwrap().width(), 800, "width grew");
        // A resize never drags nodes.
        assert_eq!(
            coord.node_by_id(NodeId(0)).unwrap().x,
            n0.x,
            "node 0 stayed put"
        );
        assert_eq!(
            coord.node_by_id(NodeId(0)).unwrap().y,
            n0.y,
            "node 0 y stayed put"
        );
        assert_eq!(
            coord.node_by_id(NodeId(2)).unwrap().x,
            n2.x,
            "node 2 stayed put"
        );
        // ★R1596 — MEMBERSHIP DOES NOT MOVE. The editor re-derived it from the
        // rectangle on every read, so widening the box silently adopted two
        // more nodes and shrinking it abandoned them — what the frame *said*
        // it held changed with nobody having edited membership at all. It is a
        // stored relation now (`Node::parent`, R1589, the DCC's model), so a resize
        // changes the box and nothing else, and joining is the explicit act
        // `attach` performs.
        assert_eq!(
            coord.query(&contains),
            Ok(IntrospectValue::Text("0,1".to_owned())),
            "a resize changes the box, not what the frame holds"
        );
        assert_eq!(
            use_undo().undo_label().as_deref(),
            Some("Resize frame"),
            "a resize is its own labelled step"
        );
        assert!(use_undo().undo(), "undo the resize");
        assert_eq!(
            coord.query(&contains),
            Ok(IntrospectValue::Text("0,1".to_owned())),
            "and undoing it leaves membership alone too"
        );
        // The nodes ARE inside the widened box geometrically — which is the
        // question a GESTURE asks, and `attach` is what turns that answer into
        // membership.
        coord
            .intervene(&format!("frame.{}.w", id.0), IntrospectValue::Int(800))
            .unwrap();
        coord.set_selection(Selection::Nodes(BTreeSet::from([NodeId(2), NodeId(3)])));
        assert_eq!(
            coord.attach_selected(),
            vec![NodeId(2), NodeId(3)],
            "attach is the act that joins them"
        );
        assert_eq!(
            coord.query(&contains),
            Ok(IntrospectValue::Text("0,1,2,3".to_owned())),
            "and now the frame holds all four"
        );
    });
}

#[test]
fn r1234_frame_resize_clamps_to_the_minimum() {
    Owner::new().run(|| {
        let (mut coord, id) = framed_pair();
        // Collapsing below the chrome height clamps to FRAME_MIN, never zero.
        coord
            .intervene(&format!("frame.{}.w", id.0), IntrospectValue::Int(1))
            .unwrap();
        assert_eq!(
            coord.frame_by_id(id).unwrap().width(),
            FRAME_MIN,
            "width clamped"
        );
        coord
            .intervene(&format!("frame.{}.h", id.0), IntrospectValue::Int(-9))
            .unwrap();
        assert_eq!(
            coord.frame_by_id(id).unwrap().height(),
            FRAME_MIN,
            "height clamped"
        );
    });
}

#[test]
fn r1234_frame_geom_type_and_path_errors() {
    Owner::new().run(|| {
        let (mut coord, id) = framed_pair();
        // A non-Int rect value is a TypeMismatch (not ReadOnly any more).
        assert_eq!(
            coord.intervene(
                &format!("frame.{}.w", id.0),
                IntrospectValue::Text("wide".to_owned()),
            ),
            Err(InterveneError::TypeMismatch),
        );
        // An unknown frame id / field is UnknownPath.
        assert_eq!(
            coord.intervene("frame.99.x", IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath),
        );
        assert_eq!(
            coord.intervene(&format!("frame.{}.zz", id.0), IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath),
        );
        // A no-op move (same x) journals nothing — the last step is still the add.
        coord
            .intervene(
                &format!("frame.{}.x", id.0),
                IntrospectValue::Int(i64::from(coord.frame_by_id(id).unwrap().x)),
            )
            .unwrap();
        assert_eq!(
            use_undo().undo_label().as_deref(),
            Some("Add frame"),
            "a no-op move records no undo step"
        );
    });
}

#[test]
fn r1235_add_reroute_splices_edge_as_one_undo_step() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Seed edge 0: node0.out0 -> node2.in0 (a Vector wire).
        assert_eq!(coord.query("node_count"), Ok(IntrospectValue::Int(4)));
        assert_eq!(coord.query("edge_count"), Ok(IntrospectValue::Int(3)));
        let rid = coord.add_reroute(EdgeId(0)).expect("edge 0 exists");
        // A fifth node (the reroute) and a net +1 edge (removed 1, added 2).
        assert_eq!(coord.query("node_count"), Ok(IntrospectValue::Int(5)));
        assert_eq!(coord.query("edge_count"), Ok(IntrospectValue::Int(4)));
        // The spliced edge is gone; the path now routes node0 -> R -> node2.
        let edges = coord.edges();
        assert!(
            !edges.iter().any(|e| e.id == EdgeId(0)),
            "the original edge is removed"
        );
        let a_to_r = edges
            .iter()
            .find(|e| e.from.node == NodeId(0))
            .expect("node0 -> reroute");
        assert_eq!(a_to_r.to.node, rid, "node0 now feeds the reroute");
        assert_eq!(a_to_r.to.port, 0, "into the reroute's only input");
        let r_to_b = edges
            .iter()
            .find(|e| e.from.node == rid)
            .expect("reroute -> node2");
        assert_eq!(r_to_b.to.node, NodeId(2), "the reroute feeds node2");
        assert_eq!(r_to_b.to.port, 0, "into the original input port");
        // One undo removes the whole reroute (node + both edges) and restores E0.
        assert_eq!(
            use_undo().undo_label().as_deref(),
            Some("Insert reroute"),
            "the splice is a single labelled step"
        );
        assert!(use_undo().undo(), "one undo reverts the splice");
        assert_eq!(coord.query("node_count"), Ok(IntrospectValue::Int(4)));
        assert_eq!(coord.query("edge_count"), Ok(IntrospectValue::Int(3)));
        assert!(
            coord.edges().iter().any(|e| e.id == EdgeId(0)),
            "the original edge is restored"
        );
        assert!(use_undo().redo(), "redo re-splices in one step");
        assert_eq!(coord.query("node_count"), Ok(IntrospectValue::Int(5)));
    });
}

#[test]
fn r1235_reroute_is_a_typed_passthrough_adopting_the_wire_type() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Build a FLOAT wire: Scalar (Float out, palette 5) -> Lerp's Float
        // factor input (palette 6, port 2).
        let scalar = coord.add_node(5).expect("Scalar");
        let lerp = coord.add_node(6).expect("Lerp");
        assert!(coord.add_edge(scalar, 0, lerp, 2), "Float -> Float wired");
        let float_edge = coord
            .edges()
            .iter()
            .copied()
            .find(|e| e.from.node == scalar)
            .expect("the float edge")
            .id;
        let rid = coord
            .add_reroute(float_edge)
            .expect("splice the float wire");
        let r = coord.node_by_id(rid).expect("reroute node");
        assert_eq!(r.title(), "Reroute", "titled Reroute");
        // R1596 — a reroute CARRIES the type it routes (`Reroute(PortType)`), so
        // its ports are declared by its own body and cannot disagree with it.
        let signature = signature_of(&coord.graph(), rid).expect("a signature");
        assert_eq!(
            signature
                .inputs
                .iter()
                .map(|p| p.value_type().copied())
                .collect::<Vec<_>>(),
            vec![Some(PortType::Float)],
            "the input port adopts the wire's type (not a hardcoded Vector)"
        );
        assert_eq!(
            signature
                .outputs
                .iter()
                .map(|p| p.value_type().copied())
                .collect::<Vec<_>>(),
            vec![Some(PortType::Float)],
            "the output port adopts the wire's type"
        );
        assert!(
            r.x >= 0 && r.y >= 0,
            "the reroute lands on the world surface"
        );
    });
}

#[test]
fn r1235_add_reroute_unknown_edge_is_none_and_verb_errors() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let n0 = coord.query("node_count");
        let e0 = coord.query("edge_count");
        // An unknown edge splices nothing (graph unchanged).
        assert_eq!(coord.add_reroute(EdgeId(99)), None, "unknown edge -> None");
        assert_eq!(coord.query("node_count"), n0, "node count unchanged");
        assert_eq!(coord.query("edge_count"), e0, "edge count unchanged");
        // The RPC verb: Null for unknown, a non-Int arg is a TypeMismatch.
        assert_eq!(
            coord.invoke("add_reroute", IntrospectValue::Int(99)),
            Ok(IntrospectValue::Null),
        );
        assert_eq!(
            coord.invoke("add_reroute", IntrospectValue::Text("x".to_owned())),
            Err(InvokeError::TypeMismatch),
        );
        // A real splice returns the new node id, and the verb is schema-declared.
        assert!(matches!(
            coord.invoke("add_reroute", IntrospectValue::Int(0)),
            Ok(IntrospectValue::Int(_))
        ));
        let fields: Vec<&str> = coord.schema().fields.iter().map(|f| f.path).collect();
        assert!(
            fields.contains(&"add_reroute"),
            "add_reroute schema-declared"
        );
    });
}

#[test]
fn r1236_dissolve_reroute_reconnects_the_wire() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Splice a reroute into edge 0 (node0 -> node2.in0), then dissolve it.
        let rid = coord.add_reroute(EdgeId(0)).expect("splice");
        assert_eq!(coord.query("node_count"), Ok(IntrospectValue::Int(5)));
        assert!(coord.dissolve_node(rid), "the reroute dissolves");
        // The reroute + its two edges are gone; the wire node0 -> node2 is bridged.
        assert_eq!(
            coord.query("node_count"),
            Ok(IntrospectValue::Int(4)),
            "the reroute node is removed"
        );
        assert_eq!(
            coord.query("edge_count"),
            Ok(IntrospectValue::Int(3)),
            "net -1 edge (removed 2, added 1 bridge)"
        );
        assert!(coord.node_by_id(rid).is_none(), "the reroute is gone");
        let bridged = coord.edges().iter().any(|e| {
            e.from.node == NodeId(0) && e.from.port == 0 && e.to.node == NodeId(2) && e.to.port == 0
        });
        assert!(bridged, "node0 -> node2.in0 is reconnected directly");
        // ONE undo restores the whole hop (the reroute + its two edges).
        assert_eq!(use_undo().undo_label().as_deref(), Some("Dissolve node"));
        assert!(use_undo().undo(), "one undo restores the hop");
        assert_eq!(coord.query("node_count"), Ok(IntrospectValue::Int(5)));
        assert!(coord.node_by_id(rid).is_some(), "the reroute is back");
    });
}

#[test]
fn r1236_dissolve_is_general_and_names_what_it_would_cut() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // ★R1596 — dissolve WIDENED. The editor required exactly one wire in
        // and one out and refused anything else; `Document::dissolve` (R1586)
        // is the general form — the DCC's `delete_reconnect` — so the question
        // worth asking is no longer *can it* but **does it lose anything**.
        //
        // node2 (Multiply) has two inputs and one output: it dissolves, and the
        // one downstream wire it cannot carry is NAMED rather than vanishing.
        // Measured, not predicted: a Multiply dissolves LOSSLESSLY. Its output
        // is routed from input 0, whose producer bridges straight to the
        // downstream consumer, so no wire is cut — what is lost is input 1's
        // CONTRIBUTION, which is not a severed link and `lossless` does not
        // claim otherwise. The first draft of this test asserted the opposite
        // from reasoning; the crate was right and the reasoning was not.
        assert!(
            coord.dissolvable(NodeId(2)),
            "a Multiply routes its output from input 0, so nothing is cut"
        );
        assert_eq!(
            coord.dissolve_severs(NodeId(2)),
            Some(String::new()),
            "and the cut list is empty"
        );
        assert!(coord.dissolve_node(NodeId(2)), "it dissolves");
        assert!(coord.node_by_id(NodeId(2)).is_none(), "the node is gone");
        assert!(use_undo().undo(), "one undo restores it");
        // node0 (Texture) is a source: nothing flows into it, so its output
        // has no input to be routed from and the wire leaving it is CUT —
        // named, where `node_internal_relink` deletes it and returns void so nothing in the DCC
        // can be asked what a reconnect-delete is about to throw away.
        assert!(
            !coord.dissolvable(NodeId(0)),
            "a source's dissolve is lossy"
        );
        assert_eq!(
            coord.dissolve_severs(NodeId(0)),
            Some("0".to_owned()),
            "and the wire it cuts is named"
        );
        // node3 (Output) is a sink: nothing flows past it, so dissolving it
        // cuts nothing and is simply a delete. The editor refused this because
        // its rule was a SHAPE test; losslessness is the honest question and the
        // answer here is yes.
        assert!(
            coord.dissolvable(NodeId(3)),
            "a sink's dissolve cuts nothing"
        );
        assert!(coord.dissolve_node(NodeId(3)), "so it dissolves");
        assert!(use_undo().undo(), "and one undo restores it");
        // Unknown id: there is no dissolve at all.
        assert!(
            !coord.dissolve_node(NodeId(99)),
            "unknown id -> no dissolve"
        );
        // The graph is back where it started.
        assert_eq!(coord.query("node_count"), Ok(IntrospectValue::Int(4)));
        assert_eq!(coord.query("edge_count"), Ok(IntrospectValue::Int(3)));
    });
}

#[test]
fn r1236_dissolve_selected_verb_and_gate() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice");
        // A lone reroute selection dissolves via the no-arg verb.
        coord.set_selection(Selection::single(rid));
        assert_eq!(
            coord.invoke("dissolve_selected", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true)),
            "dissolve_selected removes the lone reroute",
        );
        assert!(coord.node_by_id(rid).is_none(), "reroute dissolved");
        // With nothing selected, dissolve_selected is a no-op.
        coord.set_selection(Selection::None);
        assert_eq!(
            coord.invoke("dissolve_selected", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(false)),
            "no selection -> no-op",
        );
        // The by-id verb: Bool for a real/unknown id, non-Int is a TypeMismatch.
        assert_eq!(
            coord.invoke("dissolve_node", IntrospectValue::Int(99)),
            Ok(IntrospectValue::Bool(false)),
        );
        assert_eq!(
            coord.invoke("dissolve_node", IntrospectValue::Text("x".to_owned())),
            Err(InvokeError::TypeMismatch),
        );
        let fields: Vec<&str> = coord.schema().fields.iter().map(|f| f.path).collect();
        assert!(
            fields.contains(&"dissolve_node"),
            "dissolve_node schema-declared"
        );
        assert!(
            fields.contains(&"dissolve_selected"),
            "dissolve_selected schema-declared"
        );
    });
}

#[test]
fn r1236_alt_delete_dissolves_the_selected_node() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        // Splice a reroute into edge 0 and select it.
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            let IntrospectValue::Int(id) = intro
                .invoke("add_reroute", IntrospectValue::Int(0))
                .unwrap()
            else {
                panic!("add_reroute returns the new node id");
            };
            let _ = intro.intervene("selected", IntrospectValue::Int(id));
        }
        assert_eq!(
            graph_intro(&scene).query("node_count"),
            Ok(IntrospectValue::Int(5)),
            "the reroute was spliced in"
        );
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        // Alt+Delete DISSOLVES (delete + reconnect); plain Delete would just cut.
        assert!(NodeEditorView::apply_key(
            &mut scene,
            Some(GRAPH_TAG),
            "Delete",
            alt
        ));
        assert_eq!(
            graph_intro(&scene).query("node_count"),
            Ok(IntrospectValue::Int(4)),
            "the reroute node was removed"
        );
        assert_eq!(
            graph_intro(&scene).query("edge_count"),
            Ok(IntrospectValue::Int(3)),
            "the wire survived the removed hop (bridged, net -1 edge)"
        );
    });
}

#[test]
fn r1240_empty_frame_move_keeps_the_whole_rect_on_world() {
    Owner::new().run(|| {
        let (mut coord, id) = framed_pair();
        // Shrink the frame so it contains no nodes (an empty annotation).
        coord
            .intervene(
                &format!("frame.{}.w", id.0),
                IntrospectValue::Int(i64::from(FRAME_MIN)),
            )
            .unwrap();
        coord
            .intervene(
                &format!("frame.{}.h", id.0),
                IntrospectValue::Int(i64::from(FRAME_MIN)),
            )
            .unwrap();
        // R1596 — shrinking the box does NOT abandon what the frame holds; the
        // members are a relation, and `detach` is the act that ends it.
        coord.set_selection(Selection::Nodes(BTreeSet::from([NodeId(0), NodeId(1)])));
        assert_eq!(
            coord.detach_selected(),
            vec![NodeId(0), NodeId(1)],
            "detach takes them out"
        );
        assert_eq!(
            coord.query(&format!("frame.{}.contains", id.0)),
            Ok(IntrospectValue::Text(String::new())),
            "the frame now holds nothing"
        );
        // Push it far past the right / bottom: the whole RECT must stay on-world
        // (pre-R1240 only the origin was bounded, so the box slid fully off).
        coord
            .intervene(
                &format!("frame.{}.x", id.0),
                IntrospectValue::Int(1_000_000),
            )
            .unwrap();
        coord
            .intervene(
                &format!("frame.{}.y", id.0),
                IntrospectValue::Int(1_000_000),
            )
            .unwrap();
        let f = coord.frame_by_id(id).unwrap();
        assert!(f.x >= 0 && f.y >= 0, "origin on-world");
        assert!(
            f.x + f.width() <= WORLD,
            "the empty frame's right edge stays on-world"
        );
        assert!(
            f.y + f.height() <= WORLD,
            "the empty frame's bottom edge stays on-world"
        );
    });
}

#[test]
fn r1240_populated_frame_right_edge_stays_on_world() {
    Owner::new().run(|| {
        let (mut coord, id) = framed_pair();
        let rel = coord.frame_by_id(id).unwrap().x - coord.node_by_id(NodeId(0)).unwrap().x;
        coord
            .intervene(
                &format!("frame.{}.x", id.0),
                IntrospectValue::Int(1_000_000),
            )
            .unwrap();
        let f = coord.frame_by_id(id).unwrap();
        // The frame edge stops at the world (no FRAME_PAD overhang past WORLD)...
        assert!(
            f.x + f.width() <= WORLD,
            "the frame's right edge stays on-world"
        );
        // ...and the rigid group is preserved (members carried, still on-world).
        assert!(
            coord.node_by_id(NodeId(0)).unwrap().x <= WORLD - NODE_W,
            "member on-world"
        );
        assert_eq!(
            f.x - coord.node_by_id(NodeId(0)).unwrap().x,
            rel,
            "the frame->member offset is preserved (rigid)"
        );
    });
}

#[test]
fn r1241_dissolvable_query_matches_the_verb() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).unwrap();
        // R1596 — the read twin agrees with the verb, and what it answers is
        // LOSSLESSNESS: a reroute passes its one value through, a Multiply
        // cannot pass two through one output, a source has nothing upstream to
        // bridge from, and an unknown id has no dissolve at all.
        assert!(coord.dissolvable(rid), "the reroute loses nothing");
        assert!(
            coord.dissolvable(NodeId(2)),
            "nor does a Multiply — its output routes from input 0, so the wire \
             past it is bridged and nothing is cut"
        );
        assert!(
            !coord.dissolvable(NodeId(0)),
            "a source has no upstream to bridge from, so its outgoing wire dies"
        );
        assert!(!coord.dissolvable(NodeId(99)), "an unknown id is not");
        assert_eq!(
            coord.dissolve_severs(NodeId(99)),
            None,
            "and has no cut list either — absent is absent"
        );
        // The RPC reads mirror the method.
        assert_eq!(
            coord.query(&format!("dissolvable.{}", rid.0)),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(
            coord.query("dissolvable.0"),
            Ok(IntrospectValue::Bool(false)),
            "the source is the lossy one"
        );
        // Derived, not written down: the reroute splice retired the seed's
        // edge 0 and minted fresh ones, so the wire leaving the source is
        // whichever one currently does.
        let leaving = coord
            .edges()
            .iter()
            .find(|e| e.from.node == NodeId(0))
            .map(|e| e.id.0)
            .expect("the source is wired");
        assert_eq!(
            coord.query("dissolve_severs.0"),
            Ok(IntrospectValue::Text(leaving.to_string())),
            "and the read names the wire it would cut"
        );
        // Eligibility predicts the verb: dissolvable -> the verb succeeds -> gone.
        assert!(coord.dissolve_node(rid), "the verb agrees with the read");
        assert!(!coord.dissolvable(rid), "after dissolve the node is gone");
    });
}

#[test]
fn r1241_dissolve_rejects_a_self_loop_bridge() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Build a 2-cycle: Multiply m1 <-> Multiply m2 (each edge type-valid and
        // NOT a direct self-loop, so add_edge accepts both).
        let m1 = coord.add_node(2).unwrap();
        let m2 = coord.add_node(2).unwrap();
        assert!(coord.add_edge(m1, 0, m2, 0), "m1 -> m2");
        // ★R1596 — the cycle CANNOT BE AUTHORED. The editor's own gate blocked
        // only a DIRECT self-loop, so a two-hop cycle was constructible and the
        // dissolve had to defend against bridging one; `Document::connect`
        // refuses any wire that would close a cycle and names the path, so this
        // whole class is unreachable by editing and only a peer's blob can carry
        // one (which `validate` then catches).
        let (n0, e0) = (coord.query("node_count"), coord.query("edge_count"));
        assert!(
            !coord.add_edge(m2, 0, m1, 0),
            "the wire that would close the cycle is refused"
        );
        assert_eq!(coord.query("node_count"), n0, "graph unchanged");
        assert_eq!(coord.query("edge_count"), e0, "graph unchanged");
    });
}

#[test]
fn r1242_reroute_is_a_first_class_model_identity_not_a_title() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).unwrap();
        // The flag is set on the model, and the read twin reports it.
        assert!(
            coord.node_by_id(rid).unwrap().is_reroute(),
            "the model flag is set"
        );
        assert_eq!(
            coord.query(&format!("node.{}.is_reroute", rid.0)),
            Ok(IntrospectValue::Bool(true)),
        );
        // A seed op node is NOT a reroute...
        assert_eq!(
            coord.query("node.2.is_reroute"),
            Ok(IntrospectValue::Bool(false)),
        );
        // ...and the identity is NOT the title: renaming the knot keeps it a
        // reroute, and renaming an op node "Reroute" does NOT make it one.
        coord
            .intervene(
                &format!("node.{}.title", rid.0),
                IntrospectValue::Text("knot".to_owned()),
            )
            .unwrap();
        assert!(
            coord.node_by_id(rid).unwrap().is_reroute(),
            "renamed knot stays a reroute"
        );
        coord
            .intervene("node.2.title", IntrospectValue::Text("Reroute".to_owned()))
            .unwrap();
        assert!(
            !coord.node_by_id(NodeId(2)).unwrap().is_reroute(),
            "a node named Reroute is not one"
        );
        // The enumeration finds exactly the reroute.
        assert_eq!(
            coord.query("reroute_ids"),
            Ok(IntrospectValue::Text(rid.0.to_string())),
        );
    });
}

#[test]
fn r1242_reroute_identity_survives_serialize_reload() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).unwrap();
        let json = coord.serialized_json();
        // Wipe + reload from the blob; the reroute identity round-trips.
        coord.document.set(Graph::new("material"));
        assert!(coord.load_json(&json), "reload the snapshot");
        assert!(
            coord.node_by_id(rid).unwrap().is_reroute(),
            "the reroute flag persisted through serialize/reload (not just the title)"
        );
        assert_eq!(
            coord.query("reroute_ids"),
            Ok(IntrospectValue::Text(rid.0.to_string())),
        );
    });
}

// ── R1243 — reroute knot RENDER + double-click-on-wire splice ──────────

/// R1243 — the compact-knot render: a spliced reroute paints as a
/// `KNOT_SIZE` dot, while an op node keeps its full `NODE_W` card. The
/// snapshot proof the audit deferred (the reroute was a full "Reroute" card
/// through R1242).
#[test]
fn r1243_reroute_paints_as_a_compact_knot_not_a_card() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice edge 0");
        // The accessors are the SSOT the paint + bounds both read.
        let knot = coord.node_by_id(rid).expect("the reroute");
        assert_eq!(knot.width(), KNOT_SIZE, "the knot is KNOT_SIZE wide");
        assert_eq!(knot.height(), KNOT_SIZE, "the knot is KNOT_SIZE tall");
        assert_eq!(
            knot.right(),
            knot.x + KNOT_SIZE,
            "its extent is the dot, not NODE_W"
        );
        // An op node is unchanged — still a full card.
        let card = coord.node_by_id(NodeId(0)).expect("Texture");
        assert_eq!(card.width(), NODE_W, "an op node stays a full-width card");
        assert!(card.height() > KNOT_SIZE, "and a full-height card");
        // The rendered scene: `view_node` dispatches the reroute to a compact
        // KNOT_SIZE container (tagged `#node_{id}` like any node), while an op
        // node renders its full NODE_W card — the paint mirrors the model.
        let theme = use_theme(THEME_TAG).theme_animated();
        let no_wired: BTreeSet<usize> = BTreeSet::new();
        let graph = coord.graph();
        let knot_scene = view_node(
            &graph,
            &knot,
            false,
            CardEdit {
                target: None,
                field: IDLE_TF,
            },
            &no_wired,
            &theme,
            1.0,
        );
        assert_eq!(
            knot_scene.tag(),
            Some(format!("{GRAPH_TAG}#node_{}", rid.0).as_str()),
            "the knot keeps the node tag (so it selects / drags like any node)",
        );
        let Scene::Container(knot_box) = &knot_scene else {
            panic!("the knot renders as a Container, got {knot_scene:?}");
        };
        assert_eq!(
            knot_box.layout.size,
            Size::px(upx(KNOT_SIZE), upx(KNOT_SIZE)),
            "the reroute paints as a compact KNOT_SIZE dot",
        );
        assert_eq!(
            knot_box.style.corner_radius,
            upx(KNOT_SIZE) / 2,
            "a half-diameter radius rounds the square into a dot",
        );
        assert!(
            knot_box.children.is_empty(),
            "a knot has no header / port rows"
        );
        let card_scene = view_node(
            &graph,
            &card,
            false,
            CardEdit {
                target: None,
                field: IDLE_TF,
            },
            &no_wired,
            &theme,
            1.0,
        );
        let Scene::Container(card_box) = &card_scene else {
            panic!("an op node renders as a Container, got {card_scene:?}");
        };
        assert_eq!(
            card_box.layout.size,
            Size::px(upx(NODE_W), upx(card.height())),
            "an op node still paints a full NODE_W card",
        );
        assert!(
            !card_box.children.is_empty(),
            "a card has header + port rows"
        );
    });
}

/// R1243 — a knot has no port rows: every incident wire anchors at its
/// centre (`knot_center`), so `input_port_center` == `output_port_center` ==
/// the dot centre. A wire therefore terminates on the dot, not on a phantom
/// full-card port row (which is what R1235's shared-path render did wrong).
#[test]
fn r1243_knot_ports_anchor_at_its_centre() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice");
        let knot = coord.node_by_id(rid).expect("the reroute");
        let centre = (knot.x + KNOT_SIZE / 2, knot.y + KNOT_SIZE / 2);
        assert_eq!(
            knot_center(&knot),
            centre,
            "the dot centre is the geometric centre"
        );
        assert_eq!(
            input_port_center(&knot, 0),
            centre,
            "the input anchors at the centre"
        );
        assert_eq!(
            output_port_center(&knot, 0),
            centre,
            "the output anchors at the centre"
        );
        assert_eq!(
            input_port_center(&knot, 0),
            output_port_center(&knot, 0),
            "both ports coincide — the wire passes straight through the dot",
        );
        // Contrast: an op node's ports are on its LEFT / RIGHT edges, never
        // coincident (the branch is a real behaviour change, not a no-op).
        let card = coord.node_by_id(NodeId(2)).expect("Multiply");
        assert_ne!(
            input_port_center(&card, 0),
            output_port_center(&card, 0),
            "a card's input and output ports are on opposite edges",
        );
    });
}

/// R1243 — `add_reroute` centres the compact knot on the wire midpoint
/// (`mid - KNOT_SIZE/2`), not on a phantom `NODE_W/2` card half — so the dot
/// sits exactly on the double-click point.
#[test]
fn r1243_add_reroute_centres_the_knot_on_the_wire_midpoint() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let nodes = default_nodes();
        let from = output_port_center(&nodes[0], 0);
        let to = input_port_center(&nodes[2], 0);
        let straight_mid = (i32::midpoint(from.0, to.0), i32::midpoint(from.1, to.1));
        let coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice");
        let knot = coord.node_by_id(rid).expect("the reroute");
        assert_eq!(
            knot.x,
            clamp_node_x(straight_mid.0 - KNOT_SIZE / 2),
            "knot x centres the dot (KNOT_SIZE/2), not the card (NODE_W/2)",
        );
        assert_eq!(
            knot.y,
            clamp_node_y(straight_mid.1 - KNOT_SIZE / 2),
            "knot y centres the dot",
        );
    });
}

/// R1243 — the headline gesture: a double-click ON a wire splices a reroute
/// knot into it (the live twin of `invoke add_reroute`). The press carrying
/// the `DoubleClick` seeds the background edge-hit probe, so the arm splices
/// the wire under the cursor; the trailing release must not re-select the
/// now-removed edge (the consumed-probe invariant).
#[test]
fn r1243_double_click_on_a_wire_splices_a_reroute() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let nodes = default_nodes();
        // Edge 0 = Texture.out0 -> Multiply.in0 — its bezier midpoint sits in
        // open space (the same wire r839 hit-tests).
        let mid = edge_mid(
            output_port_center(&nodes[0], 0),
            input_port_center(&nodes[2], 0),
        );
        assert_eq!(query_int(&scene, "node_count"), 4, "4 seed nodes");
        assert_eq!(query_int(&scene, "edge_count"), 3, "3 seed edges");
        // Background press, capture-seed move onto the wire (arms the probe),
        // the DoubleClick (arms the splice), then the in-place release fires it.
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, mid.0, mid.1);
        send(&mut scene, "DoubleClick");
        assert_eq!(
            query_int(&scene, "node_count"),
            4,
            "the splice is deferred to the release"
        );
        send(&mut scene, "PointerUp");
        assert_eq!(
            query_int(&scene, "node_count"),
            5,
            "the in-place release splices the knot"
        );
        assert_eq!(
            query_int(&scene, "edge_count"),
            4,
            "net +1 edge (removed 1, added 2)"
        );
        // The new node is a reroute, and the original edge is gone.
        let reroute_ids = match graph_intro(&scene).query("reroute_ids") {
            Ok(IntrospectValue::Text(t)) => t,
            other => panic!("expected Text at reroute_ids, got {other:?}"),
        };
        assert_eq!(reroute_ids, "4", "the double-click minted reroute node 4");
        assert!(
            matches!(
                graph_intro(&scene).query("edge.0"),
                Err(ReadRefusal::NoSuchMember(_))
            ),
            "the double-clicked edge 0 was removed"
        );
        // The in-place release that spliced also left the NEW KNOT selected
        // (not a stale edge, not the fresh A->R wire under the cursor).
        assert_eq!(
            graph_intro(&scene).query("selected_edge"),
            Ok(IntrospectValue::Null),
            "no edge is selected after the splice",
        );
        assert_eq!(
            graph_intro(&scene).query("selected"),
            Ok(IntrospectValue::Int(4)),
            "the new reroute knot is the selection",
        );
    });
}

/// R1243 — a double-click that turns into a DRAG (a marquee begun on a wire,
/// e.g. a click immediately followed by a marquee in the RPC drain's tight
/// double-click window) marquees instead of splicing: the splice is armed on
/// the DoubleClick but only fires on an IN-PLACE release, and a moved gesture
/// routes to the marquee. Regression guard for the r880 marquee interaction.
#[test]
fn r1243_double_click_then_drag_marquees_instead_of_splicing() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let nodes = default_nodes();
        let mid = edge_mid(
            output_port_center(&nodes[0], 0),
            input_port_center(&nodes[2], 0),
        );
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, mid.0, mid.1); // anchor + seed the edge probe
        send(&mut scene, "DoubleClick"); // ARMS a splice (does not fire it)
        // Now drag far away — the marquee latch goes live past the dead zone.
        bg_move(&mut scene, mid.0 + 240.0, mid.1 - 90.0);
        send(&mut scene, "PointerUp");
        assert_eq!(
            query_int(&scene, "node_count"),
            4,
            "a moved gesture marquees — an armed splice never fires on a drag",
        );
        assert_eq!(
            graph_intro(&scene).query("reroute_ids"),
            Ok(IntrospectValue::Text(String::new())),
            "no reroute was spliced by the dragged double-click",
        );
    });
}

/// R1243 — a double-click on EMPTY canvas seeds no edge hit, so it splices
/// nothing (the negative twin of the wire double-click).
#[test]
fn r1243_double_click_on_empty_canvas_is_inert() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        assert_eq!(query_int(&scene, "node_count"), 4);
        send(&mut scene, "PointerDown");
        bg_move(&mut scene, 8.0, 8.0); // an empty corner — hit_test_edge == None
        send(&mut scene, "DoubleClick");
        send(&mut scene, "PointerUp");
        assert_eq!(
            query_int(&scene, "node_count"),
            4,
            "a double-click off any wire splices nothing",
        );
    });
}

/// R1243 — the `width()` SSOT flows through the distribute centre key and the
/// frame-membership centre: a reroute knot is measured by its dot, not a
/// phantom `NODE_W` (the horizontal twin of the already-`height()`-aware
/// vertical axis).
#[test]
fn r1243_reroute_width_flows_through_centre_key_and_contains() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice");
        let knot = coord.node_by_id(rid).expect("the reroute");
        assert_eq!(
            centre_key(&knot, DistributeAxis::Horizontal),
            2 * knot.x + KNOT_SIZE,
            "the horizontal distribute key uses the knot width, not NODE_W",
        );
        // A frame tightly around the knot's dot contains it (its centre sits
        // inside), proving `contains_node` measures the dot, not a phantom card.
        // R1596 — a frame is a NODE, so a fixture builds one the way the editor
        // does: an empty-signature body whose extent is authored.
        let mut probe = coord.graph();
        let fid = probe
            .add_node(TREE, NodeBody::Frame, knot.x - 2, knot.y - 2)
            .expect("root tree");
        if let Some(node) = probe.tree_mut(TREE).and_then(|t| t.node_mut(fid)) {
            node.appearance.width = Some(upx(KNOT_SIZE + 4));
            node.appearance.height = Some(upx(KNOT_SIZE + 4));
        }
        let tight = frame_node(&probe, fid).expect("the frame").clone();
        assert!(
            frame_contains(&tight, &knot),
            "the dot's centre is inside a tight frame"
        );
    });
}

// ── R1246 — knot double-click is a no-op; begin_edit(Card) refuses knots ──

/// R1246 — a double-click on a reroute KNOT is a NO-OP: it neither dissolves
/// (R1245's invented footgun gesture, reverted) nor arms a rename (`begin_edit`
/// refuses the card edit — a knot paints no card). Dissolve stays on the
/// standard `Alt`+`Delete` / `invoke dissolve_node`.
#[test]
fn r1246_double_click_a_knot_is_a_noop() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let rid = coordinator().add_reroute(EdgeId(0)).expect("splice");
        assert_eq!(query_int(&scene, "node_count"), 5, "the knot is spliced in");
        send(&mut scene, &format!("node_{}:DoubleClick", rid.0));
        assert_eq!(
            query_int(&scene, "node_count"),
            5,
            "the knot double-click dissolves nothing",
        );
        assert_eq!(
            graph_intro(&scene).query("reroute_ids"),
            Ok(IntrospectValue::Text(rid.0.to_string())),
            "the knot is still there",
        );
        assert_eq!(
            use_active_edit().get(),
            None,
            "and arms no (unpainted) rename editor",
        );
    });
}

/// R1246 — the paint==a11y ROOT fix (the R1243 latent bug R1245 only
/// half-cleared): `begin_rename` on a knot is refused via EVERY entry point —
/// direct (double-click), the RPC verb by id, and the `Null` = selection form
/// the F2 key drives — because `begin_edit` refuses a CARD-surface edit on a
/// reroute. Pre-R1246 F2 + `invoke begin_rename` armed an unpainted a11y textbox.
#[test]
fn r1246_begin_rename_on_a_knot_is_refused_every_route() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice"); // the knot ends selected
        assert!(
            !coord.begin_rename(rid),
            "direct begin_rename refuses a knot"
        );
        assert_eq!(
            coord.invoke("begin_rename", IntrospectValue::Int(i64::from(rid.0))),
            Ok(IntrospectValue::Bool(false)),
            "invoke begin_rename <knot> is refused",
        );
        assert_eq!(
            coord.invoke("begin_rename", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(false)),
            "F2 (begin_rename Null) on the selected knot is refused",
        );
        assert_eq!(
            use_active_edit().get(),
            None,
            "no card editor armed on the knot by any route",
        );
        assert!(
            coord.begin_rename(NodeId(2)),
            "a compute node still renames"
        );
    });
}

/// R1246 — the port-default variant: even an UNWIRED knot pin (its `A->R` edge
/// cut) refuses a card default editor. R901.1's wired-guard covered the normal
/// case (a knot's input is wired); the R1246 reroute gate closes the cut case's
/// RPC `begin_edit_default` route too.
#[test]
fn r1246_begin_edit_default_on_an_unwired_knot_pin_is_refused() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice");
        let in_edge = coord
            .edges()
            .iter()
            .find(|e| e.to.node == rid)
            .expect("A->R edge")
            .id;
        assert!(coord.remove_edge(in_edge), "cut the knot's input wire");
        assert!(
            !coord.begin_edit_default(rid, 0),
            "a knot's card pin default is refused even when unwired",
        );
        assert_eq!(
            use_active_edit().get(),
            None,
            "no unpainted port editor armed"
        );
    });
}

/// R1246 — the standard dissolve path is UNCHANGED by the double-click revert:
/// `invoke dissolve_node` still removes a knot and reconnects the wire.
#[test]
fn r1246_dissolve_verb_still_removes_a_knot() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice");
        assert!(coord.dissolve_node(rid), "the dissolve verb still works");
        assert_eq!(
            coord.query("node_count"),
            Ok(IntrospectValue::Int(4)),
            "the knot is gone",
        );
        let bridged = coord.edges().iter().any(|e| {
            e.from.node == NodeId(0) && e.from.port == 0 && e.to.node == NodeId(2) && e.to.port == 0
        });
        assert!(bridged, "node0 -> node2.in0 is reconnected");
    });
}

/// R1246 — a COMPUTE node's double-click still opens its title rename (R878
/// unchanged); only reroute knots are refused.
#[test]
fn r1246_double_click_a_compute_node_still_renames() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        send(&mut scene, "node_2:PointerDown");
        send(&mut scene, "node_2:DoubleClick");
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::Title(NodeId(2))),
            "a compute node's double-click still renames its title",
        );
        assert_eq!(
            query_int(&scene, "node_count"),
            4,
            "renaming a compute node dissolves nothing",
        );
    });
}

/// R1248 — width() SSOT completeness (the missed pre-existing peer): the
/// `open_pin_create` spawn point routes through `right()`/`width()`, so opening
/// a create menu from a KNOT's output pin (`invoke open_pin_create "<knot>.0"`)
/// lands the new node at the knot's compact DOT right edge + gap, not ~112px off
/// as if the 18px knot were a 130px card (the pre-R1248 raw-`NODE_W` bug).
#[test]
fn r1248_open_pin_create_on_a_knot_uses_its_dot_right_edge() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        let rid = coord.add_reroute(EdgeId(0)).expect("splice"); // a Vector knot
        let knot = coord.node_by_id(rid).expect("the knot");
        assert_eq!(
            coord.invoke(
                "open_pin_create",
                IntrospectValue::Text(format!("{}.0", rid.0)),
            ),
            Ok(IntrospectValue::Bool(true)),
            "the create menu opens from a knot's typed output pin",
        );
        let new_id = coord
            .commit_pin_create_highlighted()
            .expect("committed a node");
        let new_node = coord.node_by_id(new_id).expect("the created node");
        assert_eq!(
            new_node.x,
            clamp_node_x(knot.right() + PIN_CREATE_GAP),
            "the node lands at the knot's DOT right edge + gap (width()-routed)",
        );
        assert_ne!(
            new_node.x,
            clamp_node_x(knot.x + NODE_W + PIN_CREATE_GAP),
            "NOT placed ~112px off as if the 18px knot were a 130px card",
        );
    });
}

// ── R1255 — dataflow evaluation (the Phase-C entry) ─────────────────────────

#[test]
fn r1255_colour_arithmetic_is_component_wise_and_clamped() {
    // Add saturates each channel at 255 (200+100, 100+200, 0+50).
    assert_eq!(
        color_add(Color::rgb(200, 100, 0), Color::rgb(100, 200, 50)),
        Color::rgb(255, 255, 50),
    );
    // Multiply blend is `a·b/255` per channel (the 0..255 multiply).
    assert_eq!(
        color_mul(Color::rgb(255, 128, 0), Color::rgb(128, 255, 255)),
        Color::rgb(128, 128, 0),
    );
    // Lerp hits its endpoints exactly and rounds the midpoint (127.5 -> 128);
    // a factor outside 0..=1 clamps (no extrapolation).
    let (black, white) = (Color::rgb(0, 0, 0), Color::rgb(255, 255, 255));
    assert_eq!(color_lerp(black, white, 0.0), black, "t=0 -> a");
    assert_eq!(color_lerp(black, white, 1.0), white, "t=1 -> b");
    assert_eq!(
        color_lerp(black, white, 0.5),
        Color::rgb(128, 128, 128),
        "t=0.5 midpoint"
    );
    assert_eq!(color_lerp(black, white, 2.0), white, "t>1 clamps to b");
    // Scalar broadcast treats the Float as a normalized 0..=1 channel.
    assert_eq!(broadcast_scalar(0.0), black, "0.0 -> black");
    assert_eq!(broadcast_scalar(1.0), white, "1.0 -> white");
    assert_eq!(
        broadcast_scalar(0.5),
        Color::rgb(128, 128, 128),
        "0.5 -> mid-grey"
    );
    assert_eq!(broadcast_scalar(2.0), white, "clamps above 1.0");
}

#[test]
fn r1255_float_broadcasts_into_a_vector_input_but_never_narrows() {
    // R1593 / R1596 — the coercion IS the relation now
    // (`NodeKind::conversion`), so whether a wire is legal and what arrives
    // along it are one declaration instead of two that could disagree.
    let cross = |value: CellValue, from, to| {
        MaterialOp::conversion(&from, &to)
            .apply(value)
            .expect("a permitted crossing yields a value")
    };
    // The only coercion the lattice permits: a scalar Float promotes to a Vector.
    assert_eq!(
        cross(CellValue::Float(0.5), PortType::Float, PortType::Vector),
        CellValue::Color(Color::rgb(128, 128, 128)),
    );
    // An exact-type value passes through unchanged (no coercion).
    let c = CellValue::Color(Color::rgb(10, 20, 30));
    assert_eq!(
        cross(c.clone(), PortType::Vector, PortType::Vector),
        c,
        "an exact match is Direct"
    );
    assert_eq!(
        cross(CellValue::Float(0.25), PortType::Float, PortType::Float),
        CellValue::Float(0.25),
    );
    // And the asymmetry: a Vector never narrows back.
    assert!(
        !MaterialOp::conversion(&PortType::Vector, &PortType::Float).is_allowed(),
        "no narrowing — the relation is directed, which is why it is ONE \
         declaration and not an equality"
    );
}

#[test]
fn r1255_source_ops_yield_their_port_type_constant() {
    // R1596 — a source COMPUTES nothing: its output is the value authored on
    // its own port, else the kind's own resting value, and the crate supplies
    // the fallback where `evaluate` leaves the hole. So the kind answers
    // `vec![None]` here, and the graph answers the constant.
    for op in [MaterialOp::Texture, MaterialOp::Color, MaterialOp::Scalar] {
        assert_eq!(op.evaluate(&[]), vec![None], "a source computes nothing");
    }
    let graph = graph_of(&[(0, 0, 0), (1, 0, 0), (5, 0, 0)], &[]);
    assert_eq!(
        evaluate(&graph, NodeId(0)),
        Some(PortType::Vector.default_value()),
        "Texture rests at its Vector default"
    );
    assert_eq!(
        evaluate(&graph, NodeId(1)),
        Some(PortType::Vector.default_value()),
    );
    assert_eq!(evaluate(&graph, NodeId(2)), Some(CellValue::Float(0.0)));
}

#[test]
fn r1255_add_evaluates_over_its_pin_defaults_when_unconnected() {
    // An Add node with both inputs UNCONNECTED evaluates its authored pin
    // defaults (the R899 substrate drives the compute) — red + green = yellow.
    let graph = graph_with(
        &[(3, 0, 0)],
        &[],
        &[
            (
                0,
                PortRef::input(0),
                CellValue::Color(Color::rgb(200, 0, 0)),
            ),
            (
                0,
                PortRef::input(1),
                CellValue::Color(Color::rgb(0, 200, 0)),
            ),
        ],
    );
    assert_eq!(
        evaluate(&graph, NodeId(0)),
        Some(CellValue::Color(Color::rgb(200, 200, 0))),
    );
}

#[test]
fn r1255_a_wired_source_propagates_and_overrides_the_pin_default() {
    // Color source (grey) -> Add.in0; Add.in1 keeps a red pin default.
    let graph = graph_with(
        &[(1, 0, 0), (3, 0, 0)],
        &[(0, 0, 1, 0)],
        &[
            // hidden: in0 is wired
            (
                1,
                PortRef::input(0),
                CellValue::Color(Color::rgb(255, 255, 255)),
            ),
            (1, PortRef::input(1), CellValue::Color(Color::rgb(90, 0, 0))),
        ],
    );
    // in0 = the wired grey (NOT its 255 default), in1 = its red default.
    let grey = 0x80;
    assert_eq!(
        evaluate(&graph, NodeId(1)),
        Some(CellValue::Color(Color::rgb(grey + 90, grey, grey))),
    );
}

#[test]
fn r1255_output_sink_reports_the_value_flowing_into_it() {
    // Multiply(grey, grey) -> Output; the sink's "value" is its resolved input.
    let graph = graph_of(
        &[(1, 0, 0), (1, 0, 0), (2, 0, 0), (4, 0, 0)],
        &[(0, 0, 2, 0), (1, 0, 2, 1), (2, 0, 3, 0)],
    );
    let expected = CellValue::Color(color_mul(
        Color::rgb(0x80, 0x80, 0x80),
        Color::rgb(0x80, 0x80, 0x80),
    ));
    assert_eq!(
        evaluate(&graph, NodeId(3)),
        Some(expected.clone()),
        "Output.value = its input"
    );
    assert_eq!(
        eval_terminal(&graph),
        Some(expected),
        "eval.output = the Output sink's input"
    );
}

#[test]
fn r1255_a_cycle_is_uncomputable_and_detected() {
    // A -> B -> A: both nodes sit on a cycle, so neither evaluates.
    // R1596 — `connect` REFUSES the wire that closes the loop, so a cycle only
    // ever arrives from a peer. That is a capability the editor's own model did
    // not have (it accepted the wire and detected the cycle afterwards), and it
    // is why this fixture goes through the wire form.
    let mut graph = graph_of(&[(3, 0, 0), (3, 0, 0)], &[(0, 0, 1, 0)]);
    assert!(
        graph
            .connect(TREE, Socket::new(NodeId(1), 0), Socket::new(NodeId(0), 0))
            .is_err(),
        "the closing wire is refused up front"
    );
    graph = force_cycle(&graph, NodeId(1), NodeId(0));
    assert_eq!(evaluate(&graph, NodeId(0)), None, "a cycle node is None");
    assert!(
        !graph.cycle_nodes(TREE).is_empty(),
        "the graph is not a DAG"
    );
    // The seed graph, by contrast, is a DAG.
    assert!(
        default_graph().cycle_nodes(TREE).is_empty(),
        "the seed graph is acyclic"
    );
}

#[test]
fn r1255_query_reads_the_evaluated_graph() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        let grey = Color::rgb(0x80, 0x80, 0x80);
        let terminal = CellValue::Color(color_mul(grey, grey)).to_introspect();
        // Output (node 3) reports its resolved input = the Multiply result.
        assert_eq!(
            intro.query("node.3.value"),
            Ok(terminal.clone()),
            "node.3.value"
        );
        assert_eq!(
            intro.query("eval.output"),
            Ok(terminal),
            "eval.output = terminal"
        );
        assert_eq!(
            intro.query("eval.acyclic"),
            Ok(IntrospectValue::Bool(true)),
            "seed graph is a DAG"
        );
        // A source (Texture, node 0) reports its Vector constant.
        assert_eq!(
            intro.query("node.0.value"),
            Ok(PortType::Vector.default_value().to_introspect()),
            "a source reads its constant",
        );
    });
}

#[test]
fn r1255_a_wired_port_ignores_its_retained_pin_default() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        let before = intro.query("node.2.value"); // Multiply, both inputs wired
        // Intervene Multiply.in0's (retained-but-hidden) pin default. The wire,
        // not the default, feeds a connected port, so the value is unchanged.
        assert!(
            intro
                .intervene(
                    "node.2.input_default.0",
                    IntrospectValue::Text("#ff0000".to_owned())
                )
                .is_ok(),
            "the write lands on the retained default",
        );
        assert_eq!(
            intro.query("node.2.value"),
            before,
            "a wired port uses the wire, not the default"
        );
    });
}

#[test]
fn r1255_detail_value_mirrors_the_selected_node() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        intro
            .intervene("selected_ids", IntrospectValue::Text("3".to_owned()))
            .expect("select the Output node");
        assert_eq!(
            intro.query("detail.value"),
            intro.query("node.3.value"),
            "detail.value is the selection-relative alias",
        );
    });
}

// ── R1256 — audit-clearance of R1255 (op read / derived is_reroute /
//    from_palette SSOT / symmetric default coercion) ─────────────────────────

#[test]
fn r1256_node_op_read_distinguishes_same_signature_ops() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // The seed nodes: Texture and Color share ()->Vector; the structural
        // reads cannot tell them apart, but `op` can.
        assert_eq!(
            intro.query("node.0.op"),
            Ok(IntrospectValue::Text("Texture".into()))
        );
        assert_eq!(
            intro.query("node.1.op"),
            Ok(IntrospectValue::Text("Color".into()))
        );
        assert_eq!(
            intro.query("node.0.input_types"),
            intro.query("node.1.input_types"),
            "Texture and Color are structurally identical...",
        );
        assert_ne!(
            intro.query("node.0.op"),
            intro.query("node.1.op"),
            "...but `op` separates them"
        );
        // Multiply (node 2) vs a fresh Add: identical (Vector,Vector)->Vector.
        assert_eq!(
            intro.query("node.2.op"),
            Ok(IntrospectValue::Text("Multiply".into()))
        );
        let add = intro
            .invoke("add_node", IntrospectValue::Text("Add".into()))
            .expect("add");
        let IntrospectValue::Int(add_id) = add else {
            panic!("add id")
        };
        assert_eq!(
            intro.query(&format!("node.{add_id}.op")),
            Ok(IntrospectValue::Text("Add".into())),
            "the new node reads op=Add (not derivable from its signature)",
        );
        // detail.op mirrors the selected node.
        intro
            .intervene("selected_ids", IntrospectValue::Text("2".into()))
            .expect("select");
        assert_eq!(
            intro.query("detail.op"),
            Ok(IntrospectValue::Text("Multiply".into()))
        );
    });
}

#[test]
fn r1256_is_reroute_is_derived_from_op_at_construction() {
    // R1259 — `is_reroute()` is a pure derivation of the node's BODY (no stored
    // field to drift), keyed off the compute identity NOT the title. A Reroute
    // node is a knot; any compute op is not.
    //
    // R1596 — a reroute now carries the type it routes as a payload, so a knot
    // cannot disagree with its own ports; and a rename is `Node::label`, which
    // `display_name` prefers and nothing else reads.
    let mut graph = Graph::new("fixture");
    let knot = graph
        .add_node(
            TREE,
            NodeBody::Kind(MaterialOp::Reroute(PortType::Vector)),
            0,
            0,
        )
        .expect("root tree");
    let add = graph
        .add_node(TREE, NodeBody::Kind(MaterialOp::Add), 0, 0)
        .expect("root tree");
    for (id, label) in [(knot, "renamed"), (add, "Reroute")] {
        graph
            .tree_mut(TREE)
            .and_then(|t| t.node_mut(id))
            .expect("a fresh node")
            .label = Some(label.to_owned());
    }
    let node = |id: NodeId| kind_node(&graph, id).expect("a kind node").0;
    assert!(
        node(knot).is_reroute(),
        "a Reroute body derives is_reroute=true (even with a non-'Reroute' title)"
    );
    assert!(
        !node(add).is_reroute(),
        "an Add body derives is_reroute=false (even titled 'Reroute')"
    );
}

#[test]
fn r1256_from_palette_matches_the_palette_ssot() {
    // R1596 — the palette carries `(title, op)` and NOTHING ELSE: a kind
    // declares its own ports, so the port lists this row used to carry (and
    // `node_invariants_hold` used to re-check against every node) cannot drift
    // because there is nowhere for a second copy to live.
    let mut graph = Graph::new("fixture");
    for (kind, &(title, op)) in PALETTE.iter().enumerate() {
        let id = add_palette_node(&mut graph, kind, 5, 6).expect("in-range kind");
        let (node, made) = kind_node(&graph, id).expect("a kind node");
        assert_eq!(node.title(), title, "title from PALETTE");
        assert_eq!(made, op, "op from PALETTE (no drift)");
        let signature = signature_of(&graph, id).expect("a signature");
        assert_eq!(signature.inputs.len(), op.inputs().len(), "inputs from op");
        assert_eq!(
            signature.outputs.len(),
            op.outputs().len(),
            "outputs from op"
        );
    }
    assert!(
        add_palette_node(&mut graph, PALETTE.len(), 0, 0).is_none(),
        "out-of-range kind = None"
    );
    // The seed graph is the palette SSOT too — its op/title cannot drift.
    let seed = default_graph();
    assert_eq!(
        kind_nodes(&seed).map(|(_, op)| op).collect::<Vec<_>>(),
        vec![
            MaterialOp::Texture,
            MaterialOp::Color,
            MaterialOp::Multiply,
            MaterialOp::Output
        ]
    );
}

#[test]
fn r1256_a_mistyped_default_coerces_instead_of_evaluating_null() {
    // A `set_graph`/loaded blob can carry a Float default on a Vector input port
    // (the interactive path constructs matching defaults, but nothing validates
    // an injected graph). R1256 coerces the default branch symmetrically with
    // the wired branch, so the Float broadcasts instead of yielding a silent null.
    // R1596 — `set_port_value` REFUSES a Float on a Vector port, so this graph
    // is no longer constructible by editing; it is exactly the blob a peer can
    // still send. The coercion under test is the same one either way.
    let mut graph = graph_with(
        &[(3, 0, 0)],
        &[],
        &[(
            0,
            PortRef::input(1),
            CellValue::Color(Color::rgb(0, 0, 0)), // black
        )],
    );
    assert!(
        graph
            .set_port_value(TREE, NodeId(0), PortRef::input(0), CellValue::Float(1.0))
            .is_err(),
        "a Float cannot be AUTHORED onto a Vector port"
    );
    // ★R1596/R1597 — the blob that CAN carry one is REJECTED rather than
    // silently repaired. R1256 made the editor coerce a mistyped default so it
    // would not evaluate to a null; migrating onto the crate showed that the
    // right answer is neither — a document whose authored value its port cannot
    // hold is a document that came from somewhere untrusted, and the load gate
    // says so. Coercing would be the model quietly rewriting what a peer sent.
    graph
        .tree_mut(TREE)
        .and_then(|t| t.node_mut(NodeId(0)))
        .expect("the Add")
        .values
        .insert(PortRef::input(0), CellValue::Float(1.0));
    assert!(
        !graph_invariants_hold(&graph),
        "the mistyped value makes the document invalid"
    );
    assert!(
        graph
            .validate()
            .iter()
            .any(|v| matches!(v, Violation::MistypedPortValue { .. })),
        "named as a mistyped port value: {:?}",
        graph.validate()
    );
    assert_eq!(
        evaluate(&graph, NodeId(0)),
        None,
        "and it evaluates to nothing — which is why the load gate must catch it",
    );
}

// ── R1257 — authorable source constants (the output-side twin of R899) ───────

#[test]
fn r1257_sources_carry_an_authorable_constant_others_do_not() {
    // Texture / Color / Scalar (no inputs, >=1 output) are sources; ops / sink /
    // reroute are not. A source's constant seeds to output port 0's type default.
    // R1596 — `is_source` is DERIVED from the signature (no inputs, at least one
    // output) where the editor stored an `Option<CellValue>` whose Some-ness was
    // the discriminator and whose agreement with the port list an invariant
    // predicate had to re-check.
    let mut graph = Graph::new("fixture");
    for kind in 0..PALETTE.len() {
        let id = add_palette_node(&mut graph, kind, 0, 0).expect("in-range kind");
        let signature = signature_of(&graph, id).expect("a signature");
        let (no_inputs, has_output) = (signature.inputs.is_empty(), !signature.outputs.is_empty());
        let node = kind_node(&graph, id).expect("a kind node").0.clone();
        assert_eq!(
            is_source(&graph, id),
            no_inputs && has_output,
            "{} is_source",
            node.title(),
        );
        if is_source(&graph, id) {
            assert_eq!(
                source_const(&graph, id),
                signature.outputs[0].default_value().cloned(),
                "{} rests at the output type default",
                node.title(),
            );
        }
    }
    // A reroute (1 input, 1 output) is a passthrough, not a source.
    let knot = graph
        .add_node(
            TREE,
            NodeBody::Kind(MaterialOp::Reroute(PortType::Vector)),
            0,
            0,
        )
        .expect("root tree");
    assert!(!is_source(&graph, knot), "a reroute is not a source");
}

#[test]
fn r1257_intervene_source_value_authors_it_and_reevaluates() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // Boot: Texture(grey) x Color(grey) -> Multiply -> Output; terminal grey64.
        assert_eq!(
            intro.query("node.0.is_source"),
            Ok(IntrospectValue::Bool(true))
        );
        let grey = Color::rgb(0x80, 0x80, 0x80);
        assert_eq!(
            intro.query("node.0.value"),
            Ok(CellValue::Color(grey).to_introspect()),
            "the Texture source reads its (default) constant",
        );
        // Author the Texture source red; the whole graph re-evaluates.
        intro
            .intervene("node.0.value", IntrospectValue::Text("#ff0000".into()))
            .expect("author the source constant");
        let red = Color::rgb(0xff, 0, 0);
        assert_eq!(
            intro.query("node.0.value"),
            Ok(CellValue::Color(red).to_introspect()),
            "the source now emits the authored red",
        );
        // Multiply(red, grey) = (255*128/255, 0, 0) = (128, 0, 0); terminal follows.
        let expected = CellValue::Color(color_mul(red, grey)).to_introspect();
        assert_eq!(
            intro.query("node.2.value"),
            Ok(expected.clone()),
            "Multiply re-evaluated"
        );
        assert_eq!(
            intro.query("eval.output"),
            Ok(expected),
            "the terminal followed the source edit"
        );
    });
}

#[test]
fn r1257_value_write_is_readonly_on_a_derived_node() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // A compute op (Multiply, node 2) and the sink (Output, node 3) have a
        // DERIVED value — authoring it is rejected.
        assert_eq!(
            intro.query("node.2.is_source"),
            Ok(IntrospectValue::Bool(false))
        );
        assert_eq!(
            intro.intervene("node.2.value", IntrospectValue::Text("#ff0000".into())),
            Err(InterveneError::ReadOnly),
            "a compute op's value is read-only",
        );
        assert_eq!(
            intro.intervene("node.3.value", IntrospectValue::Text("#ff0000".into())),
            Err(InterveneError::ReadOnly),
            "the sink's value is read-only",
        );
    });
}

#[test]
fn r1257_source_value_edit_is_one_undoable_step() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        let stack = use_undo();
        let before = intro.query("node.1.value");
        intro
            .intervene("node.1.value", IntrospectValue::Text("#00ff00".into()))
            .expect("author the Color source");
        assert_eq!(
            stack.len(),
            1,
            "one undoable step (shared apply_set_node_value)"
        );
        // A no-op re-write of the same value journals nothing.
        intro
            .intervene("node.1.value", IntrospectValue::Text("#00ff00".into()))
            .expect("re-author the same value");
        assert_eq!(stack.len(), 1, "an unchanged write journals nothing");
        assert!(stack.undo(), "undo restores the prior constant");
        assert_eq!(intro.query("node.1.value"), before, "the source reverted");
    });
}

#[test]
fn r1257_scalar_source_value_is_type_checked() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        let scalar = match intro.invoke("add_node", IntrospectValue::Text("Scalar".into())) {
            Ok(IntrospectValue::Int(id)) => id,
            other => panic!("add Scalar: {other:?}"),
        };
        // Scalar's output is Float — a float authors it, a hex (a Vector value)
        // is a type mismatch.
        assert!(
            intro
                .intervene(&format!("node.{scalar}.value"), IntrospectValue::Float(0.5))
                .is_ok(),
            "a Float authors the Scalar source",
        );
        assert_eq!(
            intro.query(&format!("node.{scalar}.value")),
            Ok(IntrospectValue::Float(0.5))
        );
        assert_eq!(
            intro.intervene(
                &format!("node.{scalar}.value"),
                IntrospectValue::Text("#ff0000".into())
            ),
            Err(InterveneError::TypeMismatch),
            "a hex (Vector) value is rejected against a Float source",
        );
    });
}

// ── R1264 — source-const GUI authoring (paint the constant + inline edit) ────

/// R1264 — the inline editor opens on a SOURCE node's output constant, seeds
/// from its `edit_text`, paints + lowers to a "Source value" textbox, and
/// commits the typed value through the SAME `apply_set_node_value` /
/// `NodeValueTarget::OutputConst` SSOT the AI-first `intervene node.<id>.value`
/// uses (R1257) — one undoable step, then the field wipes. Node 1 is a Color
/// source (Vector output), edited as a `#RRGGBB` hex.
#[test]
fn r1264_source_const_inline_editor_begins_seeds_and_commits() {
    Owner::new().run(|| {
        let scene = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        assert!(
            coord.begin_edit_source_value(NodeId(1)),
            "the Color source's constant opens for edit",
        );
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::SourceConst(NodeId(1))),
            "the editor targets the source constant",
        );
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "#808080",
            "seeded with the current constant's hex",
        );
        // The open field paints over the output row and lowers to a textbox
        // named "Source value" (the paint gate == the a11y gate).
        let painted = view((TextFieldState::Editing, 0), &Frame::new());
        assert!(
            painted.contains_tag(EDIT_TF_TAG),
            "the shared field paints over the source constant",
        );
        let a11y = NodeEditorView::access_node(&(TextFieldState::Editing, 0), Some(EDIT_TF_TAG));
        let textbox = a11y
            .iter()
            .find(|n| n.tag == EDIT_TF_TAG)
            .expect("the source constant lowers to a textbox");
        assert_eq!(textbox.role, AriaRole::TextInput);
        assert_eq!(
            textbox.name.as_deref(),
            Some("Source value"),
            "named for the source-value edit kind",
        );
        // `query editing` reports the source-value target on the card surface.
        assert_eq!(
            graph_intro(&scene).query("editing"),
            Ok(IntrospectValue::Json(serde_json::json!({
                "kind": "source_value", "node": 1, "surface": "card"
            }))),
        );
        use_text_edit_state(EDIT_TF_TAG).set_text("#3366cc".to_owned());
        commit_edit(true);
        assert_eq!(use_active_edit().get(), None, "commit leaves edit mode");
        assert_eq!(
            source_const(&coord.graph(), NodeId(1)),
            Some(CellValue::Color(Color::rgb(0x33, 0x66, 0xcc))),
            "the typed hex parsed into the source constant",
        );
        assert_eq!(
            graph_intro(&scene).query("node.1.value"),
            Ok(CellValue::Color(Color::rgb(0x33, 0x66, 0xcc)).to_introspect()),
            "the source now evaluates to the authored colour",
        );
        assert_eq!(
            use_text_edit_state(EDIT_TF_TAG).text(),
            "",
            "field wiped for the next edit",
        );
        assert_eq!(
            stack.undo_label().as_deref(),
            Some("Set source value"),
            "journaled through the shared OutputConst command",
        );
        assert!(stack.undo());
        assert_eq!(
            source_const(&coord.graph(), NodeId(1)),
            Some(CellValue::Color(Color::rgb(0x80, 0x80, 0x80))),
            "undo restores the prior constant",
        );
    });
}

/// R1264 — the source-value editor's keystroke gate is the constant's OWN
/// `CellKind` (the output-side twin of the R901 pin-default gate): a `Color`
/// source is hex-gated, a `Scalar` (Float output) source is number-gated. The
/// single funnel that keeps the keyboard editor and the AI write from drifting.
#[test]
fn r1264_source_const_editor_uses_the_typed_keystroke_gate() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        assert_eq!(
            edit_target_kind(EditTarget::SourceConst(NodeId(1))),
            CellKind::Color,
            "a Color source is hex-gated",
        );
        let scalar = coord.add_node(5).expect("Scalar"); // Float output source
        assert_eq!(
            edit_target_kind(EditTarget::SourceConst(scalar)),
            CellKind::Float,
            "a Scalar (Float) source is number-gated",
        );
        // Seed + commit the Float source through the same funnel.
        assert!(coord.begin_edit_source_value(scalar));
        assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "0");
        use_text_edit_state(EDIT_TF_TAG).set_text("0.5".to_owned());
        commit_edit(true);
        assert_eq!(
            source_const(&coord.graph(), scalar),
            Some(CellValue::Float(0.5)),
            "the typed float parsed into the source constant",
        );
    });
}

/// R1264 — the editor refuses a NON-source node: a compute op's / sink's value
/// is derived (no `output_const`, no painted `oconst_` label to anchor the
/// field), so opening it would steal focus and advertise an unpainted textbox
/// (the R901.1 wired-port class). The reject holds across every entry — the
/// `begin_edit_source_value` method AND the `begin_edit_value` invoke.
#[test]
fn r1264_source_const_editor_rejects_a_non_source_node() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let coord = coordinator();
        assert!(
            !coord.begin_edit_source_value(NodeId(2)),
            "a compute op (Multiply) has no constant to edit",
        );
        assert!(
            !coord.begin_edit_source_value(NodeId(3)),
            "the sink (Output) has no constant to edit",
        );
        assert_eq!(use_active_edit().get(), None, "nothing opened");
        let intro = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .unwrap()
            .handle
            .introspect_mut()
            .unwrap();
        assert_eq!(
            intro.invoke("begin_edit_value", IntrospectValue::Int(2)),
            Ok(IntrospectValue::Bool(false)),
            "the invoke twin also rejects a compute op",
        );
    });
}

/// R1264 — the editor opens from BOTH the AI-first `invoke begin_edit_value`
/// (an unknown node is rejected, graph unchanged) and a double-click on the
/// source card's `oconst_<id>` value label — the output-side twin of the R901
/// `idefault_` pin-default gesture.
#[test]
fn r1264_begin_edit_value_via_invoke_and_double_click() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        {
            let intro = scene
                .find_external_with_tag_mut(GRAPH_TAG)
                .unwrap()
                .handle
                .introspect_mut()
                .unwrap();
            assert_eq!(
                intro.invoke("begin_edit_value", IntrospectValue::Int(1)),
                Ok(IntrospectValue::Bool(true)),
                "a source node opens by id",
            );
            assert_eq!(
                intro.invoke("begin_edit_value", IntrospectValue::Int(999)),
                Ok(IntrospectValue::Bool(false)),
                "an unknown node is rejected",
            );
        }
        cancel_edit();
        assert_eq!(use_active_edit().get(), None);
        // A double-click on the source card's constant label re-opens it.
        send(&mut scene, "oconst_1:DoubleClick");
        assert_eq!(
            use_active_edit().get(),
            card(EditTarget::SourceConst(NodeId(1))),
            "double-clicking the source constant opens its editor",
        );
    });
}

/// R1264 — an IDLE source card paints its authored constant as a static value
/// label tagged `oconst_<id>` (the output-side twin of the R899 `idefault_`
/// label), observable through `scene/snapshot`; a compute op / sink paints no
/// such label (its output is derived).
#[test]
fn r1264_source_card_paints_its_constant_label() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let painted = view((TextFieldState::Editing, 0), &Frame::new());
        assert!(
            painted.contains_tag(&format!("{GRAPH_TAG}#oconst_0")),
            "the Texture source paints its constant label",
        );
        assert!(
            painted.contains_tag(&format!("{GRAPH_TAG}#oconst_1")),
            "the Color source paints its constant label",
        );
        assert!(
            !painted.contains_tag(&format!("{GRAPH_TAG}#oconst_2")),
            "the Multiply compute op paints no constant label",
        );
        assert!(
            !painted.contains_tag(&format!("{GRAPH_TAG}#oconst_3")),
            "the Output sink paints no constant label",
        );
    });
}

/// R1264 — a malformed hex commit keeps the prior constant (no data loss, no
/// spurious undo step — the `CellKind::parse` contract shared with the R901 pin
/// editor); a subsequent valid edit is exactly one undoable step.
#[test]
fn r1264_malformed_source_const_commit_keeps_prior_value() {
    Owner::new().run(|| {
        let _scene = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        assert!(coord.begin_edit_source_value(NodeId(1)));
        let before = stack.len();
        use_text_edit_state(EDIT_TF_TAG).set_text("nothex".to_owned());
        commit_edit(true);
        assert_eq!(
            source_const(&coord.graph(), NodeId(1)),
            Some(CellValue::Color(Color::rgb(0x80, 0x80, 0x80))),
            "a malformed commit keeps the prior constant",
        );
        assert_eq!(stack.len(), before, "no undo step for a rejected parse");
        // A valid edit journals exactly one step.
        assert!(coord.begin_edit_source_value(NodeId(1)));
        use_text_edit_state(EDIT_TF_TAG).set_text("#112233".to_owned());
        commit_edit(true);
        assert_eq!(
            stack.len(),
            before + 1,
            "one undoable step for a valid edit"
        );
    });
}

// ── R1258 / R1596 — set_graph structural validation (trust boundary) ────────
//
// R1596 — the editor kept three predicates of its own (`node_invariants_hold` /
// `edges_invariants_hold` / `graph_invariants_hold`) checking unique ids, a
// node's stored port list against its op's canonical shape, per-port default
// typing, a source's constant, type-assignable wires and one-wire-per-input.
//
// Every one of those is now EITHER unrepresentable OR `Document::validate`'s,
// and telling those two apart is the point of this block: an invariant that
// became a property of the types is stronger than one a checker enforces,
// because nothing has to remember to run.

/// A minimal valid two-node graph: a Color source wired into an Output sink.
fn valid_pair() -> Graph {
    graph_of(&[(1, 0, 0), (4, 200, 0)], &[(0, 0, 1, 0)])
}

/// The same graph with one link appended through the WIRE FORM, which is how a
/// blob from a peer reaches the editor and the only way an invariant violation
/// can arrive at all.
fn with_forced_link(graph: &Graph, from: (u32, u32), to: (u32, u32)) -> Graph {
    let mut wire = serde_json::to_value(graph).expect("a document serializes");
    let next = wire["trees"][0]["next_link"].as_u64().expect("link ids");
    wire["trees"][0]["links"]
        .as_array_mut()
        .expect("links")
        .push(serde_json::json!({
            "id": next,
            "from": {"node": from.0, "port": from.1},
            "to": {"node": to.0, "port": to.1},
            "muted": false,
        }));
    wire["trees"][0]["next_link"] = serde_json::json!(next + 1);
    serde_json::from_value(wire).expect("the forced document round-trips")
}

#[test]
fn r1258_a_live_graph_passes_and_round_trips() {
    assert!(
        graph_invariants_hold(&default_graph()),
        "the seed graph is valid"
    );
    assert!(graph_invariants_hold(&valid_pair()), "so is a minimal pair");
    Owner::new().run(|| {
        let coord = coordinator();
        let blob = coord.serialized_json();
        assert!(
            coord.load_json(&blob),
            "a graph this editor produced always loads back"
        );
    });
}

#[test]
fn r1258_rejects_an_ill_typed_edge() {
    // Texture (Vector out) into Lerp's input 2 (the Float factor). `connect`
    // refuses it up front, so it can only arrive from a peer.
    let mut graph = graph_of(&[(0, 0, 0), (6, 0, 0)], &[]);
    assert!(
        graph
            .connect(TREE, Socket::new(NodeId(0), 0), Socket::new(NodeId(1), 2))
            .is_err(),
        "an ill-typed wire is refused at the edit"
    );
    let forced = with_forced_link(&graph, (0, 0), (1, 2));
    assert!(!graph_invariants_hold(&forced), "and caught on load");
    assert!(
        forced.validate().iter().any(|v| matches!(
            v,
            Violation::TypeMismatch { tree, .. } if *tree == TREE
        )),
        "named as a type mismatch, not a bare false: {:?}",
        forced.validate()
    );
}

#[test]
fn r1258_a_wrong_arity_node_is_unrepresentable() {
    // The editor could build a node whose stored port list disagreed with its
    // op, and `node_invariants_hold` existed largely to catch that. A kind
    // DECLARES its ports (R1596), so the two cannot disagree: there is no second
    // copy to be wrong. The signature is derived on every read.
    let graph = graph_of(&[(3, 0, 0)], &[]); // Add
    let signature = signature_of(&graph, NodeId(0)).expect("a signature");
    assert_eq!(
        signature.inputs.len(),
        MaterialOp::Add.inputs().len(),
        "arity comes from the kind and from nowhere else"
    );
    assert!(
        graph_invariants_hold(&graph),
        "so there is no arity violation to construct"
    );
}

#[test]
fn r1258_duplicate_ids_are_unrepresentable() {
    // A tree keys its nodes and links BY id, so a duplicate is not a graph the
    // type can hold — where the editor's `Vec<GraphNode>` could carry two of
    // anything and needed a checker to say so.
    let mut graph = valid_pair();
    let again = graph
        .add_node(TREE, NodeBody::Kind(MaterialOp::Color), 0, 0)
        .expect("root tree");
    assert_ne!(again, NodeId(0), "a fresh mint never repeats a live id");
    assert_eq!(
        graph.tree(TREE).expect("root").nodes().count(),
        3,
        "three nodes, three ids"
    );
}

#[test]
fn r1258_rejects_multiple_wires_into_one_input() {
    // Two producers into one input. `connect` displaces the first rather than
    // accepting both, so again this can only arrive from a peer.
    let graph = graph_of(&[(1, 0, 0), (1, 0, 0), (4, 0, 0)], &[(0, 0, 2, 0)]);
    let forced = with_forced_link(&graph, (1, 0), (2, 0));
    assert!(!graph_invariants_hold(&forced));
    assert!(
        forced
            .validate()
            .iter()
            // R1599 renamed this: the limit is a port's own multiplicity, and
            // a CONTROL output has it too, so the violation is named for the
            // rule rather than for the one side that used to be its only case.
            .any(|v| matches!(
                v,
                Violation::Overlinked {
                    side: Side::Input,
                    ..
                }
            )),
        "named as an over-linked input: {:?}",
        forced.validate()
    );
}

#[test]
fn r1258_rejects_a_dangling_endpoint() {
    let graph = valid_pair();
    let forced = with_forced_link(&graph, (0, 0), (99, 0));
    assert!(!graph_invariants_hold(&forced));
    assert!(
        forced
            .validate()
            .iter()
            .any(|v| matches!(v, Violation::DanglingLink { .. })),
        "named as a dangling link: {:?}",
        forced.validate()
    );
}

#[test]
fn r1258_a_mistyped_port_value_is_refused_at_the_edit_and_named_on_load() {
    // A Float authored on a Vector port. The edit path refuses it by type; a
    // value that outlived its PORT is what `validate` reports, and this test
    // pins the refusal, which is the half a peer cannot get past.
    let mut graph = graph_of(&[(3, 0, 0)], &[]); // Add: two Vector inputs
    assert!(
        graph
            .set_port_value(TREE, NodeId(0), PortRef::input(0), CellValue::Float(1.0))
            .is_err(),
        "a Float cannot be authored onto a Vector port"
    );
    assert!(
        graph
            .set_port_value(TREE, NodeId(0), PortRef::input(9), CellValue::Float(1.0))
            .is_err(),
        "nor onto a port the signature does not have"
    );
    assert!(graph_invariants_hold(&graph), "the graph is untouched");
}

#[test]
fn r1258_a_source_constant_cannot_disagree_with_its_node() {
    // The editor stored `output_const: Option<CellValue>` beside `op`, so a blob
    // could claim a constant on a compute node or a reroute — one more thing for
    // a checker to notice. R1594 made an authored value a PORT value, so a node
    // that has no output port has nowhere to put one, and `is_source` is derived
    // from the signature rather than from the field's Some-ness.
    let mut graph = graph_of(&[(4, 0, 0)], &[]); // Output: a sink, no outputs
    assert!(!is_source(&graph, NodeId(0)), "a sink is not a source");
    assert!(
        graph
            .set_port_value(
                TREE,
                NodeId(0),
                PortRef::output(0),
                CellValue::Color(Color::rgb(1, 2, 3))
            )
            .is_err(),
        "and cannot be given an output constant, because it has no output port"
    );
}

#[test]
fn r1258_stale_id_counters_are_unrepresentable() {
    // The editor carried three monotonic counters in the blob and gated a load
    // on each leading its own ids. A tree mints from its own counter, which
    // travels INSIDE the document, so a snapshot cannot disagree with itself.
    Owner::new().run(|| {
        let coord = coordinator();
        let before = coord.next_node_id();
        let blob = coord.serialized_json();
        assert!(coord.load_json(&blob), "the snapshot loads");
        assert_eq!(
            coord.next_node_id(),
            before,
            "and mints exactly where it left off"
        );
        assert!(
            !coord.load_json("{not valid json"),
            "malformed JSON is still rejected"
        );
    });
}

#[test]
fn r1258_a_reroute_graph_passes_validation() {
    Owner::new().run(|| {
        let coord = coordinator();
        coord.add_reroute(EdgeId(0)).expect("splice edge 0");
        let blob = coord.serialized_json();
        assert!(
            coord.load_json(&blob),
            "a spliced reroute round-trips through the wire"
        );
        assert!(
            graph_invariants_hold(&coord.graph()),
            "and the reloaded graph is valid"
        );
    });
}

// ── R1259 / R1596 — frames are nodes, so they validate with them ────────────

#[test]
fn r1259_load_json_validates_frames_and_their_containment() {
    Owner::new().run(|| {
        let coord = coordinator();
        coord.set_selection(Selection::Nodes(BTreeSet::from([NodeId(0), NodeId(1)])));
        let frame = coord.add_frame().expect("framed the selection");
        let blob = coord.serialized_json();
        assert!(blob.contains("Comment 1"), "the frame is in the blob");
        coord.document.set(Graph::new("material"));
        assert!(coord.load_json(&blob), "load the snapshot");
        assert_eq!(coord.frames().len(), 1, "frame restored from persistence");
        assert_eq!(
            coord.graph().members(TREE, frame),
            vec![NodeId(0), NodeId(1)],
            "and so is what it holds — containment travels with the document"
        );

        // R1596 — a frame is a node, so the checks that used to be about frame
        // ids are the ones every node gets, PLUS two the editor never had: a
        // node claiming a parent that is not there, and one claiming a parent
        // that is not a frame.
        let mut broken = coord.graph();
        broken
            .tree_mut(TREE)
            .and_then(|t| t.node_mut(NodeId(0)))
            .expect("Texture")
            .parent = Some(NodeId(99));
        assert!(
            broken
                .validate()
                .iter()
                .any(|v| matches!(v, Violation::DanglingParent { .. })),
            "a parent that is not in the tree is named: {:?}",
            broken.validate()
        );
        let mut not_a_frame = coord.graph();
        not_a_frame
            .tree_mut(TREE)
            .and_then(|t| t.node_mut(NodeId(0)))
            .expect("Texture")
            .parent = Some(NodeId(2));
        assert!(
            not_a_frame
                .validate()
                .iter()
                .any(|v| matches!(v, Violation::ParentNotAFrame { .. })),
            "a container that is not a frame is named: {:?}",
            not_a_frame.validate()
        );
    });
}

// ── R1260 — §2#7 debugger reads (per-input resolved values + cycle localisation) ──

#[test]
fn r1260_cycle_nodes_localises_only_the_cycle_members() {
    // A 2-cycle A(0) <-> B(1), plus a downstream C(2) fed by B. Only A and B are
    // ON the cycle; C is downstream but NOT on it.
    // R1596 — the localisation is `Document::cycle_nodes`, the crate gap that
    // starting this migration surfaced: `Violation::Cycle` named the TREE, so
    // this read had no server. Both cycle answers now come off ONE walk, so
    // "is it acyclic" and "who is on it" cannot disagree.
    let graph = graph_of(
        &[(3, 0, 0), (3, 0, 0), (3, 0, 0)],
        &[(0, 0, 1, 0), (1, 0, 2, 0)],
    );
    let looped = force_cycle(&graph, NodeId(1), NodeId(0));
    assert_eq!(
        looped.cycle_nodes(TREE),
        vec![NodeId(0), NodeId(1)],
        "only the cycle members, sorted (C is downstream, not on the cycle)",
    );
    // A self-loop counts as a cycle.
    let one = graph_of(&[(3, 0, 0)], &[]);
    let selfloop = force_cycle(&one, NodeId(0), NodeId(0));
    assert_eq!(
        selfloop.cycle_nodes(TREE),
        vec![NodeId(0)],
        "a self-loop is a cycle"
    );
    // A DAG has no cycle nodes.
    assert!(
        default_graph().cycle_nodes(TREE).is_empty(),
        "the seed graph has none"
    );
}

#[test]
fn r1260_resolved_input_shows_the_wired_value_else_the_default() {
    // Color source -> Multiply.in0; in1 is unwired and rests at its default.
    let graph = graph_of(&[(1, 0, 0), (2, 0, 0)], &[(0, 0, 1, 0)]);
    let grey = CellValue::Color(Color::rgb(0x80, 0x80, 0x80));
    assert_eq!(
        resolve_input_value(&graph, NodeId(1), 0),
        Some(grey.clone()),
        "wired input resolves the source"
    );
    assert_eq!(
        resolve_input_value(&graph, NodeId(1), 1),
        Some(grey),
        "unwired input resolves its default"
    );
    assert_eq!(
        resolve_input_value(&graph, NodeId(1), 9),
        None,
        "out-of-range port is None"
    );
}

#[test]
fn r1260_resolved_input_shows_the_float_to_vector_broadcast() {
    // A Scalar (Float 0.0) wired into an Add's Vector input: resolved_input shows
    // the POST-coercion value (black), not the raw Float -- the exact broadcast
    // an AI would otherwise re-derive by hand.
    let graph = graph_of(&[(5, 0, 0), (3, 0, 0)], &[(0, 0, 1, 0)]);
    assert_eq!(
        resolve_input_value(&graph, NodeId(1), 0),
        Some(CellValue::Color(Color::rgb(0, 0, 0))),
        "the Float source broadcast to black at the Vector input",
    );
}

#[test]
fn r1260_query_resolved_input_and_localises_a_cycle() {
    Owner::new().run(|| {
        let mut scene = boot_scene();
        let node = scene
            .find_external_with_tag_mut(GRAPH_TAG)
            .expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        // Multiply (node 2) inputs are both wired from grey sources.
        let grey = CellValue::Color(Color::rgb(0x80, 0x80, 0x80)).to_introspect();
        assert_eq!(
            intro.query("node.2.resolved_input.0"),
            Ok(grey.clone()),
            "in0 resolves the wired grey"
        );
        assert_eq!(intro.query("node.2.resolved_input.1"), Ok(grey), "in1 too");
        assert!(
            matches!(
                intro.query("node.2.resolved_input.9"),
                Err(ReadRefusal::NoSuchMember(_))
            ),
            "R1667 - the family is declared and this argument addresses nothing"
        );
        // detail.resolved_input mirrors the selected node.
        intro
            .intervene("selected_ids", IntrospectValue::Text("2".into()))
            .unwrap();
        assert_eq!(
            intro.query("detail.resolved_input.0"),
            intro.query("node.2.resolved_input.0"),
            "detail mirror"
        );
        // The seed graph is a DAG: cycle_nodes is empty.
        assert_eq!(
            intro.query("eval.cycle_nodes"),
            Ok(IntrospectValue::Text(String::new())),
            "no cycle at boot"
        );
        // ★R1596 — a cycle CANNOT BE AUTHORED any more. The editor's own gate
        // blocked only a direct self-loop, so two `add_edge` calls could build a
        // 2-cycle; `Document::connect` refuses any wire that would close one and
        // names the path. So the reads below have to be driven from a document
        // that ARRIVED, which is the only way one can exist.
        let a = match intro.invoke("add_node", IntrospectValue::Text("Add".into())) {
            Ok(IntrospectValue::Int(i)) => i,
            o => panic!("{o:?}"),
        };
        let b = match intro.invoke("add_node", IntrospectValue::Text("Add".into())) {
            Ok(IntrospectValue::Int(i)) => i,
            o => panic!("{o:?}"),
        };
        intro
            .invoke("add_edge", IntrospectValue::Text(format!("{a},0,{b},0")))
            .expect("the forward wire");
        assert_eq!(
            intro.invoke("add_edge", IntrospectValue::Text(format!("{b},0,{a},0"))),
            Ok(IntrospectValue::Bool(false)),
            "the wire that would close the cycle is refused"
        );
        assert_eq!(
            intro.query("eval.acyclic"),
            Ok(IntrospectValue::Bool(true)),
            "so the graph is still a DAG"
        );
    });

    // The cycle reads themselves, over a document built the way one arrives.
    Owner::new().run(|| {
        let coord = coordinator();
        let a = coord.add_node(3).expect("Add");
        let b = coord.add_node(3).expect("Add");
        assert!(coord.add_edge(a, 0, b, 0), "the forward wire");
        let looped = force_cycle(&coord.graph(), b, a);
        coord.document.set(looped);
        assert_eq!(
            coord.query("eval.acyclic"),
            Ok(IntrospectValue::Bool(false)),
            "now cyclic"
        );
        assert_eq!(
            coord.query("eval.cycle_nodes"),
            Ok(IntrospectValue::Text(format!("{},{}", a.0, b.0))),
            "cycle_nodes localises exactly the two knots",
        );
        // A cycle node's resolved input is null (uncomputable).
        assert_eq!(
            coord.query(&format!("node.{}.resolved_input.0", a.0)),
            Ok(IntrospectValue::Null),
            "a cycle-fed input reads null",
        );
    });
}

// ── R1261 — node-graph paint scales: O(nodes·edges) cross-scans -> O((n+e)logn) ──

#[test]
fn r1261_large_graph_paints_the_full_structure_at_scale() {
    // R1261 precomputes the node/edge lookup indices in view_node_cards /
    // view_edges (the paint was O(nodes·edges)). This pins that the refactor's
    // output is identical AT SCALE — exactly one card per node and one wire per
    // edge for a large chain, no drops / dups from the index change.
    Owner::new().run(|| {
        let _ = boot_scene(); // establishes the theme + reactive scopes
        let n: usize = 300;
        let mut graph = Graph::new("material");
        let mut previous: Option<NodeId> = None;
        for _ in 0..n {
            let id = add_palette_node(&mut graph, 3, 0, 0).expect("Add");
            if let Some(from) = previous {
                graph
                    .connect(TREE, Socket::new(from, 0), Socket::new(id, 0))
                    .expect("the chain's wire");
            }
            previous = Some(id);
        }
        let theme = use_theme(THEME_TAG).theme_animated();
        let sel = BTreeSet::new();
        let cards = view_node_cards(&graph, &sel, None, IDLE_TF, &theme, 1.0);
        assert_eq!(cards.len(), n, "one card per node at scale");
        let tags: BTreeSet<String> = cards
            .iter()
            .filter_map(|c| c.tag().map(str::to_owned))
            .collect();
        assert_eq!(
            tags.len(),
            n,
            "every card carries a distinct node tag (no drops/dups)"
        );
        let wires = view_edges(&graph, None, &theme, 1.0);
        assert_eq!(wires.len(), n - 1, "one wire per edge at scale");
    });
}

/// R1358 — a wire's painted geometry is relative to its own `rect`, so the
/// window position of a control point is `rect.origin + command`. This pins
/// that sum against the `edge_curve` SSOT the hit-test reads: paint and
/// hit-test derive from one curve, and R1358 must not slide them apart.
///
/// Falsifiable in both directions — a producer that regressed to
/// window-absolute commands would land at `2 * origin` here, and one that
/// rebased by the raw (unclamped) minimum instead of the rect's origin would
/// miss whenever a wire extends left of the canvas origin (the case the
/// `zero_origin` arm below pins, where `upx` floors the minimum at 0).
#[test]
fn r1358_wire_commands_are_rect_relative_and_sum_to_the_curve() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let theme = use_theme(THEME_TAG).theme_animated();
        // Counts the cases where "the command is NOT the window x" is a
        // meaningful claim (rect.x > 0). Asserted non-zero after the loop so
        // the check below can never be silently skipped into vacuity.
        let mut local_checks = 0_u32;
        // Two cases: a wire well inside the canvas, and one whose control
        // points reach left of x = 0 (where `upx` clamps the rect origin).
        for (label, from, to) in [
            ("interior", (400_i32, 300_i32), (700_i32, 380_i32)),
            ("zero_origin", (10_i32, 40_i32), (-90_i32, 120_i32)),
        ] {
            let scene = view_edge("w".to_string(), from, to, theme.on_surface, 2, 1.0);
            let Scene::Path(p) = &scene else {
                panic!("{label}: view_edge builds a Scene::Path")
            };
            let (c1, c2) = edge_curve(from, to);
            // The four control points in window space, per the shared SSOT.
            // `ppt` is the crate's i32 -> PathPoint helper, so both sides of
            // the comparison are built by the same conversion the producer uses.
            let want = [from, c1, c2, to].map(|(x, y)| ppt(x, y));
            // R1358 — rebase back: rect.origin + command == the window point.
            let org = ppt(ipx(p.rect.x), ipx(p.rect.y));
            let got: Vec<PathPoint> = p
                .commands
                .iter()
                .flat_map(|cmd| match *cmd {
                    PathCommand::MoveTo(pt) | PathCommand::LineTo(pt) => vec![pt],
                    PathCommand::CurveTo { c1, c2, end } => vec![c1, c2, end],
                    _ => vec![],
                })
                .map(|pt| PathPoint::new(org.x + pt.x, org.y + pt.y))
                .collect();
            assert_eq!(got.len(), 4, "{label}: MoveTo + CurveTo = 4 control points");
            for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                assert!(
                    (g.x - w.x).abs() < f32::EPSILON && (g.y - w.y).abs() < f32::EPSILON,
                    "{label}: control point {i}: rect.origin + command = {:?} must \
                     equal the edge_curve window point the hit-test reads {:?}",
                    (g.x, g.y),
                    (w.x, w.y)
                );
            }
            // The commands themselves must NOT be window coordinates — that is
            // the property that lets layout, not the producer, place the wire.
            let PathCommand::MoveTo(start) = p.commands[0] else {
                panic!("{label}: a wire starts with MoveTo")
            };
            if p.rect.x > 0 {
                assert!(
                    (start.x - want[0].x).abs() > f32::EPSILON,
                    "{label}: MoveTo.x must be rect-local, not the window x"
                );
                local_checks += 1;
            }
        }
        assert!(
            local_checks > 0,
            "at least one case must have a non-zero rect origin, or the \
             rect-local claim above never ran"
        );
    });
}

#[test]
fn r1261_wired_input_precompute_matches_the_per_node_scan() {
    // The precomputed wired-port map must equal the old per-node edge scan for
    // every node (identical paint input). Multiple wires into distinct ports of
    // one node aggregate; an unwired node maps to the empty set.
    // Color, Color -> Lerp's inputs 0 and 2 (its input 1 stays unwired).
    // Color -> Lerp.in0 (Vector) and Scalar -> Lerp.in2 (the Float factor);
    // Lerp's in1 stays unwired. The fixture goes through `connect`, so a wire
    // the lattice refuses cannot be stated here at all — the first draft wired a
    // Color into the Float port and the model said so.
    let graph = graph_of(
        &[(1, 0, 0), (5, 0, 0), (6, 0, 0)],
        &[(0, 0, 2, 0), (1, 0, 2, 2)],
    );
    let wires = edges(&graph);
    for (node, _) in kind_nodes(&graph) {
        let precomputed: BTreeSet<usize> = {
            let mut m: BTreeMap<NodeId, BTreeSet<usize>> = BTreeMap::new();
            for e in wires {
                m.entry(e.to.node).or_default().insert(uidx(e.to.port));
            }
            m.get(&node.id).cloned().unwrap_or_default()
        };
        let per_node: BTreeSet<usize> = wires
            .iter()
            .filter(|e| e.to.node == node.id)
            .map(|e| uidx(e.to.port))
            .collect();
        assert_eq!(precomputed, per_node, "node {} wired set", node.id.0);
    }
    // Node 2 (Lerp) has ports 0 and 2 wired, not 1.
    let n2: BTreeSet<usize> = wires
        .iter()
        .filter(|e| e.to.node == NodeId(2))
        .map(|e| uidx(e.to.port))
        .collect();
    assert_eq!(n2, BTreeSet::from([0, 2]), "aggregated wired ports");
}

// ── R1262 — audit-clearance of R1260/R1261 (restore the edge_endpoints SSOT) ──

#[test]
fn r1262_edge_endpoint_variants_share_one_body() {
    // R1261's paint-perf index inlined a copy of the endpoint math; R1262 routes
    // both the linear (cold-caller) and indexed (paint) resolves through the ONE
    // `edge_endpoints_via` body, so they produce IDENTICAL anchors for every edge
    // — a port-anchor change can't drift the drawn wire from the hit-test/knife.
    let graph = default_graph();
    let index: BTreeMap<NodeId, &Node<MaterialOp>> =
        kind_nodes(&graph).map(|(n, _)| (n.id, n)).collect();
    for e in edges(&graph) {
        assert_eq!(
            edge_endpoints(&graph, e),
            edge_endpoints_via(|id| index.get(&id).copied(), e),
            "linear and indexed endpoint resolves agree for edge {}",
            e.id.0,
        );
    }
    // A dangling edge drops identically through both.
    let dangling = Edge {
        id: EdgeId(9),
        from: Socket::new(NodeId(99), 0),
        to: Socket::new(NodeId(0), 0),
        muted: false,
    };
    assert_eq!(
        edge_endpoints(&graph, &dangling),
        None,
        "linear drops a dangling edge"
    );
    assert_eq!(
        edge_endpoints_via(|id| index.get(&id).copied(), &dangling),
        None,
        "indexed drops it identically",
    );
}

// ------------------------------------------------------------------ R1592

/// R1592 — a card's far edge is PART OF THE CARD for an area selection.
///
/// "Touching counts" has been this marquee's stated rule since R880 (the
/// toolkit's rubber band and the engine's share it) and nothing asserted it: a
/// counterfactual that narrowed the extent by one unit on each far side passed
/// `r880_node_marquee_select.py`, because a pointer-driven sweep in screen pixels cannot land on an
/// exact graph-unit edge. The rule now has a name ([`card_span`]) and this is the test
/// that can fail.
#[test]
fn r1592_a_sweep_that_grazes_a_cards_far_edge_takes_it() {
    let graph = graph_of(&[(0, 40, 70)], &[]);
    let node = kind_node(&graph, NodeId(0)).expect("Texture").0;
    let (min, max) = card_span(node);
    assert_eq!(min, Point::new(40, 70));
    assert_eq!(
        max,
        Point::new(node.right().into(), node.bottom().into()),
        "the far edge is included rather than the last unit before it"
    );

    // A sweep whose LEFT edge lands exactly on the card's right edge.
    let grazing = Region::span(node.right().into(), 0, i64::from(node.right()) + 500, 4000);
    assert!(
        grazing.covers_span(min, max, RegionFit::Intersects),
        "touching counts — this is the assertion the pointer demo cannot make"
    );
    // One unit further out and it does not.
    let clear = Region::span(
        i64::from(node.right()) + 1,
        0,
        i64::from(node.right()) + 500,
        4000,
    );
    assert!(
        !clear.covers_span(min, max, RegionFit::Intersects),
        "and the rule stops there, or 'touching' would mean 'nearby'"
    );
    // The same on the near edge, so the rule is symmetric.
    let before = Region::span(0, 0, node.x.into(), 4000);
    assert!(before.covers_span(min, max, RegionFit::Intersects));
    assert!(
        !Region::span(0, 0, i64::from(node.x) - 1, 4000).covers_span(
            min,
            max,
            RegionFit::Intersects
        )
    );
}

// ── R1598 — swap what a node IS, keeping which node it is ─────────

#[test]
fn r1598_swap_keeps_the_id_and_reports_what_it_cost() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        let stack = use_undo();
        // Node 2 is the Multiply: two wired Vector inputs, one wired output.
        let before = coord.edges().len();
        let cost = coord
            .swap_node(NodeId(2), kind_of("Add"))
            .expect("Multiply and Add are both palette kinds");
        // Multiply and Add share (A, B) -> Out, so everything carries BY NAME.
        assert_eq!(
            cost, "in0>in0,in1>in1,out0>out0||",
            "nothing was lost: {cost}"
        );
        assert_eq!(coord.edges().len(), before, "every wire survived");
        // ★ The id survived, which is what the DCC's swap cannot do.
        assert_eq!(
            coord.query("node.2.op"),
            Ok(IntrospectValue::Text("Add".to_owned())),
            "the node at id 2 is an Add now"
        );
        assert_eq!(
            stack.undo_label().as_deref(),
            Some("Swap node"),
            "one undoable step"
        );
        assert!(stack.undo(), "and it undoes");
        assert_eq!(
            coord.query("node.2.op"),
            Ok(IntrospectValue::Text("Multiply".to_owned())),
        );
    });
}

#[test]
fn r1598_a_swap_that_loses_a_port_names_the_wire_and_the_value() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let coord = coordinator();
        // Author a value on Multiply's second input, then swap it for Output,
        // which has ONE input and no output at all.
        assert!(apply_set_node_value(
            &use_graph_handle(),
            NodeId(2),
            NodeValueTarget::InputDefault(1),
            CellValue::Color(Color::rgb(1, 2, 3)),
        ));
        let cost = coord
            .swap_node(NodeId(2), kind_of("Output"))
            .expect("Output is a palette kind");
        let (carried, rest) = cost.split_once('|').expect("three fields");
        let (severed, discarded) = rest.split_once('|').expect("three fields");
        assert_eq!(
            carried, "in0>in0",
            "only the first input has anywhere to go"
        );
        assert!(!severed.is_empty(), "the wires that fed the rest are named");
        assert_eq!(
            discarded, "in1=#010203",
            "★ and so is the authored value that died, WITH WHAT IT WAS -- the \
             address alone leaves a client nothing to show"
        );
        // The graph is left sound: no dangling wire, no stray value.
        assert!(
            graph_invariants_hold(&coord.graph()),
            "{:?}",
            coord.graph().validate()
        );
    });
}

#[test]
fn r1598_swap_refuses_what_it_cannot_name() {
    Owner::new().run(|| {
        let _ = boot_scene();
        let mut coord = coordinator();
        assert_eq!(coord.swap_node(NodeId(99), 0), None, "no such node");
        assert_eq!(coord.swap_node(NodeId(2), 99), None, "no such kind");
        // Over the wire the refusals are NAMED rather than answered false.
        assert_refused_saying(
            &coord.invoke("swap_node", IntrospectValue::Text("2,Nope".to_owned())),
            "not a node kind",
        );
        assert_refused_saying(
            &coord.invoke("swap_node", IntrospectValue::Text("nonsense".to_owned())),
            "malformed argument",
        );
        assert_eq!(
            coord.invoke("swap_node", IntrospectValue::Int(2)),
            Err(InvokeError::TypeMismatch),
        );
        // And a frame is not the application's to overwrite.
        coord.set_selection(Selection::Nodes(BTreeSet::from([NodeId(0)])));
        let frame = coord.add_frame().expect("framed");
        assert_eq!(
            coord.swap_node(frame, kind_of("Add")),
            None,
            "a frame's body is the crate's"
        );
    });
}

#[test]
fn r1599_the_persisted_shape_cannot_change_without_the_version() {
    // The gate under the standing "bump PERSISTED_SCHEMA_VERSION" rule, which
    // until now was prose no commit gate read -- R1597 reshaped this very blob
    // and the version stayed at 7 until an end-of-session audit caught it.
    //
    // The sample must be STRUCTURALLY COMPLETE rather than minimal: the shape
    // that changed in R1599 lives in a tree's `interface`, which only a
    // document with a group definition has, so a bare graph would have hashed
    // identically before and after and the gate would have said nothing.
    let mut graph = default_graph();
    let ids: Vec<NodeId> = graph.tree(TREE).unwrap().nodes().map(|n| n.id).collect();
    graph
        .group(TREE, &ids[..1], "Pinned")
        .expect("a definition, so the persisted blob carries an interface");
    assert!(
        graph
            .definitions()
            .any(|tree| !tree.interface().inputs().is_empty()
                || !tree.interface().outputs().is_empty()),
        "the sample carries at least one interface port, which is what R1599 \
         reshaped -- without this the digest could not see the change"
    );
    // ★ R1689 — the digest is taken over **the text that is actually written**,
    // not over a proxy struct serialized by a different call. Until this round
    // the gate hashed a `SerializedGraph` while the file was produced by
    // `serialized_json`, so a change to how the blob is composed — exactly what
    // this round did — could have moved the file without moving the digest.
    pinion_core::test_fixtures::assert_persisted_shape(
        "hello-node-editor archive",
        PERSISTED_SCHEMA_VERSION,
        &Archive::of(graph)
            .with_companion(EditorState {
                schema_version: PERSISTED_SCHEMA_VERSION,
            })
            .write()
            .expect("the sample is representable"),
        PERSISTED_SHAPE_HISTORY,
    );
}
