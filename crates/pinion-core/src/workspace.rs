//! R1893 §5.49 §2 #7 — **named arrangements**: a set of saved layouts a caller
//! can list, apply, save into and delete from, where each one says whether the
//! application shipped it.
//!
//! # What was duplicated
//!
//! Two consumers in this tree had already built this by hand, each holding its
//! own map from a name to whatever it calls a layout: the dock preset manager
//! stores serialised topologies, and the analysis shell stores a board grid
//! plus the cards on it. Measured at R1893, neither `pinion-core` nor
//! `pinion-widget-paint` had any type for the *set* — so the rules about it
//! (what a missing name does, whether a shipped arrangement can be deleted)
//! existed once per consumer or not at all.
//!
//! The layout itself stays the consumer's: this is generic over `L` precisely
//! because the two consumers disagree about what a layout is, and a shared type
//! that forced them to agree would be the wrong abstraction. What they DO agree
//! about is the set and its rules.
//!
//! # ★★★★★ The rule that needed a home: not every arrangement is a person's
//!
//! An application ships arrangements — the analysis tool opens on one, and the
//! behaviour canon offers four before a person has saved anything. A person
//! also saves their own. Those are the same kind of thing to a menu and
//! **different kinds of thing to a delete**: deleting one of yours is undoing
//! your own work, and deleting one the application shipped is removing
//! something no gesture can bring back.
//!
//! Before this module nothing in the tree could tell them apart, so a `delete`
//! built on either consumer's map would have taken both. [`Provenance`] is that
//! distinction, and [`Workspaces::delete`] refuses a built-in **with a
//! sentence** rather than by returning nothing.
//!
//! # Where the floor stands, measured
//!
//! Built from the 6.11 install and run offscreen at R1893, against its main
//! window class:
//!
//! * An arrangement **round-trips as 126 opaque bytes**, and a blob that is not
//!   one is refused. So the *serialisation* is there and it is honest.
//! * Of **108 published members** (39 methods, 69 properties), **zero** name a
//!   named arrangement, and **zero** name a SET of them. There is one current
//!   arrangement and a caller who wants several keeps its own map — exactly the
//!   position both of this tree's consumers were in.
//! * ⇒ with no set, there is nothing to mark as shipped-with-the-application,
//!   so the built-in/saved distinction has no counterpart there at all.
//!
//! This module is therefore not parity work. It is the layer above what the
//! floor offers, and the reason to build it without waiting for a third
//! consumer is that the two existing ones already wrote its rules down
//! differently.

use std::collections::BTreeMap;

use crate::external::RefusalReason;

/// Who put an arrangement in the set.
///
/// Two variants and no third: an arrangement is in the set because the
/// application shipped it or because somebody saved it, and there is no useful
/// middle. A `Modified` or `Unknown` arm would be a value the delete rule could
/// not decide on, which is the one question this type exists to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provenance {
    /// The application shipped it. A person may apply it and may not delete it.
    BuiltIn,
    /// A person saved it. Theirs to overwrite and theirs to delete.
    Saved,
}

impl Provenance {
    /// The scene-as-data name — what a published entry carries, so a client
    /// showing a menu knows which rows offer a delete without trying one.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::Saved => "saved",
        }
    }

    /// Parse a wire name back to a provenance — the inverse of [`Self::as_str`].
    ///
    /// `None` for an unknown word rather than a default: a row whose provenance
    /// could not be read must fail loudly, because guessing `Saved` would make
    /// a shipped arrangement deletable and guessing `BuiltIn` would strand a
    /// person's.
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        match name {
            "built-in" => Some(Self::BuiltIn),
            "saved" => Some(Self::Saved),
            _ => None,
        }
    }

    /// Whether an arrangement of this provenance may be removed.
    ///
    /// A method rather than a match at each call site: the delete rule is this
    /// module's subject, and a consumer re-deciding it is how two callers come
    /// to disagree about whether a shipped layout is removable.
    #[must_use]
    pub const fn deletable(self) -> bool {
        matches!(self, Self::Saved)
    }
}

