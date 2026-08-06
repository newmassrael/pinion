//! R1577 — the example's own gates. What is asserted here is the COMPOSITION:
//! that a five-arm taxonomy plus this file's paint is a working node system.

use super::{Op, Ty, Val, seed};
use pinion_node_graph::{Document, NodeBody, NodeId, NodeKind, ROOT, Socket};

/// The seed is the fixture, so a demo reading it is reading what ships.
#[test]
fn r1577_the_seeded_material_evaluates() {
    let document = seed();
    let out = document
        .tree(ROOT)
        .unwrap()
        .nodes()
        .find(|n| n.body == NodeBody::Kind(Op::Output))
        .unwrap();
    // mix(200,60,60 / 40,90,220 @ 25%) then fade @ 0% -> unchanged.
    assert_eq!(
        document.evaluator().input(ROOT, Socket::new(out.id, 0)),
        Some(Val::Colour([160, 67, 100]))
    );
    assert!(document.validate().is_empty());
}

#[test]
fn r1577_grouping_the_mix_derives_three_in_and_one_out() {
    let mut document = seed();
    let mix = NodeId(3);
    let made = document.group(ROOT, &[mix], "Blend").unwrap();
    let interface = document.tree(made.definition).unwrap().interface();
    assert_eq!(
        interface
            .inputs()
            .iter()
            .map(|p| (p.name.as_str(), p.ty))
            .collect::<Vec<_>>(),
        vec![
            ("Base", Ty::Colour),
            ("Blend", Ty::Colour),
            ("Factor", Ty::Amount),
        ]
    );
    assert_eq!(interface.outputs().len(), 1);
    assert!(document.validate().is_empty());
}

#[test]
fn r1577_grouping_does_not_change_what_the_material_is() {
    let mut document = seed();
    let out = NodeId(5);
    let before = document.evaluator().input(ROOT, Socket::new(out, 0));
    document.group(ROOT, &[NodeId(3)], "Blend").unwrap();
    assert_eq!(
        document.evaluator().input(ROOT, Socket::new(out, 0)),
        before
    );
}

#[test]
fn r1577_a_bypass_selection_is_refused_by_name() {
    let mut document = seed();
    // {swatch, output}: a path leaves and returns through mix and fade.
    let error = document
        .group(ROOT, &[NodeId(0), NodeId(5)], "Bad")
        .unwrap_err();
    let sentence = error.to_string();
    assert!(sentence.contains("cycle"), "{sentence}");
    assert!(sentence.contains("0 -> 3 -> 4 -> 5"), "{sentence}");
}

#[test]
fn r1577_the_palette_names_round_trip() {
    for name in ["swatch", "level", "mix", "fade", "output"] {
        let op = Op::parse(name).unwrap();
        assert_eq!(op.name().to_lowercase(), name);
    }
    assert!(Op::parse("nope").is_none());
}

#[test]
fn r1577_the_document_is_a_signal_payload() {
    // The property `Signal<T>` needs, asserted here rather than discovered when
    // the window opens.
    fn owned<T: serde::de::DeserializeOwned + serde::Serialize>() {}
    owned::<Document<Op>>();
}
