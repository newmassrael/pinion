//! R1602 — the census verdicts this crate is responsible for, proven.
//!
//! The reference census (`tools/reference_census.py`, `docs/reference-census.json`)
//! judges every node operator Blender and Unreal register. Three of those
//! verdicts are `have` because of a capability that lives **here** rather than
//! in `pinion-node-graph`: `NODE_OT_select_box`, `NODE_OT_select_circle` and
//! `NODE_OT_select_lasso` test a region against the node's *drawn* rectangle,
//! which R1590 measured as a fact about the painted surface and not about a
//! node model — a model crate has no card geometry.
//!
//! So a proof is addressed `<crate>::<test>` in the pin, and this file answers
//! for the rows addressed to `pinion-core`. The shape of the check is the same
//! one `pinion-node-graph/tests/reference_census.rs` carries, and the two are
//! deliberately not lifted into a shared helper yet: two is not three, and the
//! second copy is what would tell us what the shared thing has to be.

use std::collections::{BTreeMap, BTreeSet};

use pinion_core::region::{Point, Region, RegionError, RegionFit};
use serde::Deserialize;

// -------------------------------------------------------------- the pin

const PIN: &str = include_str!("../../../docs/reference-census.json");
const CRATE: &str = "pinion-core";

#[derive(Deserialize)]
struct Row {
    verdict: String,
    #[serde(default)]
    proven_by: String,
}

/// One `have` verdict and the proof that runs it. The name is read off the
/// function by the **compiler** rather than transcribed — see the note on
/// `proof` in `pinion-node-graph`'s census, where a counterfactual showed that
/// a bare `fn()` pointer leaves `proven_by` free to name a test that is not
/// there.
struct Proof {
    tree: &'static str,
    operator: &'static str,
    name: &'static str,
    run: Box<dyn Fn()>,
}

fn proof<F: Fn() + 'static>(tree: &'static str, operator: &'static str, run: F) -> Proof {
    let path = std::any::type_name_of_val(&run);
    Proof {
        tree,
        operator,
        name: path.rsplit("::").next().unwrap_or(path),
        run: Box::new(run),
    }
}

fn proofs() -> Vec<Proof> {
    vec![
        proof("blender", "NODE_OT_select_box", blender_select_box),
        proof("blender", "NODE_OT_select_circle", blender_select_circle),
        proof("blender", "NODE_OT_select_lasso", blender_select_lasso),
    ]
}

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

