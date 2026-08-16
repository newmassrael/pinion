//! R1706 §5.38 §5.40 §2 #7 — **a selection is a set, and one of it leads.**
//!
//! # The fact that was missing
//!
//! A surface that lets a person select more than one thing owes two answers,
//! not one: *which things are selected*, and *which of them is the one an
//! inspector, a rename box or a "delete this" button means*. The second is the
//! **active** member — WAI-ARIA spells the pair `aria-selected` and
//! `aria-activedescendant`, and a DCC calls it the active node.
//!
//! Measured on the reference toolkit at 6.11.1, by building a probe and running
//! it offscreen rather than reading its headers: the family whose shape a node
//! canvas actually has — a free-form scene of items — publishes **11
//! properties and 21 methods**, of which exactly **two** name selection
//! (`selectionChanged`, `clearSelection`), and **zero** name a current, a
//! primary, a lead or an anchor. Selecting three items leaves its focus item
//! null, so nothing there answers "which of the three". The other family, bound
//! to an item model, *does* carry a current index — and its whole vocabulary is
//! a model index, so it cannot address an item on a canvas at all. The toolkit
//! has the fact and the shape, and never in the same place.
//!
//! Both halves of that hole are in this tree, measured the same way. The
//! analysis tool's node laboratory held `Option<NodeId>` — a leader with no
//! set, so "select this frame's members" had nowhere to land. The material node
//! editor held a `BTreeSet<NodeId>` — a set with no leader, and its `selected`
//! slot answers **nothing** whenever two nodes are selected, which is what a
//! reader's inspector follows.
//!
//! So the value lives here, once, generic over whatever a surface calls a
//! thing.
//!
//! # What it guarantees that the toolkit does not
//!
//! * **A non-empty selection has exactly one active member**, structurally: the
//!   active is an index into the members, so "active but not selected" is
//!   unrepresentable rather than merely avoided. The free-floating cursor that
//!   a keyboard walks *without* selecting is a different fact and already has a
//!   home ([`widgets::roving`](crate::widgets::roving)); conflating the two is
//!   what leaves a surface unable to say which of them moved.
//! * **Every mutation returns what changed** — [`Change`] names what came, what
//!   went, and where the active was before and after. The toolkit's canvas
//!   signal carries *no arguments at all*, so a listener must re-derive the
//!   delta from a fresh read; its item-model signal carries the delta but
//!   splits the active's move into a second signal that can be observed
//!   between, which is a state no single read explains.
//! * **Members keep the order they arrived in**, so the active of a group is
//!   the group's first member and stays put while the rest are added.
//!
//! # What it deliberately does not hold
//!
//! No anchor for range extension. The toolkit exposes none either (measured:
//! zero anchor accessors on its selection model *and* on its view), but that is
//! not the argument — the argument is that neither canvas in this tree extends
//! a range, because an unordered canvas has no range to extend. A field with no
//! consumer is a guess about the future, and this module would rather be asked
//! for one.

use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What a mutation did to a selection.
///
/// Returned by every mutating method so a caller never has to diff two reads.
/// `added` and `removed` are disjoint, and both are in the order the change
/// touched them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change<T> {
    /// Members this call put in, in arrival order.
    pub added: Vec<T>,
    /// Members this call took out, in the order they had been held.
    pub removed: Vec<T>,
    /// The active member before the call.
    pub active_before: Option<T>,
    /// The active member after the call.
    pub active_after: Option<T>,
}

impl<T: PartialEq> Change<T> {
    /// Whether anything moved at all — membership or the active.
    ///
    /// A caller that repaints on every event uses this to not; a caller
    /// growing a selection until it stops growing uses it to know it has
    /// stopped.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty() || self.active_moved()
    }

    /// Whether the member an inspector follows is a different one.
    ///
    /// Separate from [`changed`](Change::changed) because the two demand
    /// different work: membership moving repaints the canvas, the active moving
    /// rebuilds the inspector.
    #[must_use]
    pub fn active_moved(&self) -> bool {
        self.active_before != self.active_after
    }
}

