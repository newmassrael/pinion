//! R1442 — `hello-topology` unit tests.
//!
//! The interesting claims are about what happens BETWEEN two drawings, so most
//! of these walk the scripted feed and compare consecutive placements. The
//! stability claim is re-derived here from SERVICE NAMES rather than read off
//! `Stats::order_changes`, so a metric that agreed with itself but not with the
//! drawing would still be caught.

use std::collections::BTreeMap;

use pinion_core::Scene;
use pinion_core::style::{Color, Stroke};

use super::{
    CARD_H, CARD_W, Drawing, Mode, STREAM, Step, Topology, card_scene, column_order, relayout,
    seed_topology, wire_scene,
};

/// Apply one scripted step to a topology.
fn apply(topology: &mut Topology, step: Step) -> bool {
    match step {
        Step::Add(name) => topology.add(name).is_some(),
        Step::Remove(name) => topology.remove(name),
        Step::Connect(from, to) => topology.connect(from, to),
        Step::Disconnect(from, to) => topology.disconnect(from, to),
    }
}

/// Every service's `(column, centre)`, keyed by NAME — the drawing as a viewer
/// would describe it, with ids (which they never see) resolved away.
fn placement(topology: &Topology, drawing: &Drawing) -> BTreeMap<String, (usize, i32)> {
    topology
        .services
        .iter()
        .filter_map(|s| {
            Some((
                s.name.clone(),
                (*drawing.columns.get(&s.id)?, *drawing.centres.get(&s.id)?),
            ))
        })
        .collect()
}

/// **The stability claim, re-derived independently of `Stats`.** The pairs of
/// services that shared a column in `before` AND share one in `after`, drawn in
/// the opposite order.
fn flipped_pairs(
    before: &BTreeMap<String, (usize, i32)>,
    after: &BTreeMap<String, (usize, i32)>,
) -> Vec<(String, String)> {
    let mut flipped = Vec::new();
    let names: Vec<&String> = after.keys().filter(|n| before.contains_key(*n)).collect();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            let (before_a, before_b) = (before[*a], before[*b]);
            let (after_a, after_b) = (after[*a], after[*b]);
            if before_a.0 == before_b.0
                && after_a.0 == after_b.0
                && (before_a.1 - before_b.1).signum() != (after_a.1 - after_b.1).signum()
            {
                flipped.push(((*a).clone(), (*b).clone()));
            }
        }
    }
    flipped
}

/// **`Stats::order_changes` re-derived from names**, which is a DIFFERENT
/// question from [`flipped_pairs`] and R1443 is where the two come apart.
///
/// The free axis is global: a service's centre says where it sits vertically in
/// the whole drawing, not just within its column. So a pair counts here when
/// both were on screen before and share a column NOW, drawn in the opposite
/// vertical order to the one the viewer remembers — whether or not they used to
/// share a column. [`flipped_pairs`] asks the stricter question, about pairs the
/// viewer actually saw stacked; every pair it finds is one of these, and a
/// service that moved column while overtaking another is one of these and not
/// one of those.
fn reordered_pairs(
    before: &BTreeMap<String, (usize, i32)>,
    after: &BTreeMap<String, (usize, i32)>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let names: Vec<&String> = after.keys().filter(|n| before.contains_key(*n)).collect();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            let (was_a, was_b) = (before[*a], before[*b]);
            let (now_a, now_b) = (after[*a], after[*b]);
            if now_a.0 == now_b.0 && (was_a.1 > was_b.1) != (now_a.1 > now_b.1) {
                out.push(((*a).clone(), (*b).clone()));
            }
        }
    }
    out
}

/// Play the whole feed in `mode`, returning the topology and drawing at each
/// step (index 0 is the first drawing, before any step).
fn play(mode: Mode) -> Vec<(Topology, Drawing)> {
    let mut topology = seed_topology();
    let mut drawing = relayout(&topology, &BTreeMap::new(), mode);
    let mut out = vec![(topology.clone(), drawing.clone())];
    for step in STREAM {
        assert!(
            apply(&mut topology, step),
            "the scripted step {step:?} applies"
        );
        drawing = relayout(&topology, &drawing.centres, mode);
        out.push((topology.clone(), drawing.clone()));
    }
    out
}

