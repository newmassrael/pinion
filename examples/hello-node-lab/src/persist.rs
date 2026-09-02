//! R1689 — **the graph survives being closed**, and the screen declares which
//! of what it holds is carried and which is deliberately not.
//!
//! # The cluster a self-census cannot see
//!
//! The reference publishes its own list of this screen's operations and even
//! measures its coverage of it — thirty entries, which `spec::OPERATIONS`
//! mirrors one-for-one and which R1688 finished. Saving is not on that list.
//! It is nonetheless *in* the reference: a save, a load, an import from pasted
//! text and a reset-to-default, plus a meter of its own that asks whether every
//! piece of state is either carried or declared volatile.
//!
//! So a census taken over a declared list is complete against that list and
//! blind to everything the list leaves out — the same shape as R1688's finding
//! one level up, where a census of what is *on screen* could not see what the
//! screen cannot do. This module is that cluster.
//!
//! # ★★ What is saved is a partition, not a list
//!
//! [`spec::KEPT`](crate::spec::KEPT) says of **every** introspection slot an
//! operation can move whether a save carries it. The gate then asserts three
//! things, and the third is the one with teeth:
//!
//! 1. the partition covers exactly the slots the operations name, in both
//!    directions — a slot nobody classified, and a classification for a slot
//!    nothing moves, are both failures;
//! 2. drive each operation, save, put the screen back, load — and the slot it
//!    moves reads what it read *after* the operation;
//! 3. for a slot declared volatile, the same run asserts it reads what it read
//!    **before** — so "we deliberately do not keep this" is checked rather than
//!    asserted.
//!
//! The reference's own meter only asks (1). A key can be classified as carried
//! and still not come back.
//!
//! # What is not here, and why
//!
//! **The screen does not restore itself when it opens.** The reference does,
//! from browser storage. Doing that here would make every gate in this example
//! a function of whatever is on the machine that runs it — which
//! [[zero-flake-policy]] refuses, and which the sibling node editor refused for
//! the same reason in R852 ("no surprise auto-load"). It is a deliberate
//! divergence and is registered as one.

use std::rc::Rc;

use pinion_core::Storage;
use pinion_core::selection::Selection;
use pinion_core::utterance::{Tone, Utterance};
use pinion_core::widgets::config_form::ConfigForm;
use pinion_node_graph::{Archive, Condition, Document, NodeId};
use serde::{Deserialize, Serialize};

use crate::graph::LabNode;
use crate::{LabState, Placement};

/// The one [`Storage`] key the whole graph is written under — one blob, so the
/// file backend's tempfile-and-rename covers the whole save.
pub const STORAGE_KEY: &str = "node_lab.graph";

/// The per-OS data directory this screen's saves live in.
///
/// Named under a `cfg` because the test build never reaches the file backend —
/// see `crate::app_storage` for why, and for where the file half IS proven.
#[cfg_attr(
    test,
    allow(
        dead_code,
        reason = "the test build stores in memory; the demo covers the real directory"
    )
)]
pub const STORAGE_APP: &str = "pinion-node-lab";
/// The cache key the shared storage hook is parked under.
pub const STORAGE_CACHE_KEY: &str = "node_lab.storage";

/// What this screen keeps beside its graph.
///
/// Everything here is state the *document* cannot hold: a card's form is the
/// screen's editor over a configuration, the host names are this screen's
/// reading of its frames, and the opening placements are the baselines the
/// reset affordances measure against. The graph itself — the cards, the links,
/// the containment, the positions, which cards are collapsed and which are
/// switched off — is in the document, where the substrate can check it.
///
/// ★ Each map is written as a **list of pairs** rather than a map keyed by
/// [`NodeId`]: an id is a newtype over a number, JSON object keys are strings,
/// and a map whose key encoding depends on the serialiser's opinion about
/// integers is a file format decided by accident.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    /// Each card's settings form, as it was left.
    pub forms: Vec<(NodeId, ConfigForm)>,
    /// Which host frame each card sits in, by the name the canvas shows.
    pub frames: Vec<(NodeId, String)>,
    /// Where each card came into being — the baseline the resets restore to.
    pub opened_at: Vec<(NodeId, Placement)>,
    /// The master auto-discovery switch.
    pub discovery: bool,
    /// What was selected, by NAME rather than by id.
    ///
    /// ★ The archive carries a selection of its own and this does not use it,
    /// which is a decision worth stating: the crate's selection is a list of
    /// [`NodeId`]s and it checks them against the document it came with, which
    /// is exactly right — but this screen's inspector is driven by *the card
    /// the canvas has picked*, and every other place it names a card it names
    /// it the way a person reads it. Carrying both would be two records of one
    /// fact. The archive's own is written too, so a reader that is not this
    /// screen still finds a selection where it expects one.
    pub selected: Option<String>,
}