/// The pin's `have` rows addressed to this crate and the proofs in this file are
/// the same set, and each row names the proof that runs it.
#[test]
fn the_pin_and_the_proofs_are_in_bijection() {
    let pin: BTreeMap<String, BTreeMap<String, Row>> =
        serde_json::from_str(PIN).expect("the census pin parses");

    let mut claimed: BTreeMap<(String, String), String> = BTreeMap::new();
    for (tree, rows) in &pin {
        for (operator, row) in rows {
            if row.verdict != "have" {
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
    assert_eq!(
        pinned, mine,
        "the pin's {CRATE} rows and this file's proofs must be the same set"
    );
    for entry in &table {
        assert_eq!(entry.name, proof_name(entry.tree, entry.operator));
        assert_eq!(
            claimed[&(entry.tree.to_owned(), entry.operator.to_owned())],
            entry.name,
            "{}/{} names a proof the compiler says is called something else",
            entry.tree,
            entry.operator
        );
        (entry.run)();
    }
}

// ------------------------------------------------------------- the proofs

/// One node's card, in **graph units** — the coordinate space a node editor
/// actually selects in: pan-invariant, and negative to the left of the origin.
struct Card {
    name: &'static str,
    min: Point,
    max: Point,
}

fn canvas() -> Vec<Card> {
    vec![
        Card {
            name: "left",
            min: Point::new(-300, -40),
            max: Point::new(-200, 20),
        },
        Card {
            name: "middle",
            min: Point::new(-60, -40),
            max: Point::new(40, 20),
        },
        Card {
            name: "right",
            min: Point::new(200, -40),
            max: Point::new(300, 20),
        },
        Card {
            name: "below",
            min: Point::new(-60, 400),
            max: Point::new(40, 460),
        },
    ]
}

/// One selection policy for every shape. That is the point of the type: Blender
/// has three operators and three implementations, so its box, circle and lasso
/// are free to disagree about what "selected" means.
fn selected(region: &Region, fit: RegionFit) -> Vec<&'static str> {
    canvas()
        .iter()
        .filter(|card| region.covers_span(card.min, card.max, fit))
        .map(|card| card.name)
        .collect()
}

/// Blender's box select. A marquee drawn in graph units, including the negative
/// half of the plane the canvas pans into — and `RegionFit` is the axis Blender
/// does not have: its box select takes whatever it touches, with no way to ask
/// for the nodes fully inside.
#[test]
fn blender_select_box() {
    let touching = Region::span(-320, -100, 0, 100);
    assert_eq!(
        selected(&touching, RegionFit::Intersects),
        vec!["left", "middle"]
    );
    assert_eq!(
        selected(&touching, RegionFit::Contains),
        vec!["left"],
        "only the card the marquee covers whole"
    );

    let dragged_backwards = Region::span(0, 100, -320, -100);
    assert_eq!(
        dragged_backwards, touching,
        "a drag in any direction is the same region"
    );
    assert_eq!(Region::span(5, 5, 5, 5).validate(), Ok(()));
    assert_eq!(
        Region::rect(10, 10, 0, 4).validate(),
        Err(RegionError::Empty)
    );
}

/// Blender's circle select — a brush, so what it means is a disc rather than
/// the square Blender's own `NODE_OT_select_box` would give for the same drag.
/// The predicate is exact integer geometry, so the node whose corner is one unit
/// outside the radius is not taken.
#[test]
fn blender_select_circle() {
    let brush = Region::circle(-10, -10, 80);
    assert_eq!(selected(&brush, RegionFit::Intersects), vec!["middle"]);

    let wide = Region::circle(-10, -10, 300);
    assert_eq!(
        selected(&wide, RegionFit::Intersects),
        vec!["left", "middle", "right"],
        "the row is within the radius and the card 410 units below it is not — \
         which a square of the same reach would have taken"
    );
    assert!(!Region::circle(0, 0, 1).covers_span(
        Point::new(10, 10),
        Point::new(20, 20),
        RegionFit::Intersects
    ));
    assert_eq!(Region::circle(0, 0, 0).validate(), Err(RegionError::Empty));
}

/// Blender's lasso. Closed by derivation, so a caller never repeats the first
/// point the way Blender's own buffer and Qt's `QPolygonF` both require; the
/// interior is the even-odd rule, which is what a hand-drawn loop means; and a
/// **degenerate** lasso is named rather than answered with an empty selection —
/// there, "your lasso bounded nothing" and "nothing was there" are one value.
#[test]
fn blender_select_lasso() {
    let around_the_row = Region::lasso([(-320, -60), (320, -60), (320, 40), (-320, 40)]);
    assert_eq!(
        selected(&around_the_row, RegionFit::Intersects),
        vec!["left", "middle", "right"]
    );

    // A concave loop that reaches around the middle card without enclosing it.
    let horseshoe = Region::lasso([
        (-320, -60),
        (320, -60),
        (320, 40),
        (60, 40),
        (60, -50),
        (-80, -50),
        (-80, 40),
        (-320, 40),
    ]);
    assert_eq!(
        selected(&horseshoe, RegionFit::Contains),
        vec!["left", "right"],
        "the even-odd interior excludes the card the loop reaches around"
    );

    assert_eq!(
        Region::lasso([(0, 0), (10, 10)]).validate(),
        Err(RegionError::LassoTooShort { vertices: 2 })
    );
}
