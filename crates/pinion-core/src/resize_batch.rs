//! R1708 §5.16 §5.41 §2 #7 — **a resize batch says what it folded.**
//!
//! # The fact that was missing
//!
//! A window resize arrives at pointer rate. A frame does not. Dragging a
//! window edge for one second delivers on the order of a hundred size changes,
//! and this tree painted **one frame per event**, each one synchronously, each
//! one superseded microseconds later by the next.
//!
//! Measured on this host before the repair, driving a real mapped window with
//! real window-system resize events (not the RPC path, which mixes in a signal
//! write) and counting the shell's own frame counter:
//!
//! | resize events | frames painted | time to catch up after the LAST event |
//! |--:|--:|--:|
//! | 16 | 16 | 340 ms |
//! | 40 | 41 | 1,119 ms |
//! | 80 | 81 | 2,693 ms |
//!
//! That last row is the user's report: let go of the edge and the *contents*
//! of the window keep arriving for another two and a half seconds. Nothing was
//! slow — a resize frame measured *cheaper* than an idle one. There were
//! simply eighty of them, seventy-nine of which no one could ever see.
//!
//! # What the reference does, measured
//!
//! Measured at 6.11.1 by building a probe and running it offscreen rather than
//! reading headers. Forty resizes queued before the loop drains them:
//!
//! * At the **window** layer it delivers **all forty**, in order, every
//!   intermediate size observable.
//! * At the **widget** layer it delivers **one**, carrying the final size, and
//!   paints **once**, in 4.4 ms total.
//!
//! So the reference does not throttle the *facts*; it folds the *work*. That
//! is the right split and this module adopts it. What the reference does not
//! do is **say that it folded** — a consumer who wants to know that thirty-nine
//! sizes went unpainted has to instrument two layers itself and subtract one
//! count from the other. There is no API that answers it, because there is no
//! value that holds it.
//!
//! # What this guarantees that the reference does not
//!
//! * **Every event is accounted for.** [`ResizeTally::events`] equals
//!   `painted + superseded + repeated + pending`, and
//!   [`ResizeTally::is_balanced`] asserts exactly that. A resize that vanished
//!   without falling into one of those four buckets is a bug this type can
//!   report rather than a frame nobody counted. ★ A frame counts as *painted*
//!   only once the caller says it painted one ([`ResizeBatch::painted`], after
//!   the paint), so "took the fold and drew nothing" is a state the identity
//!   can see — see that method for what it cost to learn.
//! * **A drop names its reason.** [`Noted`] distinguishes a size that
//!   *replaced* pending work ([`Noted::Superseded`], which discarded a paint)
//!   from one that merely *repeated* the size already pending
//!   ([`Noted::Repeated`], which discarded nothing). Collapsing those into one
//!   number would make an idle window that re-announces its size look like a
//!   drag.
//! * **The fold says where it came from.** [`Fold::opened_at`] is the size the
//!   batch started at, so the single frame that answers a whole drag can state
//!   the span it covers instead of only its endpoint.
//!
//! This is the same rule R1707 wrote for a row filter, one layer down: a thing
//! that discards work owes the reader what it kept **and why it dropped the
//! rest**.
//!
//! # What this deliberately does not hold
//!
//! No timestamps, and no policy about *when* to drain. When to paint is the
//! event loop's judgment — it knows whether more events are already queued —
//! and a deadline stored here would be a second opinion about a fact the loop
//! already owns. This value holds the fold and its accounting; the shell's
//! `about_to_wait` decides the moment.

use serde::{Deserialize, Serialize};

/// A window surface size in physical pixels, spelled the way the rest of the
/// tree spells one (`external::surface_size`, `external::layout_size`).
pub type Size = (u32, u32);

/// What [`ResizeBatch::note`] did with an arriving resize.
///
/// Three arms, because "the batch grew" is three different facts and only one
/// of them threw work away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Noted {
    /// The batch was empty and this resize opened it. Nothing was discarded;
    /// this size is currently the one that will be painted.
    Opened,
    /// A different size was already pending and this one replaces it. The
    /// superseded size will **never be painted**: a later size is already
    /// known, so painting it would draw a window shape that no longer exists.
    Superseded {
        /// The size that was pending and now will not be painted.
        size: Size,
    },
    /// The pending size is already this size, so there is nothing new to
    /// paint. **Not** a supersede — no work was discarded, because no distinct
    /// frame was ever owed. A window-system that re-announces an unchanged
    /// size lands here.
    Repeated,
}

/// A pending resize together with what folding into it cost.
///
/// Produced by [`ResizeBatch::take`]; one `Fold` is answered by exactly one
/// painted frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fold {
    /// The size to paint — the most recent one noted.
    pub size: Size,
    /// The size that opened this batch. Equal to [`Self::size`] when nothing
    /// was folded in; otherwise the far end of the span this one frame answers.
    pub opened_at: Size,
    /// Distinct sizes discarded because a later one arrived first.
    pub superseded: u32,
    /// Repeats of the pending size, which discarded nothing.
    pub repeated: u32,
}

