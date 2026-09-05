//! ★★★★★ R2008 — **two people changed this graph, and the screen says where
//! their changes meet.**
//!
//! # What this is, and why it belongs on THIS screen
//!
//! The reference's script editor reaches a merge from source control: a base
//! (the version both sides started from), a remote (what arrived) and a local
//! (what is on screen), plus a callback for the resolution. Its view takes the
//! union of graph paths, records per path which of the three hold it, diffs
//! each side against the base, and joins the two difference lists.
//!
//! This screen already saves and opens a graph ([`crate::persist`]), so both
//! of the other two versions are things it can already hold: a base is the
//! document as it stands at the moment somebody says *this is where we both
//! started*, and a peer is an archive that arrived from somewhere else — the
//! same text `open` takes, read WITHOUT replacing what is on the canvas.
//!
//! # ★★★★★ Derived on every read, never latched
//!
//! [`merging_wire`] runs the three-way each time it is asked, so a person
//! resolving a conflict by editing the canvas watches the conflict list shrink.
//! A latched result would be a second copy of an answer the document already
//! carries, and the moment it disagreed with the canvas the screen would be
//! showing a merge of a graph nobody has.
//!
//! ⚠ **What this does NOT do, stated rather than left to be discovered: it does
//! not apply anything.** The reference's tool writes a merged asset back; this
//! reports. Applying is a separate act with its own refusals (which side wins a
//! conflict is a person's decision, and there is no verb here that takes it),
//! and building the report first is what makes that act's arguments knowable.

use std::rc::Rc;

use pinion_core::Storage;
use pinion_core::utterance::Utterance;
use pinion_node_graph::{Archive, Condition, Document, Meet, Merged, Subject, What};

use crate::LabState;
use crate::graph::LabNode;
use crate::persist::{self, STORAGE_KEY};

/// The three versions a merge needs, once a person has named them.
///
/// `local` is never here: it is the document on the canvas, which is the whole
/// reason this screen is where the merge lives.
#[derive(Default)]
pub struct Sides {
    /// The version both sides started from.
    pub base: Option<Document<LabNode>>,
    /// The version that arrived from somewhere else.
    pub peer: Option<Document<LabNode>>,
}

/// ★★★★★ R2008 — remember the graph as it stands as the version both sides
/// started from.
///
/// **The peer is dropped with it**, and that is the point rather than tidiness:
/// a peer is a set of changes *against a base*, so keeping one across a new
/// base would report differences nobody made.
pub fn keep_base(state: &Rc<LabState>) -> String {
    let document = state.doc.borrow().clone();
    let cards = document
        .trees()
        .map(|tree| tree.nodes().count())
        .sum::<usize>();
    *state.sides.borrow_mut() = Sides {
        base: Some(document),
        peer: None,
    };
    let said = Utterance::done(format!(
        "base kept · {cards} card(s) · nothing has arrived to merge yet"
    ));
    state.say(said.clone());
    said.sentence()
}

/// ★★★★★ R2008 — read a version that arrived from somewhere else, WITHOUT
/// putting it on the canvas.
///
/// The same argument shape [`crate::persist::open`] has, and the reference's
/// own: text when there is any, otherwise whatever storage holds.
///
/// ⚠ **An unsound peer is taken anyway**, which is R1977's rule reaching this
/// verb: there IS a document, and comparing it is exactly how somebody works
/// out what the other person did to break it. Only an unreadable one is
/// refused, because there is nothing to compare.
///
/// # Errors
///
/// The sentence to put in front of whoever asked, with the screen unchanged.
pub fn take_peer(state: &Rc<LabState>, text: &str) -> Result<String, String> {
    if state.sides.borrow().base.is_none() {
        // ★ Refused with the reason, not silently accepted: a peer with no base
        // is two documents and no way to tell who changed what. Naming the verb
        // that fixes it is what makes the refusal actionable (R1706).
        let why = "no base has been kept — say `keep_base` first, then bring a version in";
        state.say(Utterance::refused(&why));
        return Err(why.to_owned());
    }
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
    let opening = Archive::<LabNode, persist::Kept>::read(text);
    let unsound = match opening.condition() {
        Condition::Unreadable(why) => {
            let why = why.to_string();
            state.say(Utterance::refused(&why));
            return Err(why);
        }
        Condition::Unsound(violations) => violations.len(),
        Condition::Sound => 0,
    };
    let archive = opening
        .take_despite_violations()
        .ok_or_else(|| "unreadable".to_owned())?;
    let (document, ..) = archive.into_parts();
    let cards = document
        .trees()
        .map(|tree| tree.nodes().count())
        .sum::<usize>();
    state.sides.borrow_mut().peer = Some(document);
    let mut clauses = vec![format!("{cards} card(s)")];
    if unsound > 0 {
        clauses.push(format!("{unsound} fault(s) it arrived with"));
    }
    let said = Utterance::done(format!("version taken in · {}", clauses.join(" · ")));
    state.say(said.clone());
    Ok(said.sentence())
}