/// The seed graph really does contain a long edge — without one the bend
/// machinery, the routes and the straightness metric would all be vacuous.
#[test]
fn r1442_the_seed_topology_spans_a_wire_over_two_columns() {
    let topology = seed_topology();
    let drawing = relayout(&topology, &BTreeMap::new(), Mode::Stable);
    let gateway = topology.id_of("gw-eu").expect("gw-eu");
    let warehouse = topology.id_of("warehouse").expect("warehouse");
    assert_eq!(drawing.columns[&gateway], 0);
    assert_eq!(
        drawing.columns[&warehouse], 3,
        "the warehouse is three columns downstream of the EU gateway"
    );
    assert_eq!(drawing.stats.bends, 2, "so its direct wire needs two bends");
    assert_eq!(
        drawing.stats.inner, 1,
        "which is exactly one inner segment..."
    );
    assert_eq!(drawing.stats.straight, 1, "...and it runs straight");
}

/// ★ A stable relayout never flips a remembered pair — asserted from the NAMES,
/// step by step through the whole feed, and cross-checked against the metric the
/// view publishes.
#[test]
fn r1442_a_stable_relayout_never_reorders_a_remembered_pair() {
    let frames = play(Mode::Stable);
    for window in frames.windows(2) {
        let (before, after) = (&window[0], &window[1]);
        let flipped = flipped_pairs(
            &placement(&before.0, &before.1),
            &placement(&after.0, &after.1),
        );
        assert!(
            flipped.is_empty(),
            "★ stable relayout flipped {flipped:?}, metric said {}",
            after.1.stats.order_changes
        );
        assert_eq!(
            after.1.stats.order_changes, 0,
            "and the published metric agrees"
        );
    }
    // The feed really did change the graph, or the assertions above are empty.
    let (first, last) = (&frames[0], frames.last().expect("frames"));
    assert_ne!(first.0.services.len(), last.0.services.len());
    assert_ne!(first.1.cards, last.1.cards, "and the drawing did move");
}

/// ★ The counterfactual bites: relaying out fresh each time DOES flip pairs the
/// viewer had learned. Without this the test above would prove nothing about the
/// seeded ordering — a layout that never churned anyway would pass it too.
#[test]
fn r1442_a_fresh_relayout_reorders_what_the_viewer_had_learned() {
    let frames = play(Mode::Fresh);
    let flips: usize = frames
        .windows(2)
        .map(|w| flipped_pairs(&placement(&w[0].0, &w[0].1), &placement(&w[1].0, &w[1].1)).len())
        .sum();
    assert!(
        flips > 0,
        "★ a fresh pass must churn somewhere in the feed, else the stable \
         pass is not being credited with anything"
    );
    let reported: usize = frames.iter().map(|f| f.1.stats.order_changes).sum();
    assert!(reported > 0, "and the metric reports it: {reported}");
}

/// Stability is not free, and the view says what it cost: over the feed the
/// stable drawings carry at least as many crossings as the fresh ones.
#[test]
fn r1442_the_trade_is_visible_in_both_currencies() {
    let stable: usize = play(Mode::Stable).iter().map(|f| f.1.stats.crossings).sum();
    let fresh: usize = play(Mode::Fresh).iter().map(|f| f.1.stats.crossings).sum();
    assert!(
        stable >= fresh,
        "a seeded pass never chooses the order, so it cannot beat a fresh one \
         on crossings: {stable} < {fresh}"
    );
}

/// ★ The long wire is routed through the channel the layout reserved — its
/// polyline has a point in every column it crosses, and the middle run is level.
#[test]
fn r1442_a_long_wire_runs_through_the_reserved_channel() {
    let topology = seed_topology();
    let drawing = relayout(&topology, &BTreeMap::new(), Mode::Stable);
    let at = topology
        .links
        .iter()
        .position(|l| topology.name_of(l.from) == "gw-eu" && topology.name_of(l.to) == "warehouse")
        .expect("the long dependency");
    let points = &drawing.wires[at];
    assert_eq!(points.len(), 4, "start, two bends, end: {points:?}");
    assert_eq!(
        points[1].1, points[2].1,
        "★ the inner segment is level, so the wire runs straight through"
    );
    // The bends sit in the columns BETWEEN the endpoints, not on the cards.
    let from_card = drawing.cards[&topology.id_of("gw-eu").expect("gw-eu")];
    let to_card = drawing.cards[&topology.id_of("warehouse").expect("warehouse")];
    for bend in &points[1..3] {
        assert!(
            bend.0 > from_card.0 + CARD_W && bend.0 < to_card.0,
            "bend {bend:?} is between the two cards"
        );
    }
    // A short dependency needs no bend at all.
    let short = topology
        .links
        .iter()
        .position(|l| topology.name_of(l.from) == "gw-eu" && topology.name_of(l.to) == "api")
        .expect("the short dependency");
    assert_eq!(drawing.wires[short].len(), 2, "a straight hop");
}