/// ★★★★★ R1895 — serialised as its wire name, so a stored set and a published
/// row cannot disagree about where an arrangement came from.
///
/// Hand-written rather than derived, for the reason
/// [`crate::detach::DetachHome`]'s is: a derive would spell the two words a
/// second time, and a provenance that persists as one word and publishes as
/// another is a shipped arrangement that reopens deletable.
impl serde::Serialize for Provenance {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Provenance {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::from_wire(&name).ok_or_else(|| {
            serde::de::Error::custom(format!("an arrangement is built-in or saved, not {name:?}"))
        })
    }
}

/// Why an operation on the set was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRefusal {
    /// No arrangement of that name.
    NoSuchArrangement {
        /// The name the caller asked for.
        asked: String,
        /// Every name the set holds, in order — what the caller could have said.
        known: Vec<String>,
    },
    /// That name belongs to an arrangement the application shipped.
    ///
    /// Raised by both `delete` and `save`: a built-in that can be overwritten
    /// is a built-in only until somebody saves over it, and then the set is
    /// lying about where that arrangement came from.
    BuiltIn {
        /// The name that is the application's.
        asked: String,
    },
    /// An arrangement with no name cannot be found again.
    Unnamed,
    /// ★★★★★ R1897 — a STORED set claimed an arrangement that is not a
    /// person's to have saved.
    ///
    /// Distinct from [`BuiltIn`](Self::BuiltIn), which is a live operation
    /// refused. This is a stored row rejected at restore time, and the two
    /// reasons it happens are worth telling apart from each other only by the
    /// sentence: a file that says `built-in` is claiming an authority it cannot
    /// have (storage is written by a session, and a session only ever stores
    /// what a person saved), while a file whose NAME this build now ships is
    /// simply out of date. Both end the same way — the build's arrangement
    /// stands — and both must be REPORTED rather than dropped, because a
    /// restore that silently discards rows is a restore nobody can debug.
    NotYours {
        /// The stored name that was refused.
        asked: String,
    },
}

impl WorkspaceRefusal {
    /// The sentence, in the vocabulary an `External` refusal already uses.
    ///
    /// Each names what would have worked, which is the shape this tree settled
    /// on: `no` is not actionable.
    #[must_use]
    pub fn reason(&self) -> RefusalReason {
        match self {
            Self::NoSuchArrangement { known, .. } if known.is_empty() => {
                RefusalReason::stated("no arrangement has been saved yet")
            }
            Self::NoSuchArrangement { known, .. } => {
                RefusalReason::from(format!("the saved arrangements are {}", known.join(", ")))
            }
            Self::BuiltIn { asked } => RefusalReason::from(format!(
                "{asked:?} is an arrangement this application ships; save your own under another name"
            )),
            Self::Unnamed => RefusalReason::stated("an arrangement needs a name to be found again"),
            Self::NotYours { asked } => RefusalReason::from(format!(
                "{asked:?} was restored from storage but this build ships an arrangement of that name; the build's stands"
            )),
        }
    }

    /// A short machine word for the wire, so a client branches without parsing
    /// the sentence.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::NoSuchArrangement { .. } => "no-such-arrangement",
            Self::BuiltIn { .. } => "built-in",
            Self::Unnamed => "unnamed",
            Self::NotYours { .. } => "not-yours",
        }
    }
}

/// One row of the set: an arrangement and where it came from.
///
/// ★ R1895 — serialisable when `L` is, because a consumer holding the set in a
/// reactive `Signal` needs that and the first consumer (holding it in a
/// `RefCell`) did not. Lifting a shape one consumer uses is a guess about the
/// second; the second is what turns the guess into a requirement.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Arrangement<L> {
    /// What the consumer calls a layout. This module never looks inside it.
    pub layout: L,
    /// Who put it here.
    pub provenance: Provenance,
}

/// A named set of arrangements.
///
/// Ordered by name, because the order a menu shows them in must not depend on
/// the order they were saved — two sessions that saved the same two layouts in
/// different orders would otherwise show different menus.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Workspaces<L> {
    entries: BTreeMap<String, Arrangement<L>>,
}

