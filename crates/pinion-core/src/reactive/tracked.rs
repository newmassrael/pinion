//! `Tracked<T>` — R1652 §5.22 — a reactive cell for a model too big to clone.
//!
//! # The gap this closes, measured rather than argued
//!
//! [`Signal<T>`](super::signal::Signal) requires `T: Clone + PartialEq +
//! Serialize + DeserializeOwned` and its [`get`](super::signal::Signal::get)
//! **clones the stored value**. That is right for a cell holding a number, a
//! word or a small record, which is what almost every binding holds.
//!
//! It is wrong for a screen whose model is a whole document. `Signal<Document>`
//! re-clones the graph on every read, and a view that re-clones its model once
//! per frame is not a screen anybody would ship — so a large binding puts the
//! model behind a bare `RefCell` instead, and **the reactive graph cannot see
//! through it**. R1651 shipped exactly that and the defect it produces: a value
//! edited in the inspector changed the launch gate on the wire and left the
//! screen painting the old row, because nothing the view had read had changed.
//!
//! The repair R1651 reached for was a `revision: Signal<u64>` the view reads and
//! every mutation bumps — nine call sites, and **nothing checks that a tenth
//! mutation remembered to bump**. Not the compiler, not a test, not a gate. The
//! symptom is "sometimes the screen does not update", which is the class R1650
//! wrote down for pointer reachability: *a capability verified only through a
//! bypass is not verified*.
//!
//! # Why this cannot be forgotten
//!
//! [`Tracked::borrow_mut`] hands back a guard, and the guard notifies
//! subscribers **when it is dropped**. There is no way to mutate the value
//! without going through it, and no way to hold it without eventually dropping
//! it, so the bump is not a step anybody performs — it is what the borrow *is*.
//! That is the difference between a rule and a shape: R1638's "make the silence
//! an arm" applied to a write.
//!
//! ```ignore
//! let doc: Tracked<Document> = Tracked::new(Document::new("graph"));
//!
//! // In a view fn — subscribes, no clone.
//! let node_count = doc.borrow().node_count();
//!
//! // Anywhere — the view re-runs when this guard drops.
//! doc.borrow_mut().add_node(...);
//! ```
//!
//! # What it deliberately does not do
//!
//! * **No equality skip.** `Signal::set` compares and skips when the value did
//!   not change; a `T` this type exists for is one comparing is expensive on,
//!   and a comparison per mutation would give back what the clone-free read
//!   bought. So a `borrow_mut` that changes nothing still notifies. Stated
//!   rather than hidden: a caller that takes a mutable borrow and does not use
//!   it costs one extra view pass.
//! * **No snapshot / restore.** `Signal` participates in `Owner::snapshot` for
//!   hot reload through its `Serialize` bound, which is the bound this type
//!   drops. A binding that wants its document in a snapshot serialises it
//!   itself.
//! * **No interior read during a mutable borrow.** `RefCell`'s rule, unchanged
//!   and deliberately not softened: two live borrows of one model is a bug
//!   whatever the reactive layer does about it.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::{Rc, Weak};

use super::owner::{
    ObserverEntry, ReactiveNode, SubscriberSet, dispatch_dirty, next_node_id, with_current_owner,
};

/// A reactive cell whose value is **borrowed** rather than cloned.
///
/// Cloning the handle yields another handle to the same storage, the `Rc`
/// semantics [`Signal`](super::signal::Signal) has.
pub struct Tracked<T> {
    inner: Rc<TrackedInner<T>>,
}

struct TrackedInner<T> {
    id: u64,
    value: RefCell<T>,
    revision: std::cell::Cell<u64>,
    observers: RefCell<SubscriberSet>,
}

