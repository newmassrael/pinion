//! R1718 §5.12 §2 #7 — **a type that can put a sentence in front of a person
//! is driven by something**, for the JSON-RPC surface.
//!
//! Same census as `pinion-core`'s, pointed at this crate. It matters here for a
//! reason the framework's own types do not have: these sentences travel to an
//! AGENT as well as to a person, and a refusal that reads like another refusal
//! is one an agent cannot act on differently — §2 #2's whole point is that the
//! headless path is the primary one.

use std::path::PathBuf;

use pinion_core::external::RefusalReason;
use pinion_core::test_fixtures::speech::assert_speaks;
use pinion_core::test_fixtures::speech::census::assert_every_speaker_is_driven;
use pinion_core::utterance::{Tone, Urgency, Utterance};
use pinion_rpc::path::PathError;
use pinion_rpc::subscribe::SubscribeError;
use pinion_rpc::{InterveneError, InvokeError};

/// Every refusal the action channel can answer, named.
///
/// A `const` list rather than a `Vec` built in each test, so an arm added
/// upstream fails to be exhaustive here in one place — and so the three
/// properties below (wording, tone, urgency) are asserted over the *same*
/// population rather than three hand-kept copies of it.
const INVOKE_ARMS: &[(&str, InvokeError)] = &[
    ("Path", InvokeError::Path(PathError::MalformedPrefix)),
    ("UnsupportedPath", InvokeError::UnsupportedPath),
    ("NoExternalAtPath", InvokeError::NoExternalAtPath),
    ("IntrospectionOptedOut", InvokeError::IntrospectionOptedOut),
    ("UnknownInvokePath", InvokeError::UnknownInvokePath),
    ("PathIsAReadSlot", InvokeError::PathIsAReadSlot),
    ("InvokeTypeMismatch", InvokeError::InvokeTypeMismatch),
    ("DeclaredButUnhandled", InvokeError::DeclaredButUnhandled),
    ("UnmappedSurfaceError", InvokeError::UnmappedSurfaceError),
    (
        "RetainedNodeNotWritable",
        InvokeError::RetainedNodeNotWritable,
    ),
    (
        "InvokeRejected",
        InvokeError::InvokeRejected(RefusalReason::stated("no detector is installed")),
    ),
];

/// The write channel's peer of [`INVOKE_ARMS`].
const INTERVENE_ARMS: &[(&str, InterveneError)] = &[
    ("Path", InterveneError::Path(PathError::MalformedPrefix)),
    ("UnsupportedPath", InterveneError::UnsupportedPath),
    ("NoExternalAtPath", InterveneError::NoExternalAtPath),
    (
        "IntrospectionOptedOut",
        InterveneError::IntrospectionOptedOut,
    ),
    ("UnknownIntervenePath", InterveneError::UnknownIntervenePath),
    ("PathIsAnAction", InterveneError::PathIsAnAction),
    (
        "InterveneTypeMismatch",
        InterveneError::InterveneTypeMismatch,
    ),
    ("ReadOnly", InterveneError::ReadOnly),
    ("UnmappedSurfaceError", InterveneError::UnmappedSurfaceError),
    (
        "RetainedNodeNotWritable",
        InterveneError::RetainedNodeNotWritable,
    ),
    (
        "OutOfRange",
        InterveneError::OutOfRange(RefusalReason::stated("0 is below the first row")),
    ),
];

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

/// ★★★★★ R1720 — every refusal the ACTION channel can answer is said to the
/// person, and no two of them read alike.
///
/// The wire already had a word for each of these; a word is what an agent
/// matches on, and `"PathIsAReadSlot"` is not a thing to put in front of
/// somebody. What made this urgent is that the framework now *shows* one of
/// these on every refusal: before R1720 an unsaid arm was merely unsaid, and
/// now it is what a person reads.
#[test]
fn r1720_every_invoke_refusal_is_said_and_distinct() {
    let said: Vec<(&str, String)> = INVOKE_ARMS
        .iter()
        .map(|(name, err)| (*name, err.said().sentence()))
        .collect();
    assert_speaks("InvokeError", 11, &said, &[]);
}

/// ★★★★★ R1720 — the same for the WRITE channel, which refuses for its own
/// reasons and is reached by the same seam.
#[test]
fn r1720_every_intervene_refusal_is_said_and_distinct() {
    let said: Vec<(&str, String)> = INTERVENE_ARMS
        .iter()
        .map(|(name, err)| (*name, err.said().sentence()))
        .collect();
    assert_speaks("InterveneError", 11, &said, &[]);
}

/// ★★★★★ R1720 — **every one of them is framed as a refusal**, which is what
/// decides the urgency a screen reader is given.
///
/// Asserted separately from the wording because it is a different property and
/// it can fail on its own: a sentence composed in the wrong tone reads fine and
/// is announced politely, so a person who cannot see the screen is told about a
/// thing that did not happen *when they are next idle*.
#[test]
fn r1720_a_refusal_is_said_in_the_refused_tone_on_both_channels() {
    for (name, err) in INVOKE_ARMS {
        assert_eq!(err.said().tone(), Tone::Refused, "InvokeError::{name}");
        assert_eq!(
            err.said().urgency(),
            Urgency::Interrupting,
            "InvokeError::{name}"
        );
    }
    for (name, err) in INTERVENE_ARMS {
        assert_eq!(err.said().tone(), Tone::Refused, "InterveneError::{name}");
    }
}

/// ★★★★★ R1720 — a producer's sentence that **cannot be said** does not stop
/// the process, and does not reach the person either.
///
/// The one arm of each channel this crate does not author carries the surface's
/// own words, and an agent's argument is interpolated into those words at many
/// producers — so a panic here would be a way to stop an application from the
/// wire. Each of the three faults is driven, because a fallback nobody drives
/// is a fallback that can say anything (R1718's own finding about this crate).
#[test]
fn r1720_an_unsayable_reason_is_replaced_rather_than_panicking() {
    let unsayable = [
        ("Empty", "   "),
        ("AlreadyFramed", "refused: no such card"),
        ("DebugSpelling", "Rejected(RefusalReason(\"no such card\"))"),
    ];
    for (fault, reason) in unsayable {
        assert!(
            Utterance::checked(Tone::Refused, reason).is_err(),
            "{fault}: this input must be unsayable or the test below proves nothing"
        );
        let by_action = InvokeError::InvokeRejected(reason.into()).said();
        let by_write = InterveneError::OutOfRange(reason.into()).said();
        assert_eq!(
            by_action.clause(),
            "this surface refused, and its reason cannot be shown",
            "{fault}"
        );
        assert_eq!(by_write.clause(), by_action.clause(), "{fault}");
        assert!(
            !by_action.clause().contains("card"),
            "{fault}: the words that could not be shown must not be shown"
        );
    }
    // And a sayable one is forwarded verbatim — otherwise the arm above would
    // be satisfied by never forwarding anything.
    let plain = InvokeError::InvokeRejected("no card \"R-99\" on this board".into()).said();
    assert_eq!(plain.clause(), "no card \"R-99\" on this board");
    assert_eq!(plain.sentence(), "refused: no card \"R-99\" on this board");
}

/// ★★★★ R1718 — and nothing else in this crate speaks without being driven.
#[test]
fn every_speaking_type_in_this_crate_is_driven_by_the_speech_gate() {
    assert_every_speaker_is_driven(PathBuf::from(env!("CARGO_MANIFEST_DIR")), 3);
}