impl<L> Workspaces<L> {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Add an arrangement the application ships.
    ///
    /// A builder rather than a `save` variant, because shipping one is
    /// something the application does at construction and saving is something a
    /// person does at runtime; sharing one entry point would make "is this a
    /// built-in" a parameter a caller could get wrong.
    #[must_use]
    pub fn with_built_in(mut self, name: impl Into<String>, layout: L) -> Self {
        self.entries.insert(
            name.into(),
            Arrangement {
                layout,
                provenance: Provenance::BuiltIn,
            },
        );
        self
    }

    /// Every name, in menu order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// How many arrangements the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every row, in menu order — name, provenance and layout.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Arrangement<L>)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Where the arrangement called `name` came from, or `None` if there is no
    /// such arrangement.
    #[must_use]
    pub fn provenance(&self, name: &str) -> Option<Provenance> {
        self.entries.get(name).map(|a| a.provenance)
    }

    /// The arrangement called `name`, or a refusal naming what the set holds.
    ///
    /// # Errors
    ///
    /// [`WorkspaceRefusal::NoSuchArrangement`], carrying every name that would
    /// have worked.
    pub fn apply(&self, name: &str) -> Result<&L, WorkspaceRefusal> {
        self.entries.get(name).map(|a| &a.layout).ok_or_else(|| {
            WorkspaceRefusal::NoSuchArrangement {
                asked: name.to_owned(),
                known: self.names(),
            }
        })
    }

    /// Save `layout` under `name`, overwriting a previously saved one.
    ///
    /// # Errors
    ///
    /// [`WorkspaceRefusal::Unnamed`] for an empty name — an arrangement nobody
    /// can name again is one nobody can apply. [`WorkspaceRefusal::BuiltIn`]
    /// for a name the application ships: overwriting it would leave the set
    /// claiming a person's layout came with the application.
    pub fn save(&mut self, name: &str, layout: L) -> Result<(), WorkspaceRefusal> {
        let name = name.trim();
        if name.is_empty() {
            return Err(WorkspaceRefusal::Unnamed);
        }
        if self.provenance(name) == Some(Provenance::BuiltIn) {
            return Err(WorkspaceRefusal::BuiltIn {
                asked: name.to_owned(),
            });
        }
        self.entries.insert(
            name.to_owned(),
            Arrangement {
                layout,
                provenance: Provenance::Saved,
            },
        );
        Ok(())
    }

    /// Remove a saved arrangement.
    ///
    /// # Errors
    ///
    /// [`WorkspaceRefusal::NoSuchArrangement`] when there is none of that name,
    /// and [`WorkspaceRefusal::BuiltIn`] when it is one the application ships —
    /// the distinction this module exists for, because before it a delete built
    /// on either consumer's plain map would have taken both.
    pub fn delete(&mut self, name: &str) -> Result<L, WorkspaceRefusal> {
        // ★ Removed FIRST, and put back when the removal is refused.
        //
        // The obvious shape — read the provenance, then remove — leaves a
        // `remove` that "cannot" be `None`, and expressing that needs an
        // `expect` whose panic path no test can reach. Taking the entry out and
        // returning it instead has no such path at all, and it makes the
        // property the refusal owes explicit rather than implied: **a refusal
        // must not remove what it refused to remove.**
        let Some(entry) = self.entries.remove(name) else {
            return Err(WorkspaceRefusal::NoSuchArrangement {
                asked: name.to_owned(),
                known: self.names(),
            });
        };
        if entry.provenance == Provenance::BuiltIn {
            self.entries.insert(name.to_owned(), entry);
            return Err(WorkspaceRefusal::BuiltIn {
                asked: name.to_owned(),
            });
        }
        Ok(entry.layout)
    }
}

