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
    // R1593 — over the declared list rather than a hand-written copy of it, so
    // an op added to the palette is added to this gate too. The copy had
    // already drifted once: the refusal that names the alternatives was still
    // saying "swatch/level/mix/fade/output" after `cap` existed.
    for name in Op::PALETTE {
        let op = Op::parse(name).unwrap_or_else(|| panic!("{name} parses"));
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

// --- R1593: a link may convert -----------------------------------------------

/// The lattice this application declares, asserted directly rather than
/// inferred from a graph that happens to exercise it.
#[test]
fn r1593_the_lattice_is_directed() {
    assert_eq!(Op::conversion(&Ty::Colour, &Ty::Colour).name(), "direct");
    assert_eq!(Op::conversion(&Ty::Amount, &Ty::Amount).name(), "direct");
    assert_eq!(Op::conversion(&Ty::Amount, &Ty::Colour).name(), "converted");
    assert_eq!(Op::conversion(&Ty::Colour, &Ty::Amount).name(), "refused");
    // The relation is not symmetric, which is the whole point: `!=` could not
    // have produced this pair of answers.
    assert!(Op::conversion(&Ty::Amount, &Ty::Colour).is_allowed());
    assert!(!Op::conversion(&Ty::Colour, &Ty::Amount).is_allowed());
}

/// The conversion is an exact map, so what an agent reads over the wire is a
/// number it can predict.
#[test]
fn r1593_an_amount_broadcasts_to_the_grey_of_that_intensity() {
    let broadcast =
        |percent: i64| Op::conversion(&Ty::Amount, &Ty::Colour).apply(Val::Amount(percent));
    assert_eq!(broadcast(0), Some(Val::Colour([0, 0, 0])));
    assert_eq!(broadcast(100), Some(Val::Colour([255, 255, 255])));
    assert_eq!(broadcast(50), Some(Val::Colour([127, 127, 127])));
    // Out of range is clamped rather than producing a colour off the scale.
    assert_eq!(broadcast(400), Some(Val::Colour([255, 255, 255])));
    assert_eq!(broadcast(-9), Some(Val::Colour([0, 0, 0])));
}

/// The seed still holds no converting wire — so a demo that makes one is
/// changing something, and the paint assertion below has a negative control.
#[test]
fn r1593_the_seeded_material_converts_nothing() {
    let document = seed();
    assert!(
        document.tree(ROOT).unwrap().links().iter().all(|l| document
            .link_conversion(ROOT, l.id)
            .is_some_and(|c| !c.converts())),
        "every seeded wire carries its value unchanged"
    );
}

/// The wire this application could not have had before R1593: a `Level`
/// feeding a `Fade`'s COLOUR input. The substrate accepts it, the value
/// arrives as a colour, and the result is the fade of that grey.
#[test]
fn r1593_a_level_can_feed_a_colour_input() {
    let mut document = seed();
    let level = NodeId(2);
    let fade = NodeId(4);
    let out = NodeId(5);
    let linked = document
        .connect(ROOT, Socket::new(level, 0), Socket::new(fade, 0))
        .expect("an amount broadcasts into a colour");
    assert!(
        linked.displaced.is_some(),
        "it took the mix's place, and the displacement is reported"
    );
    assert!(
        document
            .link_conversion(ROOT, linked.link)
            .is_some_and(|c| c.converts())
    );
    // Level is 25%: 25*255/100 = 63. Fade at 0% leaves it alone.
    assert_eq!(
        document.evaluator().input(ROOT, Socket::new(out, 0)),
        Some(Val::Colour([63, 63, 63]))
    );
    assert!(document.validate().is_empty());
}

/// The narrowing direction is refused, and the refusal names both ends —
/// which is what makes it actionable rather than a bare failure.
#[test]
fn r1593_a_colour_cannot_narrow_into_an_amount() {
    let mut document = seed();
    let base = NodeId(0);
    let mix = NodeId(3);
    let error = document
        .connect(ROOT, Socket::new(base, 0), Socket::new(mix, 2))
        .unwrap_err();
    let sentence = error.to_string();
    assert!(sentence.contains("Colour"), "{sentence}");
    assert!(sentence.contains("Amount"), "{sentence}");
}

/// The stroke a wire gets is a function of exactly two facts, and their order
/// is the assertion: a muted wire carries no value, so there is nothing for it
/// to convert.
#[test]
fn r1593_a_converting_wire_is_dotted_and_a_muted_one_stays_dashed() {
    use super::WireLook;
    use pinion_core::style::Dash;

    assert_eq!(WireLook::of(false, false), WireLook::Direct);
    assert_eq!(WireLook::of(false, true), WireLook::Converting);
    assert_eq!(WireLook::of(true, false), WireLook::Muted);
    assert_eq!(
        WireLook::of(true, true),
        WireLook::Muted,
        "muted wins: a wire carrying nothing is not converting anything"
    );

    let ink = pinion_core::Color::rgb(1, 2, 3);
    let grey = pinion_core::Color::rgb(9, 9, 9);
    assert_eq!(WireLook::Direct.stroke(ink, grey).dash, None);
    assert_eq!(
        WireLook::Converting.stroke(ink, grey).dash,
        Some(Dash::DOTTED)
    );
    assert_eq!(WireLook::Muted.stroke(ink, grey).dash, Some(Dash::DASHED));
    // The three are distinguishable from each other, which is what a
    // vocabulary means.
    assert_ne!(
        WireLook::Converting.stroke(ink, grey).dash,
        WireLook::Muted.stroke(ink, grey).dash
    );
}

// --- R1594: a socket's value is authored on the node --------------------------

/// The seeded material is unchanged, which is the migration's whole claim: the
/// two `Swatch`es and the `Level` used to carry their constants in the taxonomy
/// and now carry them on their own ports.
#[test]
fn r1594_the_seed_holds_three_values_on_three_nodes() {
    use pinion_node_graph::PortRef;
    let document = seed();
    assert_eq!(
        document.port_value(ROOT, NodeId(0), PortRef::output(0)),
        Some(&Val::Colour([200, 60, 60]))
    );
    assert_eq!(
        document.port_value(ROOT, NodeId(1), PortRef::output(0)),
        Some(&Val::Colour([40, 90, 220]))
    );
    assert_eq!(
        document.port_value(ROOT, NodeId(2), PortRef::output(0)),
        Some(&Val::Amount(25))
    );
    // Two nodes of ONE kind, two values — the thing the payload variant could
    // not express, because a kind is shared and a node is not.
    assert_eq!(
        document.evaluate(ROOT, NodeId(0)),
        vec![Some(Val::Colour([200, 60, 60]))]
    );
    assert_eq!(
        document.evaluate(ROOT, NodeId(1)),
        vec![Some(Val::Colour([40, 90, 220]))]
    );
    assert!(document.validate().is_empty());
}

/// A fresh source emits the KIND's declared resting value, so adding one from
/// the palette does not produce a node that carries nothing.
#[test]
fn r1594_a_fresh_source_rests_where_its_kind_says() {
    use pinion_node_graph::{NodeBody, PortRef};
    let mut document = seed();
    let fresh = document
        .add_node(ROOT, NodeBody::Kind(Op::Swatch), 400, 400)
        .unwrap();
    assert_eq!(
        document.port_value(ROOT, fresh, PortRef::output(0)),
        None,
        "nothing is authored on it"
    );
    assert_eq!(
        document.evaluate(ROOT, fresh),
        vec![Some(Val::Colour([128, 128, 128]))],
        "and it rests where its kind says"
    );
    document
        .set_port_value(ROOT, fresh, PortRef::output(0), Val::Colour([1, 2, 3]))
        .unwrap();
    assert_eq!(
        document.evaluate(ROOT, fresh),
        vec![Some(Val::Colour([1, 2, 3]))]
    );
}

/// The taxonomy classifies its values, so a colour cannot be authored on an
/// amount port. Blender gets this from a different C struct per socket type.
#[test]
fn r1594_the_lattice_gates_what_may_be_authored() {
    use pinion_node_graph::{PortRef, PortValueError};
    let mut document = seed();
    assert_eq!(
        document.set_port_value(ROOT, NodeId(2), PortRef::output(0), Val::Colour([1, 2, 3])),
        Err(PortValueError::WrongType {
            port: PortRef::output(0),
            expected: Ty::Amount,
            found: Ty::Colour,
        })
    );
    assert_eq!(Op::value_type(&Val::Amount(1)), Some(Ty::Amount));
    assert_eq!(Op::value_type(&Val::Colour([0, 0, 0])), Some(Ty::Colour));
}

/// The wire form reads back what it publishes.
#[test]
fn r1594_the_wire_form_round_trips() {
    for value in [
        Val::Amount(0),
        Val::Amount(100),
        Val::Amount(-3),
        Val::Colour([200, 60, 60]),
        Val::Colour([0, 0, 0]),
    ] {
        assert_eq!(Val::parse(&value.wire()), Some(value.clone()), "{value:?}");
    }
    assert_eq!(Val::parse(" 50 "), Some(Val::Amount(50)));
    assert_eq!(Val::parse("1, 2, 3"), Some(Val::Colour([1, 2, 3])));
    assert_eq!(Val::parse("nope"), None);
    assert_eq!(Val::parse("1,2"), None);
}