/// A new service lands beside the ones it connects to rather than at the bottom
/// of its column — the seeded pass's key propagation, seen end to end.
#[test]
fn r1442_a_new_service_lands_beside_what_it_connects_to() {
    let mut topology = seed_topology();
    let drawing = relayout(&topology, &BTreeMap::new(), Mode::Stable);
    topology.add("sidecar");
    topology.connect("gw-eu", "sidecar");
    topology.connect("sidecar", "db");
    let after = relayout(&topology, &drawing.centres, Mode::Stable);

    let sidecar = topology.id_of("sidecar").expect("sidecar");
    let api = topology.id_of("api").expect("api");
    assert_eq!(
        after.columns[&sidecar], after.columns[&api],
        "one hop from a gateway, like api / auth / search"
    );
    assert_eq!(after.stats.order_changes, 0, "and nothing else moved past");
    let column = column_order(&topology, &after, after.columns[&sidecar]);
    assert_eq!(column.len(), 4, "api, auth, search and sidecar share it");
    // ★ It is PLACED, not appended: an unkeyed vertex sorted by nothing would
    // fall to the bottom of the column, and the propagation is what pulls it up
    // beside the services its own dependencies point at.
    assert_ne!(
        column.last().map(String::as_str),
        Some("sidecar"),
        "★ the new service was parked at the bottom instead of placed: {column:?}"
    );
}

/// Removing a service takes its dependencies with it — a dangling link names a
/// service no layout can place.
#[test]
fn r1442_removing_a_service_drops_its_dependencies() {
    let mut topology = seed_topology();
    let before = topology.links.len();
    assert!(topology.remove("auth"));
    assert_eq!(
        topology.links.len(),
        before - 3,
        "both gateways' links to it, and its own link to db, went with it"
    );
    assert!(!topology.remove("auth"), "and it is gone for good");
    let drawing = relayout(&topology, &BTreeMap::new(), Mode::Stable);
    assert_eq!(drawing.cards.len(), topology.services.len());
}

/// The model rejects what a discovery feed should never produce twice.
#[test]
fn r1442_the_model_rejects_duplicates_and_self_dependency() {
    let mut topology = seed_topology();
    assert!(topology.add("api").is_none(), "a name is unique");
    assert!(!topology.connect("api", "api"), "a self-loop is not a link");
    let before = topology.links.len();
    assert!(!topology.connect("gw-eu", "api"), "already linked");
    assert_eq!(topology.links.len(), before);
    assert!(!topology.connect("gw-eu", "ghost"), "an unknown service");
    assert!(!topology.disconnect("gw-eu", "ghost"));
    assert!(topology.disconnect("gw-eu", "api"), "but a real one goes");
}

/// A card paints as a tagged box plus a tagged label at the placed rect, and a
/// wire as a tagged path whose bounding box covers every point it passes.
#[test]
fn r1442_the_scene_tags_every_card_and_wire() {
    let parts = card_scene(
        "api",
        (40, 90),
        Color::rgb(1, 2, 3),
        Color::rgb(4, 5, 6),
        Color::rgb(7, 8, 9),
    );
    let Scene::Box(card) = &parts[0] else {
        panic!("a card is a box")
    };
    assert_eq!(card.tag.as_deref(), Some("topology.node.api"));
    assert_eq!(card.rect.x, 40);
    assert_eq!(card.rect.y, 90);
    assert_eq!(
        card.rect.h,
        u32::try_from(CARD_H).expect("a positive height"),
        "and it has a real height"
    );
    let Scene::Text(label) = &parts[1] else {
        panic!("a card is labelled")
    };
    assert_eq!(label.tag.as_deref(), Some("topology.label.api"));

    let scene = wire_scene(
        &[(10, 20), (60, 8), (120, 40)],
        Stroke::new(Color::rgb(0, 0, 0), 2),
        "topology.wire.a-b".to_string(),
    )
    .expect("a wire with points");
    let Scene::Path(path) = &scene else {
        panic!("a wire is a path")
    };
    assert_eq!(path.tag.as_deref(), Some("topology.wire.a-b"));
    // The bbox covers the MIDDLE point too, which is the whole reason a routed
    // wire's rect is honest about the columns it crosses.
    assert_eq!(path.rect.x, 10);
    assert_eq!(path.rect.y, 8);
    assert_eq!(path.rect.w, 110);
    assert_eq!(path.rect.h, 32);
    assert_eq!(path.commands.len(), 3, "a move and two lines");
    assert!(wire_scene(&[], Stroke::new(Color::rgb(0, 0, 0), 2), "t".into()).is_none());
}