/// A set of selected things with one of them leading.
///
/// See the module documentation for what this holds that the reference
/// toolkit's two selection families each hold half of.
///
/// # Examples
///
/// ```
/// use pinion_core::selection::Selection;
///
/// // A group selects all of it, and the first member leads.
/// let mut sel = Selection::group(["a", "b", "c"]);
/// assert_eq!(sel.active(), Some(&"a"));
/// assert_eq!(sel.len(), 3);
///
/// // Narrowing to one keeps the invariant and reports the move.
/// let change = sel.set_one("b");
/// assert_eq!(change.removed, ["a", "c"]);
/// assert!(change.active_moved());
/// assert_eq!(sel.active(), Some(&"b"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection<T> {
    /// Members in arrival order, without repeats.
    members: Vec<T>,
    /// Index into `members`, or `None` exactly when `members` is empty.
    ///
    /// An index rather than a copy of the item: it is what makes "the active
    /// is one of the members" a property of the representation instead of a
    /// rule someone has to keep.
    active: Option<usize>,
}

impl<T> Default for Selection<T> {
    fn default() -> Self {
        Self {
            members: Vec::new(),
            active: None,
        }
    }
}

impl<T> Selection<T> {
    /// Nothing selected.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            members: Vec::new(),
            active: None,
        }
    }

    /// The member an inspector follows, or `None` when nothing is selected.
    #[must_use]
    pub fn active(&self) -> Option<&T> {
        self.active.and_then(|i| self.members.get(i))
    }

    /// Everything selected, in arrival order.
    #[must_use]
    pub fn members(&self) -> &[T] {
        &self.members
    }

    /// How many are selected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether nothing is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Whether more than one thing is selected.
    ///
    /// The question a form asks before it decides whether it is editing a
    /// thing or a set of them.
    #[must_use]
    pub fn is_multiple(&self) -> bool {
        self.members.len() > 1
    }
}

impl<T: Clone + PartialEq> Selection<T> {
    /// Exactly one thing selected, and it leads.
    #[must_use]
    pub fn one(item: T) -> Self {
        Self {
            members: vec![item],
            active: Some(0),
        }
    }

    /// A group selected, the first of it leading.
    ///
    /// Repeats collapse to their first appearance, which is what keeps the
    /// leader stable when a caller hands over a list it built by walking a
    /// relation twice. An empty input yields an empty selection rather than a
    /// selection with no leader — those are the same thing here.
    pub fn group<I: IntoIterator<Item = T>>(items: I) -> Self {
        let mut members: Vec<T> = Vec::new();
        for item in items {
            if !members.contains(&item) {
                members.push(item);
            }
        }
        let active = if members.is_empty() { None } else { Some(0) };
        Self { members, active }
    }

    /// Whether `item` is selected.
    #[must_use]
    pub fn contains(&self, item: &T) -> bool {
        self.members.contains(item)
    }

    /// Whether `item` is the one an inspector follows.
    ///
    /// Asked per member while painting, which is why it is a method rather
    /// than a comparison a caller writes: a surface that spells it
    /// `selection.active() == Some(&id)` in one painter and
    /// `selection.members()[0] == id` in another has two answers to one
    /// question the first time a group is selected.
    #[must_use]
    pub fn is_active(&self, item: &T) -> bool {
        self.active() == Some(item)
    }

    /// Select nothing.
    pub fn clear(&mut self) -> Change<T> {
        let before = self.active().cloned();
        let removed = std::mem::take(&mut self.members);
        self.active = None;
        Change {
            added: Vec::new(),
            removed,
            active_before: before,
            active_after: None,
        }
    }

    /// Replace the selection with exactly `item`.
    pub fn set_one(&mut self, item: T) -> Change<T> {
        self.set_group([item])
    }

    /// Replace the selection with `items`, the first of them leading.
    pub fn set_group<I: IntoIterator<Item = T>>(&mut self, items: I) -> Change<T> {
        let next = Self::group(items);
        let before = self.active().cloned();
        let added: Vec<T> = next
            .members
            .iter()
            .filter(|m| !self.members.contains(m))
            .cloned()
            .collect();
        let removed: Vec<T> = self
            .members
            .iter()
            .filter(|m| !next.members.contains(m))
            .cloned()
            .collect();
        let after = next.active().cloned();
        *self = next;
        Change {
            added,
            removed,
            active_before: before,
            active_after: after,
        }
    }