/// The archive this screen would write right now.
pub fn archive_of(state: &LabState) -> Archive<LabNode, Kept> {
    let document: Document<LabNode> = state.doc.borrow().clone();
    let selected = state.active_card().map(|node| state.name_of(node));
    Archive::of(document)
        .with_camera(crate::camera_now(state))
        .with_selection(state.active_card())
        .with_companion(Kept {
            forms: state
                .forms
                .borrow()
                .iter()
                .map(|(id, form)| (*id, form.clone()))
                .collect(),
            frames: state
                .frames
                .borrow()
                .iter()
                .map(|(id, name)| (*id, name.clone()))
                .collect(),
            opened_at: state
                .opened_at
                .borrow()
                .iter()
                .map(|(id, at)| (*id, at.clone()))
                .collect(),
            discovery: state.discovery.get(),
            selected,
        })
}

/// The archive as text — the `graph` read, and what a save writes.
pub fn graph_text(state: &LabState) -> String {
    archive_of(state).write().unwrap_or_default()
}

/// Put an archive on the screen.
///
/// ★ The camera is applied through [`crate::point_canvas_at`], the same
/// function the fit and the steppers use, so a restored view is anchored the
/// way every other view change on this screen is. A camera the archive dropped
/// leaves the view where it is rather than moving it somewhere arbitrary.
fn install(state: &Rc<LabState>, archive: Archive<LabNode, Kept>) {
    let (document, camera, _selection, kept) = archive.into_parts();
    *state.doc.borrow_mut() = document;
    if let Some(kept) = kept {
        *state.forms.borrow_mut() = kept.forms.into_iter().collect();
        *state.frames.borrow_mut() = kept.frames.into_iter().collect();
        *state.opened_at.borrow_mut() = kept.opened_at.into_iter().collect();
        state.discovery.set(kept.discovery);
        // ★★ R1706 — a save carries the LEADER and a restore collapses the
        // selection to it. Not an omission: the behaviour canon marks its own
        // member list volatile and rebuilds it as `[leader]` on restore, for a
        // reason this screen shares — a group selection is something a person
        // is holding *right now*, and handing it back with a file they opened
        // an hour later would put six highlighted cards on screen that nobody
        // picked. The leader is what the inspector shows, so restoring it is
        // what makes the panel look the way it was left.
        state.selection.set(
            kept.selected
                .and_then(|name| state.node_of(&name))
                .map_or_else(Selection::empty, Selection::one),
        );
    }
    // ★ R1961 — a restored graph is put back in step with its OWN addresses.
    // The transport a card speaks is derived (from its listen endpoint, or from
    // the address it dials), so an archive is trusted for the addresses and the
    // wires and never for the answer read off them — which is what keeps a file
    // written before this derivation existed from painting a stale colour.
    crate::settle_transports(state);
    // Everything below is declared VOLATILE, and a load is where that has to be
    // acted on rather than merely written down: a link picked in the graph that
    // was on screen a moment ago names an id this document may not have.
    state.selected_link.set(None);
    state.drag.set(None);
    state.editing.set(None);
    // ★★ The artifacts go, and this is [`crate::spec::KEPT`]'s own reasoning
    // acted on rather than restated: an exported configuration belongs to the
    // moment it was taken. Leaving it beside a graph that has just been
    // replaced would put a document on screen that describes something no
    // longer there — which is worse than having none, because it looks current.
    *state.produced.borrow_mut() = crate::Produced::default();
    if let Some(camera) = camera {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a zoom the archive kept is a positive finite scale; the \
                      percentage is clamped into the screen's range"
        )]
        let percent = (camera.zoom * 100.0).round() as u32;
        // The middle: a restore is not a gesture, so there is no cursor to hold
        // and the rounding correction belongs where a fit's does.
        crate::point_canvas_at(state, percent, camera, crate::canvas_middle());
    }
}

/// Write the graph to storage, and say what happened.
pub fn save(state: &Rc<LabState>) -> String {
    let text = graph_text(state);
    // ★★★ R1719 — one `if` whose two arms are two KINDS of thing, which is
    // what this site looked like before the tone was a value: both branches
    // built a `String` and handed it to the same setter, so a person was told
    // the save failed in the same voice and the same colour as the save
    // succeeding.
    let said = if text.is_empty() {
        Utterance::new(Tone::Refused, "the graph could not be written out")
    } else {
        state.storage.save(STORAGE_KEY, text.as_bytes());
        Utterance::done(format!(
            "saved · {} cards · {} bytes",
            state.cards().len(),
            text.len()
        ))
    };
    state.say(said.clone());
    said.sentence()
}