impl Fold {
    /// Did this fold discard any paint?
    ///
    /// False for the ordinary single resize (a window snapped to a new size
    /// once), true for a drag.
    #[must_use]
    pub fn folded(&self) -> bool {
        self.superseded > 0
    }
}

/// Lifetime accounting for one window's resizes.
///
/// Every resize ever noted lands in exactly one of the four buckets, and
/// [`Self::is_balanced`] is the assertion that says so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResizeTally {
    /// Resizes noted over the window's lifetime.
    pub events: u64,
    /// Frames REPORTED painted for a fold, via [`ResizeBatch::painted`].
    ///
    /// Not "folds drained": a fold is drained one call earlier, and the gap
    /// between the two is the only interval in which the identity below can
    /// notice a caller that took a fold and drew nothing.
    pub painted: u64,
    /// Resizes discarded because a later size arrived before they were
    /// painted. **This is the number the repair exists to make non-zero.**
    pub superseded: u64,
    /// Resizes that repeated the pending size and so discarded nothing.
    pub repeated: u64,
    /// `1` while a fold is waiting to be drained, `0` otherwise.
    ///
    /// Carried in the tally rather than asked of the caller so the identity
    /// below can be checked by whoever holds the value — including a reader on
    /// the far side of the wire, who has no way to know the phase otherwise.
    pub pending: u64,
    /// The most recently PAINTED fold, or `None` before the first one.
    ///
    /// A fold that was taken and not yet painted is deliberately absent here —
    /// this field answers "what did the last frame stand in for", and a frame
    /// that has not happened stood in for nothing.
    pub last: Option<Fold>,
}

impl ResizeTally {
    /// Does every noted event sit in exactly one bucket?
    ///
    /// This is the guarantee the type exists for: a resize is painted, or
    /// superseded, or a repeat, or still pending, and never none of those.
    #[must_use]
    pub fn is_balanced(&self) -> bool {
        self.events == self.painted + self.superseded + self.repeated + self.pending
    }

    /// Resizes that never became a frame of their own, over the lifetime.
    ///
    /// The reference toolkit's equivalent is not readable at all; here it is a
    /// field.
    #[must_use]
    pub fn unpainted(&self) -> u64 {
        self.superseded + self.repeated
    }
}

/// One window's pending resize plus its lifetime tally.
///
/// The event loop [`note`](Self::note)s every resize the window system
/// delivers — none are dropped at that layer, matching the reference — and
/// [`take`](Self::take)s the fold once per batch, when it is about to block
/// for more input.
#[derive(Clone, Debug, Default)]
pub struct ResizeBatch {
    pending: Option<Fold>,
    tally: ResizeTally,
}

impl ResizeBatch {
    /// An empty batch for a window that has not been resized.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a resize the window system delivered, and say what it did.
    pub fn note(&mut self, size: Size) -> Noted {
        self.tally.events += 1;
        match &mut self.pending {
            None => {
                self.pending = Some(Fold {
                    size,
                    opened_at: size,
                    superseded: 0,
                    repeated: 0,
                });
                self.tally.pending = 1;
                Noted::Opened
            }
            Some(fold) if fold.size == size => {
                fold.repeated += 1;
                self.tally.repeated += 1;
                Noted::Repeated
            }
            Some(fold) => {
                let replaced = fold.size;
                fold.size = size;
                fold.superseded += 1;
                self.tally.superseded += 1;
                Noted::Superseded { size: replaced }
            }
        }
    }

    /// Take the pending fold, if any, leaving the batch empty.
    ///
    /// **This does not count a frame.** The caller paints one, then reports it
    /// with [`Self::painted`]; between the two calls the batch is deliberately
    /// UNBALANCED, because in that interval the window is owed a frame it has
    /// not had.
    ///
    /// ★ The two-step shape is a counterfactual's doing. The first draft
    /// counted the frame here and its own doc comment claimed the balance
    /// identity would catch a caller that took a fold and never painted — it
    /// could not, because counting at the take is exactly what makes that
    /// caller's books add up. A break that removed the shell's paint call
    /// passed every check in the round. Splitting the two puts the assertion
    /// where it can fail.
    pub fn take(&mut self) -> Option<Fold> {
        let fold = self.pending.take()?;
        self.tally.pending = 0;
        Some(fold)
    }

    /// Record that a frame answering `fold` was painted.
    ///
    /// Called after the paint, never before: this is the half of
    /// [`Self::take`] that says the window got what it was owed, and the
    /// balance identity is false until it arrives.
    pub fn painted(&mut self, fold: Fold) {
        self.tally.painted += 1;
        self.tally.last = Some(fold);
    }

