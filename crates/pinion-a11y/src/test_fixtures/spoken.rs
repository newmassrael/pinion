//! ★★★★★ R2069 §5.40 §5.12 — **the voice census as a reader actually gets it**,
//! built once instead of six times.
//!
//! # The defect this exists for, measured
//!
//! Asking "does every addressable region of this screen either speak or say why
//! it is quiet" takes four steps:
//!
//! 1. paint the screen (the screen's own — `painted_at`, `view` + layout);
//! 2. build its accessibility tree (the screen's own — `access_node`);
//! 3. **[`enrich_names_from_scene`]**, which fills in the names a node leaves to
//!    the paint;
//! 4. [`announcements`] + [`referenced_tags`] → [`voice_census`].
//!
//! Steps 1 and 2 differ per screen and stay there. Steps 3 and 4 are the same
//! everywhere and were **hand-copied into six places** — five screens and the
//! shell that assembles four of them — which is what
//! `debt-five-screens-hand-roll-one-voice-gate` counts. Measured again at
//! R2068, still six, and `test_fixtures` held nothing that wrapped them.
//!
//! ★★★★★ **Step 3 is the dangerous one, and that is why it is not a separate
//! call here.** Omitting it does not make the gate green — it makes it FALSE
//! RED: R1867's first draft reported **90 `mumbled` regions**, every one
//! `name Some("")`, across four destinations, none of which a running window
//! has. A node may leave its name to the painted header on purpose
//! (`grid_table_nodes` says so at the site), so a tree read before enrichment
//! is not the tree a reader receives. Four of the six copies carry a
//! hand-written warning saying so, beside the call — one rule written five
//! times, which is the shape [[three-site-internal-duplication-substrate-lift]]
//! exists to end.
//!
//! # What this does NOT lift, and why
//!
//! **Not the loop, and not the judgement.** Each screen's second axis differs —
//! the shell asks about its status band's two occupants, the node lab sweeps two
//! sizes across seventeen states, the others ask one opening state — and a
//! helper that owned the loop would take that axis away.
//!
//! ★★★★★ And the `derived` count is **returned, never judged**: `hello-packet-
//! view` asserts it is `> 0` (its name redirects point at something) and
//! `hello-node-lab` asserts it is `== 0` (that screen names everything itself).
//! Two screens assert opposite directions of one number, so a helper that
//! asserted either would be wrong for one of them. [`enrich_names_from_scene`]
//! already answers it, so this only has to pass it on.
//!
//! # Why this lives in `pinion-a11y` and not in `pinion-core::test_fixtures`
//!
//! The debt's own prescription said "lift it into `test_fixtures`" without
//! naming the crate, and `pinion-core`'s fixtures module refuses: it keeps that
//! crate's `pinion-a11y` dependency direction **empty** as a cycle invariant,
//! stated in its own header. Three of the four steps are this crate's. So the
//! fixture belongs on this side of that boundary, which is the same resolution
//! [`super`]'s own header records for the orphan rule one axis over.

use pinion_core::scene::Scene;
use pinion_core::voice::{VoiceCensus, voice_census};

use crate::AccessNode;
use crate::node::{announcements, referenced_tags};
use crate::scene_label::enrich_names_from_scene;

/// What a screen says, as a reader gets it.
#[derive(Debug)]
pub struct Spoken {
    /// Every addressable region, classified.
    pub census: VoiceCensus,
    /// How many names [`enrich_names_from_scene`] filled in from the paint.
    ///
    /// Carried rather than checked: a screen that names everything itself owes
    /// zero here, and one whose nodes point at painted headers owes more than
    /// zero, and both are correct for the screen that says so.
    pub derived: usize,
}

/// Steps 3 and 4, together, so step 3 cannot be forgotten.
///
/// Takes the scene and the tree the screen built — steps 1 and 2, which are the
/// screen's own — and answers the census a reader would meet.
#[must_use]
pub fn census(scene: &Scene, nodes: &mut [AccessNode]) -> Spoken {
    let derived = enrich_names_from_scene(nodes, scene);
    let announced = announcements(nodes);
    let referenced = referenced_tags(nodes);
    Spoken {
        census: voice_census(scene, &announced, &referenced),
        derived,
    }
}