    /// Add `item` if it is absent, remove it if it is present.
    ///
    /// Toggling something in makes it the active member — it is the thing the
    /// person just pointed at. Toggling the active member OUT hands the lead
    /// to whatever arrived first among the rest, rather than leaving a set with
    /// no leader.
    pub fn toggle(&mut self, item: T) -> Change<T> {
        let before = self.active().cloned();
        if let Some(at) = self.members.iter().position(|m| *m == item) {
            let removed = self.members.remove(at);
            self.active = if self.members.is_empty() {
                None
            } else {
                match self.active {
                    // The active was the one removed: the lead goes to the
                    // first remaining member.
                    Some(i) if i == at => Some(0),
                    Some(i) if i > at => Some(i - 1),
                    other => other,
                }
            };
            let after = self.active().cloned();
            Change {
                added: Vec::new(),
                removed: vec![removed],
                active_before: before,
                active_after: after,
            }
        } else {
            self.members.push(item.clone());
            self.active = Some(self.members.len() - 1);
            Change {
                added: vec![item],
                removed: Vec::new(),
                active_before: before,
                active_after: self.active().cloned(),
            }
        }
    }

    /// Drop every member `keep` rejects.
    ///
    /// What a surface calls when the things themselves went away — a node
    /// deleted, a row filtered out. The lead is only re-seated when the member
    /// holding it is one of the dropped, so pruning something else leaves the
    /// inspector where it was.
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut keep: F) -> Change<T> {
        let before = self.active().cloned();
        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for member in std::mem::take(&mut self.members) {
            if keep(&member) {
                kept.push(member);
            } else {
                removed.push(member);
            }
        }
        self.members = kept;
        let survivor = if self.members.is_empty() {
            None
        } else {
            Some(0)
        };
        self.active = match &before {
            Some(item) => match self.members.iter().position(|m| m == item) {
                Some(at) => Some(at),
                None => survivor,
            },
            None => survivor,
        };
        let after = self.active().cloned();
        Change {
            added: Vec::new(),
            removed,
            active_before: before,
            active_after: after,
        }
    }
}

/// The shape a selection travels in — the members and which index leads.
///
/// A separate type because it is the *only* place the invariant can be
/// violated: a caller cannot build a bad `Selection`, but a snapshot on disk
/// or a hot-reload payload can hold anything. So the crossing is where it is
/// re-established rather than trusted, and the normalisation is exactly the
/// one the constructors apply.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Selection")]
struct Wire<M> {
    members: M,
    active: Option<usize>,
}

impl<T: Serialize> Serialize for Selection<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Wire {
            members: &self.members,
            active: self.active,
        }
        .serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Selection<T>
where
    T: Clone + PartialEq + DeserializeOwned,
{
    /// Re-establishes the invariant rather than trusting it: repeats collapse,
    /// an out-of-range or absent lead over a non-empty set falls to the first
    /// member, and a lead over an empty set becomes no lead at all.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::<Vec<T>>::deserialize(deserializer)?;
        let mut selection = Self::group(wire.members);
        if let Some(at) = wire.active
            && at < selection.members.len()
        {
            selection.active = Some(at);
        }
        Ok(selection)
    }
}

impl<T: fmt::Display> fmt::Display for Selection<T> {
    /// The members in arrival order, comma separated — the spelling a wire
    /// slot answers in and an agent writes back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, member) in self.members.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{member}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, Selection};

    #[test]
    fn an_empty_selection_has_no_leader() {
        let sel: Selection<u32> = Selection::empty();
        assert_eq!(sel.active(), None);
        assert!(sel.is_empty());
        assert!(!sel.is_multiple());
        assert_eq!(sel.len(), 0);
    }

    #[test]
    fn a_group_is_led_by_its_first_member() {
        let sel = Selection::group([7_u32, 3, 9]);
        assert_eq!(sel.active(), Some(&7));
        assert_eq!(sel.members(), [7, 3, 9]);
        assert!(sel.is_multiple());
    }

    #[test]
    fn a_group_collapses_repeats_to_their_first_appearance() {
        let sel = Selection::group([4_u32, 1, 4, 1, 2]);
        assert_eq!(sel.members(), [4, 1, 2]);
        assert_eq!(sel.active(), Some(&4));
    }

