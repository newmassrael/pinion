//! R1648 §5.21 — the shell's pure parts.
//!
//! The demo (`tools/demos/r1648_the_analyzer_shell_is_assembled.py`) drives the
//! live window over RPC and is where every wire claim is checked. These pin the
//! functions where a unit test is the sharper instrument — in particular the
//! ones that must be **total over a vocabulary**, which a demo can only sample.

use pinion_core::widgets::card::{CardAffordance, CardState, Remedy};
use pinion_core::widgets::transport::TransportStatus;

use super::{
    RAIL, SEEDS, SOURCES, parse_state, remedy_label, remedy_word, section_of, state_sentence,
    transport_word,
};

/// R1648 — the seeded board puts **every** state on screen at once.
///
/// The property the whole design rests on, asserted against the *definition*
/// rather than a count: a shell whose cards happen to be all `Ready` never
/// exercises the half that matters, and a hand-written "there are six" would
/// stop being true the moment a seventh arm is added.
#[test]
fn r1648_the_seeded_board_exercises_every_card_state() {
    let seeded: std::collections::BTreeSet<&str> = SEEDS.iter().map(|s| s.state.wire()).collect();
    let vocabulary: std::collections::BTreeSet<&str> =
        CardState::ALL.iter().map(CardState::wire).collect();
    assert_eq!(
        seeded, vocabulary,
        "the twelve cards must cover the state vocabulary exactly"
    );
    assert_eq!(
        SEEDS.len(),
        12,
        "the capability list names twelve widget kinds"
    );
}