    /// The fold that would be drained now, without draining it.
    #[must_use]
    pub fn pending(&self) -> Option<&Fold> {
        self.pending.as_ref()
    }

    /// Is a resize waiting to be painted?
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// This window's lifetime accounting.
    #[must_use]
    pub fn tally(&self) -> &ResizeTally {
        &self.tally
    }

    /// Does every resize this batch has ever seen sit in exactly one bucket?
    ///
    /// Also checks that the tally's own `pending` term agrees with the batch —
    /// they are two spellings of one fact, and a drift between them is the way
    /// this accounting could lie while still adding up.
    #[must_use]
    pub fn is_balanced(&self) -> bool {
        self.tally.pending == u64::from(self.is_pending()) && self.tally.is_balanced()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1708_a_first_resize_opens_a_batch_and_discards_nothing() {
        let mut b = ResizeBatch::new();
        assert_eq!(b.note((800, 600)), Noted::Opened);
        let fold = b.pending().copied().expect("a resize is pending");
        assert_eq!(fold.size, (800, 600));
        assert_eq!(fold.opened_at, (800, 600));
        assert!(!fold.folded(), "one resize folded nothing");
        assert!(b.is_balanced());
    }

    #[test]
    fn r1708_a_later_size_supersedes_the_pending_one_and_names_it() {
        let mut b = ResizeBatch::new();
        b.note((800, 600));
        assert_eq!(b.note((804, 600)), Noted::Superseded { size: (800, 600) });
        assert_eq!(b.note((808, 600)), Noted::Superseded { size: (804, 600) });
        let fold = b.pending().copied().expect("a resize is pending");
        // The one frame this batch will paint carries the LAST size and
        // remembers where the span started.
        assert_eq!(fold.size, (808, 600));
        assert_eq!(fold.opened_at, (800, 600));
        assert_eq!(fold.superseded, 2);
        assert!(fold.folded());
        assert!(b.is_balanced());
    }

    #[test]
    fn r1708_a_repeat_of_the_pending_size_discards_nothing() {
        // The distinction the reference cannot express: re-announcing the same
        // size owed no separate frame, so nothing was thrown away. Folding it
        // into `superseded` would make an idle window look like a drag.
        let mut b = ResizeBatch::new();
        b.note((800, 600));
        assert_eq!(b.note((800, 600)), Noted::Repeated);
        let fold = b.pending().copied().expect("a resize is pending");
        assert_eq!(fold.superseded, 0);
        assert_eq!(fold.repeated, 1);
        assert!(!fold.folded());
        assert!(b.is_balanced());
    }

    #[test]
    fn r1708_a_repeat_after_a_supersede_is_still_a_repeat() {
        // `Repeated` is judged against the PENDING size, not the size the
        // batch opened at: 800 -> 804 -> 804 repeats, it does not re-open.
        let mut b = ResizeBatch::new();
        b.note((800, 600));
        b.note((804, 600));
        assert_eq!(b.note((804, 600)), Noted::Repeated);
        let fold = b.pending().copied().expect("a resize is pending");
        assert_eq!((fold.superseded, fold.repeated), (1, 1));
        assert!(b.is_balanced());
    }

    #[test]
    fn r1708_a_size_returning_to_the_one_that_opened_the_batch_supersedes() {
        // 800 -> 804 -> 800 is two supersedes, not a supersede and a repeat:
        // the pending size when the third arrives is 804, and 800 replaces it.
        // The batch still paints once, and `opened_at` still says 800.
        let mut b = ResizeBatch::new();
        b.note((800, 600));
        b.note((804, 600));
        assert_eq!(b.note((800, 600)), Noted::Superseded { size: (804, 600) });
        let fold = b.pending().copied().expect("a resize is pending");
        assert_eq!(fold.size, (800, 600));
        assert_eq!(fold.opened_at, (800, 600));
        assert_eq!(fold.superseded, 2);
        assert!(b.is_balanced());
    }

    #[test]
    fn r1708_taking_a_fold_empties_the_batch_and_counts_one_frame() {
        let mut b = ResizeBatch::new();
        b.note((800, 600));
        b.note((804, 600));
        let fold = b.take().expect("a fold was pending");
        assert_eq!(fold.size, (804, 600));
        assert!(!b.is_pending());
        assert!(b.take().is_none(), "a drained batch has nothing to give");
        b.painted(fold);
        assert_eq!(b.tally().painted, 1);
        assert!(b.is_balanced());
    }

    #[test]
    fn r1708_a_fold_taken_and_never_painted_leaves_the_books_unbalanced() {
        // ★★★★★ THE COUNTERFACTUAL'S TEST. R1708's first draft counted the
        // frame inside `take`, and said in its own doc comment that the balance
        // identity would catch a caller that took a fold and drew nothing.
        // It could not: counting at the take is precisely what makes that
        // caller's books add up. A break that deleted the shell's paint call
        // passed the whole round — core tests, the wire, and a three-screen
        // integration test driving real windows.
        //
        // Now the window is owed a frame between the two calls, and saying so
        // is the only thing that makes the published `balanced` worth reading.
        let mut b = ResizeBatch::new();
        b.note((800, 600));
        b.note((804, 600));
        let fold = b.take().expect("a fold was pending");
        assert!(
            !b.is_balanced(),
            "two events, no frame yet, nothing pending: the window is owed one"
        );
        assert_eq!(b.tally().painted, 0);
        b.painted(fold);
        assert!(b.is_balanced(), "and the report closes the books");
        assert_eq!(b.tally().painted, 1);
    }

    #[test]
    fn r1708_the_tally_accounts_for_every_event_across_many_batches() {
        // The headline guarantee: eighty events, three batches, and the four
        // buckets add back up to eighty.
        let mut b = ResizeBatch::new();
        let mut w = 800;
        for batch in 0..3 {
            for _ in 0..20 {
                b.note((w, 600));
                w += 4;
            }
            // One repeat per batch, so the `repeated` bucket is exercised in
            // the running total rather than only in isolation.
            b.note((w - 4, 600));
            assert!(b.is_balanced(), "balanced mid-batch {batch}");
            let fold = b.take().expect("each batch drains one fold");
            assert!(!b.is_balanced(), "owed a frame mid-drain {batch}");
            b.painted(fold);
            assert!(b.is_balanced(), "balanced after the frame {batch}");
        }
        let t = *b.tally();
        assert_eq!(t.events, 63);
        assert_eq!(t.painted, 3, "three batches -> three frames");
        assert_eq!(t.superseded, 57, "19 superseded per batch");
        assert_eq!(t.repeated, 3);
        assert_eq!(t.unpainted(), 60);
        assert_eq!(t.pending, 0);
        assert!(t.is_balanced());
    }

    #[test]
    fn r1708_an_undrained_batch_leaves_exactly_one_event_pending() {
        // The `pending` term in the identity is what keeps "balanced" honest
        // mid-drag instead of only at rest — and it travels WITH the tally, so
        // a reader on the far side of the wire can check the same identity.
        let mut b = ResizeBatch::new();
        b.note((800, 600));
        b.note((804, 600));
        let t = *b.tally();
        assert_eq!(t.pending, 1);
        assert_eq!((t.events, t.painted, t.superseded), (2, 0, 1));
        assert!(t.is_balanced());
        assert!(b.is_balanced());
        // Draining and then painting moves that one event from `pending` to
        // `painted`; the sum is unchanged, which is the point.
        let fold = b.take().expect("a fold was pending");
        b.painted(fold);
        let t = *b.tally();
        assert_eq!((t.pending, t.painted), (0, 1));
        assert!(t.is_balanced());
    }

    #[test]
    fn r1708_a_bare_tally_whose_pending_term_is_wrong_reports_unbalanced() {
        // The identity has to be able to FAIL, or publishing it says nothing.
        // A tally is a plain value on the wire, so this is the shape a reader
        // would see if the shell ever lost a resize.
        let lost = ResizeTally {
            events: 5,
            painted: 1,
            superseded: 2,
            repeated: 0,
            pending: 0,
            last: None,
        };
        assert!(!lost.is_balanced(), "two events went unaccounted for");
    }

    #[test]
    fn r1708_the_last_drained_fold_survives_for_a_reader() {
        // The wire read happens long after the drain; the fold has to outlive
        // the frame that answered it or an agent can never see what it folded.
        let mut b = ResizeBatch::new();
        assert_eq!(b.tally().last, None);
        b.note((800, 600));
        b.note((820, 600));
        let fold = b.take().expect("a fold was pending");
        assert_eq!(
            b.tally().last,
            None,
            "a fold not yet painted answered nothing"
        );
        b.painted(fold);
        b.note((900, 700));
        let last = b.tally().last.expect("a fold was painted");
        assert_eq!(last.size, (820, 600));
        assert_eq!(last.superseded, 1);
    }

    #[test]
    fn r1708_a_tally_round_trips_through_the_wire_form() {
        let mut b = ResizeBatch::new();
        b.note((800, 600));
        b.note((804, 600));
        let fold = b.take().expect("a fold was pending");
        b.painted(fold);
        let t = *b.tally();
        let json = serde_json::to_string(&t).expect("tally serialises");
        let back: ResizeTally = serde_json::from_str(&json).expect("and reads back");
        assert_eq!(back, t);
        assert_eq!(back.last.expect("a fold").opened_at, (800, 600));
    }
}