    #[test]
    fn an_empty_group_is_an_empty_selection() {
        let sel: Selection<u32> = Selection::group([]);
        assert!(sel.is_empty());
        assert_eq!(sel.active(), None);
    }

    /// One mutation, for the sequence the invariant is driven through.
    type Mutation = Box<dyn Fn(&mut Selection<&'static str>)>;

    /// The invariant the reference toolkit's canvas family cannot state: every
    /// non-empty selection reachable through this API has exactly one leader,
    /// and the leader is one of the members.
    #[test]
    fn every_non_empty_selection_reachable_here_has_exactly_one_leader() {
        let mut sel = Selection::group(["a", "b", "c"]);
        let ops: Vec<Mutation> = vec![
            Box::new(|s: &mut Selection<&str>| {
                s.toggle("d");
            }),
            Box::new(|s: &mut Selection<&str>| {
                s.toggle("a");
            }),
            Box::new(|s: &mut Selection<&str>| {
                s.set_one("z");
            }),
            Box::new(|s: &mut Selection<&str>| {
                s.set_group(["p", "q"]);
            }),
            Box::new(|s: &mut Selection<&str>| {
                s.retain(|m| *m != "p");
            }),
            Box::new(|s: &mut Selection<&str>| {
                s.clear();
            }),
            Box::new(|s: &mut Selection<&str>| {
                s.toggle("only");
            }),
        ];
        for op in ops {
            op(&mut sel);
            match sel.active() {
                Some(active) => assert!(
                    sel.contains(active),
                    "the leader {active:?} is not one of {:?}",
                    sel.members()
                ),
                None => assert!(
                    sel.is_empty(),
                    "no leader but {} member(s)",
                    sel.members().len()
                ),
            }
        }
    }

    #[test]
    fn narrowing_to_one_reports_both_halves_of_the_move() {
        let mut sel = Selection::group([1_u32, 2, 3]);
        let change = sel.set_one(2);
        assert_eq!(change.added, Vec::<u32>::new());
        assert_eq!(change.removed, [1, 3]);
        assert_eq!(change.active_before, Some(1));
        assert_eq!(change.active_after, Some(2));
        assert!(change.changed());
        assert!(change.active_moved());
    }

    #[test]
    fn re_selecting_the_same_group_reports_nothing_moved() {
        let mut sel = Selection::group(["x", "y"]);
        let change = sel.set_group(["x", "y"]);
        assert!(!change.changed());
        assert!(!change.active_moved());
    }

    /// Membership can move while the leader stays — the two questions a caller
    /// asks separately, because one repaints and the other rebuilds.
    #[test]
    fn membership_can_move_while_the_leader_stays() {
        let mut sel = Selection::group(["a", "b"]);
        let change = sel.set_group(["a", "b", "c"]);
        assert_eq!(change.added, ["c"]);
        assert!(change.changed());
        assert!(!change.active_moved());
        assert_eq!(sel.active(), Some(&"a"));
    }

    #[test]
    fn toggling_in_leads_and_toggling_out_hands_the_lead_on() {
        let mut sel = Selection::group([1_u32, 2]);
        let added = sel.toggle(5);
        assert_eq!(added.added, [5]);
        assert_eq!(sel.active(), Some(&5));

        let removed = sel.toggle(5);
        assert_eq!(removed.removed, [5]);
        assert_eq!(sel.active(), Some(&1));
        assert_eq!(sel.members(), [1, 2]);
    }

    #[test]
    fn toggling_the_last_member_out_empties_the_selection() {
        let mut sel = Selection::one("solo");
        let change = sel.toggle("solo");
        assert!(sel.is_empty());
        assert_eq!(change.active_after, None);
        assert!(change.active_moved());
    }

    /// Removing a member that sits BEFORE the leader must not slide the lead
    /// onto its neighbour — the stored index has to follow the item, which is
    /// the one bug an index-based active can have and a copy-based one cannot.
    #[test]
    fn removing_a_member_before_the_leader_keeps_the_leader() {
        let mut sel = Selection::group(["a", "b", "c"]);
        sel.toggle("d"); // absent, so it joins and leads, at index 3
        assert_eq!(sel.active(), Some(&"d"));
        sel.toggle("b"); // present and earlier: it leaves
        assert_eq!(sel.members(), ["a", "c", "d"]);
        assert_eq!(sel.active(), Some(&"d"));
    }

    /// The other direction: removing a member AFTER the leader leaves the
    /// index alone.
    #[test]
    fn removing_a_member_after_the_leader_keeps_the_leader() {
        let mut sel = Selection::group(["a", "b", "c"]);
        sel.toggle("c");
        assert_eq!(sel.members(), ["a", "b"]);
        assert_eq!(sel.active(), Some(&"a"));
    }

    #[test]
    fn pruning_keeps_the_leader_when_it_survives() {
        let mut sel = Selection::group([10_u32, 20, 30]);
        sel.toggle(20); // 20 leaves, lead falls to 10
        sel.toggle(20); // 20 returns and leads
        assert_eq!(sel.active(), Some(&20));

        let change = sel.retain(|m| *m != 30);
        assert_eq!(change.removed, [30]);
        assert!(!change.active_moved());
        assert_eq!(sel.active(), Some(&20));
    }

    #[test]
    fn pruning_the_leader_hands_the_lead_to_the_first_survivor() {
        let mut sel = Selection::group(["a", "b", "c"]);
        let change = sel.retain(|m| *m != "a");
        assert_eq!(change.removed, ["a"]);
        assert_eq!(change.active_before, Some("a"));
        assert_eq!(change.active_after, Some("b"));
        assert_eq!(sel.active(), Some(&"b"));
    }

    #[test]
    fn pruning_everything_leaves_no_leader() {
        let mut sel = Selection::group([1_u32, 2]);
        let change = sel.retain(|_| false);
        assert!(sel.is_empty());
        assert_eq!(change.active_after, None);
        assert_eq!(change.removed, [1, 2]);
    }

    #[test]
    fn clearing_reports_everything_it_took() {
        let mut sel = Selection::group(["a", "b"]);
        let change = sel.clear();
        assert_eq!(change.removed, ["a", "b"]);
        assert_eq!(change.active_before, Some("a"));
        assert_eq!(change.active_after, None);
        assert!(sel.is_empty());
    }

    #[test]
    fn a_selection_writes_itself_in_arrival_order() {
        let sel = Selection::group(["P-01", "S-01", "R-01"]);
        assert_eq!(sel.to_string(), "P-01,S-01,R-01");
        assert_eq!(Selection::<&str>::empty().to_string(), "");
    }

    #[test]
    fn a_selection_survives_a_round_trip_unchanged() {
        let sel = Selection::group(["a", "b", "c"]);
        let json = serde_json::to_string(&sel).expect("serialise");
        let back: Selection<String> = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back.members(), ["a", "b", "c"]);
        assert_eq!(back.active().map(String::as_str), Some("a"));
    }