impl<T: 'static> Tracked<T> {
    /// A cell holding `initial`.
    #[must_use]
    pub fn new(initial: T) -> Self {
        Self {
            inner: Rc::new(TrackedInner {
                id: next_node_id(),
                value: RefCell::new(initial),
                revision: std::cell::Cell::new(0),
                observers: RefCell::new(SubscriberSet::new()),
            }),
        }
    }

    /// Stable identity, for a consumer keying anything by cell.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// Borrow the value, **subscribing** the reactive node in scope.
    ///
    /// The read half of the contract: a view fn that borrows re-runs when the
    /// value is mutated, and nothing was cloned to make that true.
    ///
    /// # Panics
    ///
    /// If the value is already mutably borrowed — `RefCell`'s rule.
    #[must_use]
    pub fn borrow(&self) -> Ref<'_, T> {
        with_current_owner(|node_opt| {
            if let Some(node) = node_opt {
                self.subscribe(node);
            }
        });
        self.inner.value.borrow()
    }

    /// Borrow the value mutably. **Subscribers are notified when the returned
    /// guard is dropped**, which is what makes the notification unforgettable.
    ///
    /// Deliberately does **not** subscribe: a mutation is not a read, and a
    /// view fn that mutated its own model would re-run itself forever.
    ///
    /// # Panics
    ///
    /// If the value is already borrowed — `RefCell`'s rule.
    #[must_use]
    pub fn borrow_mut(&self) -> TrackedMut<'_, T> {
        let value = self.inner.value.borrow_mut();
        TrackedMut {
            value,
            cell: Rc::clone(&self.inner),
        }
    }

    /// Read the value through `f` without holding a guard across the call.
    ///
    /// The shape most reads want: `tracked.with(|doc| doc.node_count())`.
    /// Subscribes exactly as [`Self::borrow`] does.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.borrow())
    }

    /// Mutate through `f`, notifying once when `f` returns.
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut guard = self.borrow_mut();
        f(&mut guard)
    }

    /// How many times a mutable borrow has been taken and released.
    ///
    /// Monotonic and **not** value-change-counting — see the module docs on why
    /// there is no equality skip.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.revision.get()
    }

    fn subscribe(&self, node: &Rc<dyn ReactiveNode>) {
        let node_id = node.node_id();
        if self.inner.observers.borrow().contains(node_id) {
            return;
        }
        self.inner.observers.borrow_mut().insert(ObserverEntry {
            id: node_id,
            node: Rc::downgrade(node),
        });
        // A `Weak` so a long-lived subscriber does not pin a short-lived
        // cell's storage — the R37.5 #2 leak fix, same as `Signal`.
        let weak: Weak<TrackedInner<T>> = Rc::downgrade(&self.inner);
        node.add_subscription_cleanup(Box::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.observers.borrow_mut().remove(node_id);
            }
        }));
    }

    #[cfg(test)]
    pub(crate) fn observer_count(&self) -> usize {
        self.inner.observers.borrow().len()
    }
}

/// The guard [`Tracked::borrow_mut`] returns: a `RefMut` that notifies the
/// cell's subscribers when it is dropped.
pub struct TrackedMut<'a, T> {
    value: RefMut<'a, T>,
    cell: Rc<TrackedInner<T>>,
}

impl<T> std::ops::Deref for TrackedMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> std::ops::DerefMut for TrackedMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T> Drop for TrackedMut<'_, T> {
    fn drop(&mut self) {
        self.cell.revision.set(self.cell.revision.get() + 1);
        // Snapshot before dispatching: a subscriber may touch this cell's
        // lists, and the borrow has to be released first.
        let snapshot = self.cell.observers.borrow().snapshot();
        dispatch_dirty(&snapshot);
        self.cell.observers.borrow_mut().prune_dead();
    }
}