/// Open a graph: from `text` when there is any, otherwise from storage.
///
/// One verb with an optional argument, which is the reference's own shape — its
/// button asks for JSON and takes the saved copy when the box is left empty.
/// Two verbs would be two paths to one act, and the second would be the one
/// that drifts.
///
/// # Errors
///
/// The sentence to put in front of whoever asked, with the screen unchanged.
pub fn open(state: &Rc<LabState>, text: &str) -> Result<String, String> {
    let owned;
    let text = if text.trim().is_empty() {
        owned = state
            .storage
            .load(STORAGE_KEY)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default();
        owned.as_str()
    } else {
        text
    };
    let opening = Archive::<LabNode, Kept>::read(text);
    // ★★★★★ R1977 — **two failures, and only one of them is "there is no
    // document".**
    //
    // This asked `Opening::reason()`, which folds both, and refused on either.
    // Measured at R1977 by driving it: a saved graph whose link names a node
    // that is not there answered *the graph is not sound in 2 ways, starting
    // with: link 0 in tree 0 names a socket that is not there* and the screen
    // stayed where it was — so a person whose file had gone structurally wrong
    // could see one sentence and NOTHING ELSE, on a screen that (since R1976)
    // can say which card each fault is on.
    //
    // The two are not the same kind of failure:
    //
    // * **Unreadable** — not the envelope, another revision, a taxonomy this
    //   build does not have. There is no document to show. Still refused.
    // * **Violations** — the document parsed and its own invariants do not
    //   hold. There IS a document, and looking at it is exactly how a person
    //   repairs it.
    //
    // `Opening::take_despite_violations` exists for this and had ZERO callers,
    // in this tree and in the crate's own tests — the crate built the door and
    // nobody opened it. This is the caller.
    //
    // ⚠ Opening it is not running it: `Document::review` reports every one of
    // these as a structural fault, `Fitness::Stopped` follows, and the launch
    // stays shut (R1976). So the graph is visible and still cannot be started,
    // which is R1689's own rule — *`Dropped` is not a failure, the graph is
    // here* — applied to the half that was refusing whole documents.
    //
    // ★ The behaviour canon is the same shape and weaker: its import checks the
    // snapshot's SHAPE and says *loaded*, and its validation pass then looks at
    // field values only — it has no structural axis, so it opens such a graph
    // and never mentions it.
    //
    // ★★★★★ R1978 — and the split is now the CRATE'S, matched here rather than
    // worked out here. R1977 asked whether there were violations and fell
    // through to `Opening::reason`, which is a re-derivation of a rule the crate
    // already owns; `Condition` hands back the three-way as one value, so this
    // screen states its policy — *unreadable is refused, unsound is opened* —
    // and a fourth answer added upstream stops this compiling instead of
    // arriving here as "fine".
    let unsound: Vec<String> = match opening.condition() {
        Condition::Unreadable(why) => {
            let why = why.to_string();
            state.say(Utterance::refused(&why));
            return Err(why);
        }
        Condition::Unsound(violations) => violations
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        Condition::Sound => Vec::new(),
    };
    // ★ What the archive could not carry is said out loud, because this is the
    // one moment a person can act on it. `Dropped` is not a failure — the graph
    // is here — and a screen that swallowed it would be the placeholder the
    // reference toolkit logs behind a category nobody has switched on.
    let dropped: Vec<String> = opening
        .dropped()
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    let archive = opening
        .take_despite_violations()
        .ok_or_else(|| "unreadable".to_owned())?;
    install(state, archive);
    // ★★★★★ R1977 — the sentence names BOTH kinds of remainder, and an unsound
    // graph is named as unsound rather than as "opened". A person who sees
    // `opened` and nothing else on a broken file has been told the wrong thing;
    // the gate will say which card each fault is on, and this is what sends
    // them to look.
    let mut clauses = vec![format!("{} cards", state.cards().len())];
    if !dropped.is_empty() {
        clauses.push(format!(
            "{} left behind: {}",
            dropped.len(),
            dropped.join(" · ")
        ));
    }
    if !unsound.is_empty() {
        clauses.push(format!(
            "{} fault(s) the gate will name: {}",
            unsound.len(),
            unsound.join(" · ")
        ));
    }
    // ⚠ `Tone::Done`, and the clause is what carries the trouble. The first
    // draft reached for a `warned` tone; measured, `Tone` has three arms and no
    // such thing, and adding one would be this screen inventing a vocabulary
    // several gates count. `Done` is also the truthful arm: the open HAPPENED,
    // which is exactly the change this round makes — what is not true of the
    // graph is in the sentence, and the launch gate is what refuses to run it.
    let said = Utterance::done(format!("opened · {}", clauses.join(" · ")));
    state.say(said.clone());
    Ok(said.sentence())
}

/// Put the whole screen back to the graph it opens with, and forget the save.
///
/// The reference's third button. It is not one of the five scoped resets — each
/// of those puts ONE thing back and is offered only while there is something to
/// put back — but the one act that discards everything, including what is on
/// disk. A reset that left a saved copy behind would be a reset a reload undoes.
pub fn clear(state: &Rc<LabState>) -> String {
    state.storage.remove(STORAGE_KEY);
    for scope in crate::ResetScope::ALL {
        scope.apply(state);
    }
    let said = Utterance::done("back to the graph this screen opens with");
    state.say(said.clone());
    said.sentence()
}

/// What storage holds, or an empty string when nothing has been saved.
///
/// A read rather than a boolean: an agent that wants to know whether a save
/// took also wants to be able to read what it wrote, and the reference puts the
/// same text on its console for the same reason.
pub fn stored(state: &LabState) -> String {
    state
        .storage
        .load(STORAGE_KEY)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}