    /// The one place the invariant can arrive broken — a snapshot on disk, a
    /// hot-reload payload — and the crossing re-establishes it instead of
    /// trusting it.
    #[test]
    fn a_wire_form_that_breaks_the_invariant_is_normalised_at_the_crossing() {
        // A lead past the end of the members.
        let out_of_range: Selection<String> =
            serde_json::from_str(r#"{"members":["a","b"],"active":9}"#).expect("deserialise");
        assert_eq!(out_of_range.active().map(String::as_str), Some("a"));

        // A lead over nothing.
        let no_members: Selection<String> =
            serde_json::from_str(r#"{"members":[],"active":0}"#).expect("deserialise");
        assert!(no_members.is_empty());
        assert_eq!(no_members.active(), None);

        // Members with no lead at all.
        let no_lead: Selection<String> =
            serde_json::from_str(r#"{"members":["x","y"],"active":null}"#).expect("deserialise");
        assert_eq!(no_lead.active().map(String::as_str), Some("x"));

        // Repeated members collapse, and the lead lands on the same ITEM.
        let repeats: Selection<String> =
            serde_json::from_str(r#"{"members":["a","b","a"],"active":2}"#).expect("deserialise");
        assert_eq!(repeats.members(), ["a", "b"]);
        assert_eq!(repeats.active().map(String::as_str), Some("a"));
    }

    #[test]
    fn an_unchanged_selection_reports_no_change() {
        let change: Change<u32> = Change::default();
        assert!(!change.changed());
        assert!(!change.active_moved());
    }
}