impl<T> Clone for Tracked<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Tracked<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tracked")
            .field("id", &self.inner.id)
            .field("revision", &self.inner.revision.get())
            .field("value", &self.inner.value.borrow())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::Tracked;
    use crate::reactive::owner::Owner;

    /// A model that is expensive to clone and cannot be compared, which is the
    /// whole population this type exists for. It is neither `Clone` nor
    /// `PartialEq` nor `Serialize`, so it could not go in a `Signal` at all —
    /// that is the test, stated as a type rather than as a paragraph.
    #[derive(Debug)]
    struct BigModel {
        nodes: Vec<String>,
    }

    #[test]
    fn r1652_a_model_that_cannot_go_in_a_signal_goes_in_this() {
        let held = Tracked::new(BigModel {
            nodes: vec!["a".into()],
        });
        assert_eq!(held.with(|m| m.nodes.len()), 1);
        held.update(|m| m.nodes.push("b".into()));
        assert_eq!(held.with(|m| m.nodes.len()), 2);
    }

    #[test]
    fn r1652_a_mutable_borrow_notifies_when_it_is_dropped() {
        // ★ The property that makes the notification unforgettable: nobody
        // calls anything to announce the change. Dropping the guard IS the
        // announcement, and a mutation cannot avoid dropping its guard.
        let owner = Owner::new();
        let held: Tracked<Vec<u32>> = Tracked::new(vec![1]);

        let cell = held.clone();
        owner.run(move || {
            let _ = cell.borrow().len();
        });
        owner.clear_dirty();
        assert!(!owner.is_dirty());

        {
            let mut guard = held.borrow_mut();
            guard.push(2);
            assert!(
                !owner.is_dirty(),
                "and nothing is notified WHILE the borrow is held — a \
                 subscriber that re-read here would panic on the borrow"
            );
        }
        assert!(owner.is_dirty(), "the drop is what notified");
    }

    #[test]
    fn r1652_a_mutation_that_forgets_to_announce_is_unrepresentable() {
        // The counterfactual the debt file asked for, as a test: R1651's repair
        // was a revision signal every mutation had to remember to bump, and
        // nothing could catch a tenth mutation that did not. Here the ONLY way
        // to reach `&mut T` is through the guard, so "mutated without
        // notifying" has no spelling.
        let owner = Owner::new();
        let held: Tracked<Vec<u32>> = Tracked::new(vec![]);
        let cell = held.clone();
        owner.run(move || {
            let _ = cell.borrow().len();
        });

        for reach in [0_u32, 1, 2] {
            owner.clear_dirty();
            match reach {
                0 => held.update(|v| v.push(reach)),
                1 => *held.borrow_mut() = vec![9],
                _ => {
                    let mut guard = held.borrow_mut();
                    guard.clear();
                }
            }
            assert!(
                owner.is_dirty(),
                "every route to a mutation notified (route {reach})"
            );
        }
    }

    #[test]
    fn r1652_a_reader_subscribes_and_a_writer_does_not() {
        // A view fn that mutated its own model and subscribed to the mutation
        // would re-run itself forever.
        let owner = Owner::new();
        let held: Tracked<u32> = Tracked::new(0);
        assert_eq!(held.observer_count(), 0);

        let cell = held.clone();
        owner.run(move || {
            *cell.borrow_mut() += 1;
        });
        assert_eq!(
            held.observer_count(),
            0,
            "★ a mutable borrow is not a read, so it subscribes nobody"
        );

        let cell = held.clone();
        owner.run(move || {
            let _ = *cell.borrow();
        });
        assert_eq!(held.observer_count(), 1, "and a read does");
    }

    #[test]
    fn r1652_the_revision_counts_mutable_borrows_and_says_so() {
        // No equality skip, deliberately — see the module docs. A caller that
        // takes a mutable borrow and changes nothing costs one view pass, and
        // the counter reports that honestly rather than hiding it.
        let held: Tracked<u32> = Tracked::new(7);
        assert_eq!(held.revision(), 0);
        held.update(|v| *v = 7);
        assert_eq!(
            held.revision(),
            1,
            "a borrow that changed nothing still counts — comparing a model \
             this type exists for would give back what the clone-free read bought"
        );
        let _ = held.borrow();
        assert_eq!(held.revision(), 1, "and a read does not count");
    }

    #[test]
    fn r1652_two_handles_are_one_cell() {
        let a: Tracked<Vec<u32>> = Tracked::new(vec![]);
        let b = a.clone();
        assert_eq!(a.id(), b.id());
        b.update(|v| v.push(9));
        assert_eq!(a.with(Vec::len), 1);
    }
}
