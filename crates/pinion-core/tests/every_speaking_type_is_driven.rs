//! R1718 §5.12 §2 #7 — **a type that can put a sentence in front of a person
//! is driven by something**, for this crate.
//!
//! The census itself lives in
//! [`pinion_core::test_fixtures::speech::census`], because three crates need
//! it: this one, the JSON-RPC surface, and the analysis tool's node-graph
//! screen. Here it is pointed at this crate's `src/`.
//!
//! Measured at R1718, before it existed: of the wordings this workspace can put
//! in front of a person, **11 of 39 matched any search at all**, and the checks
//! that did exist were substring probes over fragments — which say nothing
//! about whether a sentence reads. The launch verdict a screen paints, the
//! take-over toast, two of three configuration defects and every reason a shut
//! affordance gives a screen reader were among the unread.

use std::path::PathBuf;

use pinion_core::test_fixtures::speech::census::assert_every_speaker_is_driven;

/// ★★★★★ R1718 — every type in `pinion-core` that speaks to a person is
/// driven by the speech gate, and every drive names a type that can speak.
#[test]
fn every_speaking_type_in_this_crate_is_driven_by_the_speech_gate() {
    // Nine at the time of writing: the configuration form's four, the text
    // judgement, the unavailability reason, the destination detour, and the
    // schema's reach and mistyping. The floor is deliberately below that — this
    // guard is against a scan that reads nothing, not a second census.
    assert_every_speaker_is_driven(PathBuf::from(env!("CARGO_MANIFEST_DIR")), 6);
}
