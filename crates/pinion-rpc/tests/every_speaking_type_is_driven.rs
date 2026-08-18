//! R1718 §5.12 §2 #7 — **a type that can put a sentence in front of a person
//! is driven by something**, for the JSON-RPC surface.
//!
//! Same census as `pinion-core`'s, pointed at this crate. It matters here for a
//! reason the framework's own types do not have: these sentences travel to an
//! AGENT as well as to a person, and a refusal that reads like another refusal
//! is one an agent cannot act on differently — §2 #2's whole point is that the
//! headless path is the primary one.

use std::path::PathBuf;

use pinion_core::test_fixtures::speech::assert_speaks;
use pinion_core::test_fixtures::speech::census::assert_every_speaker_is_driven;
use pinion_rpc::subscribe::SubscribeError;

/// ★★★★★ R1718 — every refusal this surface can answer a subscribe with is
/// said, and no two of them read alike.
///
/// Measured before this existed: none of the five wordings was read by anything
/// at all. They are the sentence an agent is given when a stream cannot be
/// opened, and two that read alike would leave it retrying the one it cannot
/// fix.
#[test]
fn r1718_every_subscribe_refusal_is_said_and_distinct() {
    let said = [
        (
            "NotStreamable",
            SubscribeError::NotStreamable.message().to_owned(),
        ),
        (
            "SubscriptionsUnavailable",
            SubscribeError::SubscriptionsUnavailable
                .message()
                .to_owned(),
        ),
        (
            "InvalidSince",
            SubscribeError::InvalidSince.message().to_owned(),
        ),
        (
            "InvalidSubscriptionId",
            SubscribeError::InvalidSubscriptionId.message().to_owned(),
        ),
        (
            "UnknownSubscription",
            SubscribeError::UnknownSubscription.message().to_owned(),
        ),
    ];
    assert_speaks("SubscribeError", 5, &said, &[]);
}

/// ★★★★ R1718 — and nothing else in this crate speaks without being driven.
#[test]
fn every_speaking_type_in_this_crate_is_driven_by_the_speech_gate() {
    assert_every_speaker_is_driven(PathBuf::from(env!("CARGO_MANIFEST_DIR")), 1);
}