/// The graph and drawing a stable view is left holding at the end of the feed —
/// the state the R1443 verb exists to relieve.
fn after_a_stable_feed() -> (Topology, Drawing) {
    play(Mode::Stable).pop().expect("the feed ran")
}

/// ★ R1443, the non-vacuity guard: playing the feed stably really does leave a
/// tangle behind. Every claim below about relieving one is empty without it, and
/// a feed that happened to stay tidy would let them all pass.
#[test]
fn r1443_a_stable_feed_accumulates_a_tangle() {
    let (_, drawing) = after_a_stable_feed();
    assert!(
        drawing.stats.crossings > 0,
        "★ the stable feed ended tidy, so there is nothing to untangle"
    );
    assert_eq!(
        drawing.stats.order_changes, 0,
        "and it got there without moving anything the viewer had learned"
    );
}

/// ★ R1443 — untangling relieves the tangle a stable view is holding, and the
/// metric it publishes agrees with the placement.
///
/// The moved pairs are re-derived from SERVICE NAMES, so a `Stats` that agreed
/// with itself and not with the drawing would still be caught.
#[test]
fn r1443_untangling_relieves_what_a_stable_view_is_holding() {
    let (topology, tangled) = after_a_stable_feed();
    let before = placement(&topology, &tangled);
    let settled = relayout(&topology, &tangled.centres, Mode::Settled);
    assert!(
        settled.stats.crossings < tangled.stats.crossings,
        "★ the tangle was relieved: {} -> {}",
        tangled.stats.crossings,
        settled.stats.crossings
    );
    let after = placement(&topology, &settled);
    let moved = flipped_pairs(&before, &after);
    assert!(!moved.is_empty(), "and it cost something: {moved:?}");
    let reordered = reordered_pairs(&before, &after);
    assert_eq!(
        reordered.len(),
        settled.stats.order_changes,
        "★ the published metric agrees with the names: {reordered:?}"
    );
    // The stricter question is a subset of the one the metric asks, never the
    // other way round — a pair the viewer saw stacked and now sees the other way
    // up is necessarily a pair whose remembered vertical order was inverted.
    for pair in &moved {
        assert!(reordered.contains(pair), "{pair:?} is not counted at all");
    }
}

/// ★ R1443 — the claim the verb exists for, measured at every point in the feed
/// rather than only at the end: untangling never costs more of the drawing than
/// a fresh pass, and somewhere it costs dramatically less.
///
/// Both are needed. Without the second, a feed on which the two happen to agree
/// everywhere would pass — and the END of this feed is such a point, which is
/// why the whole feed is walked.
#[test]
fn r1443_untangling_costs_less_of_the_drawing_than_a_fresh_pass() {
    let mut best_saving = 0;
    let mut tidier_than_fresh = 0;
    for (topology, drawing) in play(Mode::Stable) {
        let now = placement(&topology, &drawing);
        let settled = relayout(&topology, &drawing.centres, Mode::Settled);
        let fresh = relayout(&topology, &drawing.centres, Mode::Fresh);
        let moved = flipped_pairs(&now, &placement(&topology, &settled)).len();
        let churned = flipped_pairs(&now, &placement(&topology, &fresh)).len();
        assert!(
            moved <= churned,
            "★ untangling moved {moved} pairs where a fresh pass moved {churned}"
        );
        assert!(
            settled.stats.crossings <= drawing.stats.crossings,
            "and an exchange never makes the drawing less tidy"
        );
        best_saving = best_saving.max(churned - moved);
        if settled.stats.crossings < fresh.stats.crossings {
            tidier_than_fresh += 1;
        }
    }
    assert!(
        best_saving >= 3,
        "★ the feed never separates the two: best saving was {best_saving} pairs"
    );
    // ★ And the direction nobody promised: a fresh pass minimises crossings with
    // a heuristic, so a local exchange from a remembered order can land tidier
    // than it as well as cheaper. That happens here, and it is the reason
    // `settled` is a defensible standing policy rather than a compromise.
    assert!(
        tidier_than_fresh > 0,
        "★ no frame where settling beat a fresh pass on crossings too"
    );
}