impl<L: Clone> Workspaces<L> {
    /// ★★★★★ R1897 — **what a session writes to storage: the saved ones, and
    /// only those.**
    ///
    /// Storing the built-ins too would look harmless and is not: they are the
    /// APPLICATION's, and a later build with different ones would find the old
    /// set on disk and resurrect it. What a person saved is theirs and has to
    /// come back; what the application ships comes back by being shipped.
    ///
    /// ⇒ this is the first thing [`Provenance`] buys beyond the delete rule,
    /// and it is why the distinction had to exist before persistence could.
    #[must_use]
    pub fn saved(&self) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter(|(_, a)| a.provenance == Provenance::Saved)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// Lay a `stored` set over the arrangements this build ships, answering
    /// every row it REFUSED.
    ///
    /// The build's arrangements stand: a stored row whose name this build now
    /// ships is out of date, and a stored row that claims to BE a built-in is
    /// claiming an authority storage cannot have. Both are refused by name.
    ///
    /// ⚠ **The refusals are returned, not logged and not dropped.** A restore
    /// that silently discards rows is a restore nobody can debug, and this
    /// tree's rule is that an unclassified thing is a red rather than a pass —
    /// so the caller receives the list and decides what to say about it.
    ///
    /// Returns the refusals in name order, which is the order they would be
    /// shown in.
    pub fn restore(&mut self, stored: Self) -> Vec<WorkspaceRefusal> {
        let mut refused = Vec::new();
        for (name, arrangement) in stored.entries {
            if arrangement.provenance != Provenance::Saved
                || self.provenance(&name) == Some(Provenance::BuiltIn)
            {
                refused.push(WorkspaceRefusal::NotYours { asked: name });
                continue;
            }
            self.entries.insert(name, arrangement);
        }
        refused
    }
}

#[cfg(test)]
mod tests {
    use super::{Provenance, WorkspaceRefusal, Workspaces};