/// ★★★★★ R2008 — the three-way, run now.
///
/// `null` for `meetings` and the rest until both other versions are named,
/// because *nothing has been compared* and *the comparison found nothing* are
/// different facts and a screen that spelled them the same way would tell a
/// person their graphs agree when nobody has looked.
pub fn merging_wire(state: &Rc<LabState>) -> serde_json::Value {
    let sides = state.sides.borrow();
    let (Some(base), Some(peer)) = (sides.base.as_ref(), sides.peer.as_ref()) else {
        return serde_json::json!({
            "base": sides.base.is_some(),
            "peer": sides.peer.is_some(),
            "trees": serde_json::Value::Null,
            "remote": serde_json::Value::Null,
            "local": serde_json::Value::Null,
            "meetings": serde_json::Value::Null,
            "conflicts": serde_json::Value::Null,
            "clean": serde_json::Value::Null,
        });
    };
    let merged: Merged = base.merge_from(peer, &state.doc.borrow());
    serde_json::json!({
        "base": true,
        "peer": true,
        // ★ The reference's view draws exactly these three boxes per graph
        // path, and this is the same table. `removed_by_one` is published
        // beside them because it is the one shape a merge cannot settle alone
        // and a reader would otherwise have to re-derive it from the flags.
        "trees": merged.trees.iter().map(|(tree, how)| serde_json::json!({
            "tree": tree.0,
            "in_base": how.base,
            "in_peer": how.remote,
            "here": how.local,
            "added_by_one": how.added_by_one(),
            "removed_by_one": how.removed_by_one(),
        })).collect::<Vec<_>>(),
        "remote": merged.remote.iter().map(|held| change_wire(state, held)).collect::<Vec<_>>(),
        "local": merged.local.iter().map(|held| change_wire(state, held)).collect::<Vec<_>>(),
        // ★★★★★ One entry per SUBJECT both sides touched, which is where the
        // reference stops at the first pair it finds.
        "meetings": merged.meetings.iter().map(|met| serde_json::json!({
            "tree": met.tree.0,
            "at": subject_wire(state, met.tree, met.at),
            "peer": what_word(met.remote),
            "here": what_word(met.local),
            "meet": match met.meet {
                Meet::Agreed => "agreed",
                Meet::Harmless => "harmless",
                Meet::Conflict => "conflict",
            },
        })).collect::<Vec<_>>(),
        "conflicts": merged.conflicts().len(),
        // ★ Not `conflicts == 0`: a tree one side removed and the other kept
        // makes a merge unclean with no meeting to carry it, so a client
        // deriving this from the count above would call that clean.
        "clean": merged.is_clean(),
    })
}

/// One change, as a client reads it.
fn change_wire(state: &Rc<LabState>, held: &pinion_node_graph::Change) -> serde_json::Value {
    serde_json::json!({
        "tree": held.tree.0,
        "at": subject_wire(state, held.tree, held.at),
        "what": what_word(held.what),
        // ★ Published rather than left to a client's table of words: whether a
        // change alters what the graph MEANS is the crate's judgement, and a
        // screen re-deriving it from the word would be a second copy of the
        // rule that decides every conflict.
        "structural": held.what.structural(),
    })
}

/// A subject, named the way the rest of this screen names things.
///
/// ⚠ A card the local document does not have — one the peer added, or one this
/// side removed — has no name here, so the id is published beside the name and
/// the name is `null`. Inventing one would be a screen making up a card.
fn subject_wire(
    state: &Rc<LabState>,
    tree: pinion_node_graph::TreeId,
    at: Subject,
) -> serde_json::Value {
    match at {
        Subject::Node(node) => serde_json::json!({
            "kind": "card",
            "id": node.0,
            "name": state
                .doc
                .borrow()
                .tree(tree)
                .and_then(|host| host.node(node))
                .map(|_| state.name_of(node)),
        }),
        Subject::Link(link) => serde_json::json!({
            "kind": "wire",
            "id": link.0,
            "name": serde_json::Value::Null,
        }),
    }
}

/// The word for a change, as this screen spells it.
const fn what_word(what: What) -> &'static str {
    match what {
        What::Added => "added",
        What::Removed => "removed",
        What::Rewritten => "rewritten",
        What::Moved => "moved",
        What::Renamed => "renamed",
        What::Restyled => "restyled",
    }
}