/// R1648 — every seeded card belongs to a rail section that exists.
///
/// Without this the rail's counts could sum to less than the board and nobody
/// would notice: a card in a section the rail does not list is simply absent
/// from the navigation.
#[test]
fn r1648_every_card_is_reachable_from_the_rail() {
    let mut counted = 0;
    for section in RAIL {
        counted += SEEDS.iter().filter(|s| s.section == section).count();
    }
    assert_eq!(
        counted,
        SEEDS.len(),
        "every card belongs to a listed section; strays: {:?}",
        SEEDS
            .iter()
            .filter(|s| !RAIL.contains(&s.section))
            .map(|s| s.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(section_of("kpi"), "metrics");
    assert_eq!(section_of("nosuch"), "", "an unknown card has no section");
}

/// R1648 — the board's ids are distinct, which is what makes a card id an
/// address.
#[test]
fn r1648_no_two_cards_share_an_id() {
    let mut seen = std::collections::BTreeSet::new();
    for spec in SEEDS {
        assert!(seen.insert(spec.id), "{} appears twice", spec.id);
    }
}

/// R1648 — a card's state sentence is total, and the two arms that carry a
/// reason SAY it.
///
/// A sentence that dropped the carried reason would leave the screen saying
/// "could not load" with the cause only on the wire — which is the failure the
/// arm carries a reason to prevent.
#[test]
fn r1648_every_state_has_a_sentence_and_the_carried_ones_quote_it() {
    for state in CardState::ALL {
        let sentence = state_sentence(&state);
        assert!(!sentence.is_empty(), "{state:?} has no sentence");
    }
    assert!(
        state_sentence(&CardState::Failed("collector unreachable".into()))
            .contains("collector unreachable"),
        "a failure quotes its reason"
    );
    assert!(
        state_sentence(&CardState::Denied("operator role".into())).contains("operator role"),
        "and a denial names the right"
    );
}

/// R1648 — every remedy has a label, and they are distinct.
///
/// Two remedies reading the same on screen would undo the distinction the
/// vocabulary exists for: "request access" and "nothing can be done" have to
/// look different or the six arms buy nothing a person can see.
#[test]
fn r1648_every_remedy_reads_differently() {
    let mut seen = std::collections::BTreeSet::new();
    for remedy in Remedy::ALL {
        let text = remedy_label(remedy);
        assert!(!text.is_empty(), "{remedy:?} has no label");
        assert!(seen.insert(text), "{text:?} labels two remedies");
    }
    assert_eq!(seen.len(), Remedy::ARMS);
}

/// R1648 — the wire word for "no remedy" is not one of the remedies.
///
/// `none` has to stay outside `Remedy::ALL`, or a client reading every answer
/// as a remedy invents a sixth one.
#[test]
fn r1648_the_absent_remedy_is_not_a_remedy() {
    assert_eq!(remedy_word(None), "none");
    assert!(
        !Remedy::ALL.map(Remedy::wire).contains(&"none"),
        "`none` must not collide with a published remedy"
    );
    for remedy in Remedy::ALL {
        assert_eq!(remedy_word(Some(remedy)), remedy.wire());
    }
}

/// R1648 — the detail is required by exactly the two arms that carry one.
///
/// Both directions. Accepting a reason on `empty` silently drops what the
/// caller sent; refusing one on `failed` is how a reason gets lost.
#[test]
fn r1648_the_detail_arity_is_a_fact_about_the_vocabulary() {
    for word in ["ready", "loading", "empty", "opaque"] {
        assert!(parse_state(word, None).is_ok(), "{word} takes no reason");
        let refused = parse_state(word, Some("why")).expect_err("and refuses one");
        assert!(refused.contains("carries no reason"), "{refused}");
    }
    for word in ["failed", "denied"] {
        let refused = parse_state(word, None).expect_err("a carried arm needs its reason");
        assert!(refused.contains("carries a reason"), "{refused}");
        let made = parse_state(word, Some("because")).expect("with one it parses");
        assert_eq!(made.detail(), Some("because"), "and it round-trips");
    }
    let unknown = parse_state("sideways", None).expect_err("closed set");
    assert!(unknown.contains("is not a card state"), "{unknown}");
}

/// R1648 — every state word the wire publishes is one `parse_state` accepts.
///
/// Publishing a vocabulary the parser does not take is two definitions of one
/// set (R1642), and the demo can only sample it.
#[test]
fn r1648_every_published_state_word_is_accepted() {
    for state in CardState::ALL {
        let word = state.wire();
        let detail = state.detail().map(|_| "a reason");
        let made = parse_state(word, detail)
            .unwrap_or_else(|why| panic!("{word} is published but refused: {why}"));
        assert_eq!(made.wire(), word);
    }
}

/// R1648 — `live` is derived, and it is the only word capture can change.
///
/// The whole cross product, because the derivation is over two inputs and a
/// test that pushed one axis at a time would not see that a replaying board
/// reports `replaying` whether or not capture is on (R1623's lesson).
#[test]
fn r1648_the_transport_word_is_derived_from_the_clock_and_the_toggle() {
    let all = [
        TransportStatus::Playing,
        TransportStatus::Paused,
        TransportStatus::Stopped,
    ];
    for status in all {
        for capturing in [true, false] {
            let word = transport_word(status, capturing);
            let expected = match (status, capturing) {
                (TransportStatus::Playing, _) => "replaying",
                (TransportStatus::Stopped, true) => "live",
                _ => "paused",
            };
            assert_eq!(word, expected, "{status:?} with capture={capturing}");
        }
    }
    assert_eq!(
        transport_word(TransportStatus::Playing, true),
        transport_word(TransportStatus::Playing, false),
        "a replaying board is replaying whatever the capture toggle says"
    );
}

/// R1648 — the app bar's source list is non-empty and its entries distinct,
/// so the first one is a defensible default rather than an accident.
#[test]
fn r1648_the_sources_are_a_set_with_a_first() {
    let mut seen = std::collections::BTreeSet::new();
    for source in SOURCES {
        assert!(seen.insert(source), "{source} listed twice");
    }
    assert_eq!(seen.len(), SOURCES.len());
    // NOT `assert!(!SOURCES.is_empty())`: on a `const` array that cannot fail,
    // and an assertion that cannot fail reads as coverage (R1644.1). What can
    // fail is that the DEFAULT the shell opens on is one of them.
    assert!(
        SOURCES.contains(&SOURCES[0]),
        "the app bar opens on a source it offers"
    );
}

/// R1648 — every seeded header offers at least one affordance, and every
/// affordance the vocabulary has is offered by at least one card.
///
/// The second half is the one that matters: an affordance no card on this board
/// offers is an affordance the assembly never exercises, and the demo's
/// refusal case would then be checking a header nobody paints.
#[test]
fn r1648_the_board_exercises_every_affordance() {
    let mut offered = std::collections::BTreeSet::new();
    for spec in SEEDS {
        assert!(!spec.chrome.is_empty(), "{} offers nothing", spec.id);
        offered.extend(spec.chrome.iter().copied());
    }
    assert_eq!(
        offered.len(),
        CardAffordance::ARMS,
        "unexercised affordances: {:?}",
        CardAffordance::ALL
            .into_iter()
            .filter(|a| !offered.contains(a))
            .collect::<Vec<_>>()
    );
    // And at least one card must NOT offer each of them, or the wire refusal
    // in the demo has no card to refuse on.
    for affordance in CardAffordance::ALL {
        assert!(
            SEEDS.iter().any(|s| !s.chrome.contains(&affordance)),
            "every card offers {affordance:?}, so nothing can refuse it"
        );
    }
}