    fn seeded() -> Workspaces<&'static str> {
        Workspaces::new()
            .with_built_in("Overview", "board:overview")
            .with_built_in("Capture", "board:capture")
    }

    #[test]
    fn a_built_in_arrangement_applies_like_any_other() {
        let ws = seeded();
        assert_eq!(ws.apply("Overview"), Ok(&"board:overview"));
        assert_eq!(ws.provenance("Overview"), Some(Provenance::BuiltIn));
        assert_eq!(ws.len(), 2);
    }

    #[test]
    fn a_missing_name_is_refused_and_the_refusal_names_what_would_have_worked() {
        let ws = seeded();
        let refusal = ws.apply("Nope").expect_err("no such arrangement");
        assert_eq!(
            refusal,
            WorkspaceRefusal::NoSuchArrangement {
                asked: "Nope".to_owned(),
                known: vec!["Capture".to_owned(), "Overview".to_owned()],
            }
        );
        let said = refusal.reason().to_string();
        assert!(
            said.contains("Overview") && said.contains("Capture"),
            "a refusal that does not name the arrangements leaves the caller \
             guessing: {said}"
        );
        assert_eq!(refusal.wire_word(), "no-such-arrangement");
    }

    #[test]
    fn an_empty_set_says_so_rather_than_listing_nothing() {
        // ★ The sentence a caller reads has to be true of the case it is in. A
        // "the saved arrangements are " with nothing after it is worse than no
        // list at all, and it is what a naive join produces.
        let ws: Workspaces<&str> = Workspaces::new();
        let refusal = ws.apply("Overview").expect_err("nothing is saved");
        assert_eq!(
            refusal.reason().to_string(),
            "no arrangement has been saved yet"
        );
    }

    #[test]
    fn a_person_saves_their_own_and_can_delete_it() {
        let mut ws = seeded();
        ws.save("Mine", "board:mine").expect("a person may save");
        assert_eq!(ws.provenance("Mine"), Some(Provenance::Saved));
        assert_eq!(ws.names(), vec!["Capture", "Mine", "Overview"]);
        // Overwriting one's own is allowed, and it stays theirs.
        ws.save("Mine", "board:mine2")
            .expect("a person may overwrite");
        assert_eq!(ws.apply("Mine"), Ok(&"board:mine2"));
        assert_eq!(ws.provenance("Mine"), Some(Provenance::Saved));
        assert_eq!(ws.delete("Mine"), Ok("board:mine2"));
        assert_eq!(ws.provenance("Mine"), None);
        assert_eq!(ws.len(), 2);
    }

    #[test]
    fn an_arrangement_the_application_ships_cannot_be_deleted_and_says_why() {
        let mut ws = seeded();
        let refusal = ws
            .delete("Overview")
            .expect_err("a built-in is not a person's");
        assert_eq!(
            refusal,
            WorkspaceRefusal::BuiltIn {
                asked: "Overview".to_owned()
            }
        );
        let said = refusal.reason().to_string();
        assert!(
            said.contains("Overview") && said.contains("another name"),
            "the refusal must name the arrangement AND what to do instead: {said}"
        );
        assert_eq!(refusal.wire_word(), "built-in");
        // ★ And it is STILL THERE. A refusal that removed the thing it refused
        // to remove would be the worst of both.
        assert_eq!(ws.apply("Overview"), Ok(&"board:overview"));
        assert_eq!(ws.len(), 2);
    }

    #[test]
    fn saving_over_a_built_in_is_refused_so_the_set_cannot_lie_about_provenance() {
        // ★★★★★ The case that makes `save` fallible at all. If this were
        // allowed the row would keep saying `built-in` while holding a person's
        // layout — and the delete rule, which is the whole point of the
        // distinction, would then protect the wrong thing.
        let mut ws = seeded();
        let refusal = ws
            .save("Overview", "board:mine")
            .expect_err("a built-in name is the application's");
        assert_eq!(
            refusal,
            WorkspaceRefusal::BuiltIn {
                asked: "Overview".to_owned()
            }
        );
        assert_eq!(ws.apply("Overview"), Ok(&"board:overview"));
    }

    #[test]
    fn an_arrangement_needs_a_name_to_be_found_again() {
        let mut ws = seeded();
        for blank in ["", "   ", "\t"] {
            assert_eq!(ws.save(blank, "board:x"), Err(WorkspaceRefusal::Unnamed));
        }
        assert_eq!(ws.len(), 2, "a refused save adds nothing");
        // A name with space around it is the same name — a menu showing
        // "Mine" and " Mine" as two rows is a set nobody can use.
        ws.save("  Mine  ", "board:mine")
            .expect("trimmed and saved");
        assert_eq!(ws.provenance("Mine"), Some(Provenance::Saved));
    }

    #[test]
    fn the_delete_rule_is_read_from_the_provenance_and_not_re_decided() {
        // The property a consumer relies on: it can ask whether a row offers a
        // delete WITHOUT trying one, and the answer is the same rule `delete`
        // enforces. Two readers, one decision.
        let mut ws = seeded();
        ws.save("Mine", "board:mine").expect("saved");
        for (name, arrangement) in ws.iter().map(|(n, a)| (n.to_owned(), a.provenance)) {
            let offered = arrangement.deletable();
            let mut probe = ws.clone();
            let actually = probe.delete(&name).is_ok();
            assert_eq!(
                offered, actually,
                "{name:?} advertises deletable={offered} and delete answered {actually}"
            );
        }
    }

    #[test]
    fn a_set_round_trips_with_every_row_keeping_its_provenance() {
        // ★★★★★ R1895 — the second consumer holds this in a reactive `Signal`,
        // which requires it, and the first (a `RefCell`) did not. The property
        // that matters is not that it serialises but that PROVENANCE survives:
        // a set that reopened with everything `Saved` would make what an
        // application ships deletable.
        let mut ws = seeded();
        ws.save("Mine", "board:mine").expect("saved");
        let json = serde_json::to_string(&ws).expect("a set serialises");
        let back: Workspaces<&str> = serde_json::from_str(&json).expect("and round-trips");
        assert_eq!(back, ws);
        assert_eq!(back.provenance("Overview"), Some(Provenance::BuiltIn));
        assert_eq!(back.provenance("Mine"), Some(Provenance::Saved));
        // And the words are the published ones, so a stored set and a wire row
        // cannot name the same provenance differently.
        assert!(json.contains("\"built-in\"") && json.contains("\"saved\""));
        // An unreadable provenance is a REFUSAL, not a default — guessing
        // either way is wrong in a different direction.
        let bad = json.replace("\"saved\"", "\"mine-i-think\"");
        assert!(serde_json::from_str::<Workspaces<&str>>(&bad).is_err());
    }

    #[test]
    fn only_what_a_person_saved_is_written_to_storage() {
        // ★★★★★ R1897 — the built-ins are the APPLICATION's. Storing them would
        // look harmless and is not: a later build with different ones would
        // find the old set on disk and bring it back.
        let mut ws = seeded();
        ws.save("Mine", "board:mine").expect("saved");
        ws.save("Other", "board:other").expect("saved");
        let out = ws.saved();
        assert_eq!(out.names(), vec!["Mine", "Other"]);
        assert!(out.iter().all(|(_, a)| a.provenance == Provenance::Saved));
        // And the live set is untouched by having been read.
        assert_eq!(ws.len(), 4);
    }

    #[test]
    fn a_restore_lays_saved_rows_over_this_builds_own_and_names_what_it_refused() {
        let mut ws = seeded();
        // A stored set as a previous session would have written it, plus two
        // rows that must NOT come back: one whose name this build now ships,
        // and one claiming to be a built-in.
        let mut stored: Workspaces<&str> = Workspaces::new();
        stored.save("Mine", "board:mine").expect("a person's");
        stored
            .save("Overview", "board:theirs")
            .expect("a name this build now ships");
        let stored = Workspaces {
            entries: stored
                .entries
                .into_iter()
                .chain(std::iter::once((
                    "Sneaky".to_owned(),
                    super::Arrangement {
                        layout: "board:sneaky",
                        provenance: Provenance::BuiltIn,
                    },
                )))
                .collect(),
        };

        let refused = ws.restore(stored);
        // ★ The person's own came back.
        assert_eq!(ws.apply("Mine"), Ok(&"board:mine"));
        assert_eq!(ws.provenance("Mine"), Some(Provenance::Saved));
        // ★★ This build's arrangement stands, with its own layout.
        assert_eq!(ws.apply("Overview"), Ok(&"board:overview"));
        assert_eq!(ws.provenance("Overview"), Some(Provenance::BuiltIn));
        // ★★★ And the row claiming to be a built-in did not get in at all.
        assert_eq!(ws.provenance("Sneaky"), None);
        // ★★★★★ Both refusals are REPORTED, by name. A restore that silently
        // discarded them would be one nobody could debug.
        let names: Vec<&str> = refused
            .iter()
            .map(|r| match r {
                WorkspaceRefusal::NotYours { asked } => asked.as_str(),
                other => panic!("unexpected refusal: {other:?}"),
            })
            .collect();
        assert_eq!(names, vec!["Overview", "Sneaky"]);
        for refusal in &refused {
            let said = refusal.reason().to_string();
            assert!(
                said.contains("this build ships"),
                "a refused restore says why the build's stands: {said}"
            );
            assert_eq!(refusal.wire_word(), "not-yours");
        }
    }

    #[test]
    fn a_save_then_restore_round_trip_brings_back_exactly_the_persons_own() {
        // The whole journey, since that is what a session does: save, write,
        // start again from the shipped set, read back.
        let mut before = seeded();
        before.save("Mine", "board:mine").expect("saved");
        let wire = serde_json::to_string(&before.saved()).expect("serialises");

        let mut after = seeded();
        let stored: Workspaces<&str> = serde_json::from_str(&wire).expect("round-trips");
        let refused = after.restore(stored);
        assert!(refused.is_empty(), "nothing to refuse: {refused:?}");
        assert_eq!(after.names(), before.names());
        assert_eq!(after.provenance("Mine"), Some(Provenance::Saved));
        assert_eq!(after.provenance("Overview"), Some(Provenance::BuiltIn));
    }

    #[test]
    fn the_menu_order_does_not_depend_on_the_order_things_were_saved() {
        let mut a = Workspaces::new().with_built_in("Overview", "o");
        let mut b = Workspaces::new().with_built_in("Overview", "o");
        a.save("Zed", "z").unwrap();
        a.save("Alpha", "a").unwrap();
        b.save("Alpha", "a").unwrap();
        b.save("Zed", "z").unwrap();
        assert_eq!(a.names(), b.names());
        assert_eq!(a.names(), vec!["Alpha", "Overview", "Zed"]);
    }
}