/// The honest limit, pinned so it cannot quietly change: an exchange has to pay
/// for itself on its own, so a tangle that only unwinds by moving two vertices
/// at once survives a settled pass. Somewhere in this feed, one does.
#[test]
fn r1443_a_local_exchange_does_not_claim_the_minimum() {
    let stuck = play(Mode::Stable)
        .into_iter()
        .filter(|(t, d)| relayout(t, &d.centres, Mode::Settled).stats.crossings > 0)
        .count();
    assert!(
        stuck > 0,
        "★ every tangle in this feed came out, so the documented limit is untested here"
    );
}

/// ★ And the other half: with nothing tangled there is nothing to buy, so a
/// settled relayout is a stable one — identical placement, not merely a similar
/// order. This is what makes it safe as a standing policy.
#[test]
fn r1443_untangling_an_untangled_drawing_changes_nothing() {
    let (topology, tangled) = after_a_stable_feed();
    let once = relayout(&topology, &tangled.centres, Mode::Settled);
    let twice = relayout(&topology, &once.centres, Mode::Settled);
    assert_eq!(
        placement(&topology, &twice),
        placement(&topology, &once),
        "★ a second untangle moved something"
    );
    assert_eq!(twice.stats.order_changes, 0);
    // The same graph laid out stably from the settled drawing is the settled
    // drawing: the exchange step found nothing a seeded pass would not.
    let stable = relayout(&topology, &once.centres, Mode::Stable);
    assert_eq!(placement(&topology, &stable), placement(&topology, &once));
}

/// ★ R1443 — as a POLICY rather than a verb: a view drawn settled all the way
/// through the feed never accumulates the tangle in the first place, and still
/// costs less churn than a fresh one.
#[test]
fn r1443_a_settled_feed_never_accumulates_the_tangle() {
    let settled = play(Mode::Settled);
    let stable = play(Mode::Stable);
    let last = |frames: &[(Topology, Drawing)]| frames.last().expect("frames").1.stats;
    assert!(
        last(&settled).crossings < last(&stable).crossings,
        "★ settled ended on {} crossings, stable on {}",
        last(&settled).crossings,
        last(&stable).crossings
    );
    let churn = |frames: &[(Topology, Drawing)]| -> usize {
        frames
            .windows(2)
            .map(|w| {
                flipped_pairs(&placement(&w[0].0, &w[0].1), &placement(&w[1].0, &w[1].1)).len()
            })
            .sum()
    };
    assert!(
        churn(&settled) < churn(&play(Mode::Fresh)),
        "★ and moved less of the drawing than a fresh feed: {} against {}",
        churn(&settled),
        churn(&play(Mode::Fresh))
    );
    // Step by step, the published metric answers its own question exactly.
    for window in settled.windows(2) {
        let (before, after) = (&window[0], &window[1]);
        assert_eq!(
            reordered_pairs(
                &placement(&before.0, &before.1),
                &placement(&after.0, &after.1)
            )
            .len(),
            after.1.stats.order_changes,
            "the metric and the names disagree after {} services",
            after.0.services.len()
        );
    }
}

/// ★ R1443 — the two questions are genuinely different on this feed, so
/// [`reordered_pairs`]'s docs are describing something that happens rather than
/// something imaginable.
///
/// The case is a service that changes COLUMN while overtaking another: nobody
/// ever saw the two stacked, so [`flipped_pairs`] rightly ignores them, but the
/// viewer did see one above the other and now sees the reverse.
#[test]
fn r1443_a_column_change_can_reorder_what_no_column_ever_held() {
    let both = play(Mode::Settled)
        .windows(2)
        .filter_map(|w| {
            let (before, after) = (placement(&w[0].0, &w[0].1), placement(&w[1].0, &w[1].1));
            let wider = reordered_pairs(&before, &after).len();
            (wider > flipped_pairs(&before, &after).len()).then_some(wider)
        })
        .count();
    assert!(
        both > 0,
        "★ no step of this feed separates the two counts, so the distinction is \
         untested here"
    );
}

/// Cards do not overlap: the layout's row gap survives the solver's centres
/// becoming card rects.
#[test]
fn r1442_no_two_cards_in_a_column_overlap() {
    for (topology, drawing) in play(Mode::Stable) {
        let mut by_column: BTreeMap<usize, Vec<i32>> = BTreeMap::new();
        for service in &topology.services {
            by_column
                .entry(drawing.columns[&service.id])
                .or_default()
                .push(drawing.cards[&service.id].1);
        }
        for (column, mut tops) in by_column {
            tops.sort_unstable();
            for pair in tops.windows(2) {
                assert!(
                    pair[1] - pair[0] >= CARD_H,
                    "column {column} stacks two cards: {pair:?}"
                );
            }
        }
    }
}
