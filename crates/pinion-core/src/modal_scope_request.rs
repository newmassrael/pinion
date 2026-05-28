//! R693 §5.39 — programmatic modal focus-trap request channel.
//!
//! The sibling of [`crate::focus_request`]: where that mailbox carries
//! a single "move focus to tag X" request, this one carries a modal
//! *scope* lifecycle event — open a focus trap over a set of member
//! tags, or close the topmost one.
//!
//! ## Model
//!
//! A widget body running under an active [`Owner`](crate::reactive::Owner)
//! scope (an [`External::invoke`](crate::external::External) handler, a
//! reducer body responding to a domain event) cannot reach
//! `pinion_runtime::FocusManager` — it lives in a downstream crate.
//! [`open`] / [`close`] write the request into a thread-local mailbox;
//! the shell substrate drains it at the same post-dispatch sync points
//! it drains [`crate::focus_request`] via [`drain`], applying the
//! request through `FocusManager::push_modal_scope` /
//! `pop_modal_scope`. Because the drain happens inside the dispatch's
//! `handle_tail` (right after the reducer ran), a reducer that opens a
//! dialog in response to a button intent sees the trap installed before
//! the next paint — the same single-frame guarantee the focus-request
//! mailbox gives.
//!
//! ## Why a reducer writes this, not a view-fn `Effect`
//!
//! A modal's open/closed state is edge-triggered: pushing a scope is a
//! one-shot action, not a per-frame level. A view-fn `Effect` that
//! watched an `open` signal would fire *after* the dispatch's
//! `handle_tail` drain point (Effects run at paint time), so the trap
//! would lag a frame and miss its drain. Writing the request from the
//! dispatch handler (reducer / `External::invoke`) — the same place
//! [`crate::focus_request`] is written — keeps the install in lockstep
//! with the state change that triggered it.
//!
//! ## Last-write-wins, single slot
//!
//! The mailbox holds one pending request. Open-then-close in the same
//! dispatch frame is degenerate (a dialog cannot open and close from a
//! single event); the final write wins, matching
//! [`crate::focus_request`]. Nested-modal stacking is expressed by
//! separate dispatch frames (each `open` is its own event), so the
//! single slot does not constrain depth — the stack lives in
//! `FocusManager`.

use std::cell::Cell;

/// A pending modal focus-trap lifecycle request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalRequest {
    /// Open a modal focus trap over `members` (the modal's focusable
    /// controls in Tab order). Maps to `FocusManager::push_modal_scope`.
    Open {
        /// Focusable tags inside the modal, in Tab order. Becomes the
        /// active focusable enumeration while the trap is topmost.
        members: Vec<String>,
    },
    /// Close the topmost modal focus trap. Maps to
    /// `FocusManager::pop_modal_scope`.
    Close,
}

thread_local! {
    /// Pending modal-scope request. Written by [`open`] / [`close`],
    /// read + cleared by [`drain`]. Last-write-wins; zero allocation on
    /// the empty-mailbox path (`Cell::take` is a single conditional
    /// pointer write when empty).
    static PENDING_MODAL_REQUEST: Cell<Option<ModalRequest>> = const { Cell::new(None) };
}

/// Request that a modal focus trap open over `members` at the next
/// shell drain point. Overwrites any prior pending request.
///
/// Typical call site: a reducer responding to a "open dialog" intent
/// that flips the application's `dialog_open` signal — it passes the
/// dialog's focusable control tags (in Tab order) so the shell
/// auto-focuses the first control and traps Tab inside the dialog.
pub fn open(members: Vec<String>) {
    PENDING_MODAL_REQUEST.with(|cell| cell.set(Some(ModalRequest::Open { members })));
}

/// Request that the topmost modal focus trap close at the next shell
/// drain point. Overwrites any prior pending request.
///
/// Typical call site: a reducer responding to a dialog dismissal (OK /
/// Cancel intent, Escape) that flips `dialog_open` back to `false` — the
/// shell pops the scope and restores the invoker's focus.
pub fn close() {
    PENDING_MODAL_REQUEST.with(|cell| cell.set(Some(ModalRequest::Close)));
}

/// Pop the pending modal-scope request from this thread's mailbox.
/// Returns `Some(req)` once per request — subsequent calls see `None`
/// until another [`open`] / [`close`] write.
///
/// Drained by the shell substrate after every dispatch path that might
/// have populated a request, alongside [`crate::focus_request::drain`].
#[must_use]
pub fn drain() -> Option<ModalRequest> {
    PENDING_MODAL_REQUEST.with(Cell::take)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_then_drain_returns_members() {
        let _ = drain();
        open(vec!["ok".to_string(), "cancel".to_string()]);
        assert_eq!(
            drain(),
            Some(ModalRequest::Open {
                members: vec!["ok".to_string(), "cancel".to_string()],
            })
        );
    }

    #[test]
    fn close_then_drain_returns_close() {
        let _ = drain();
        close();
        assert_eq!(drain(), Some(ModalRequest::Close));
    }

    #[test]
    fn drain_clears_the_mailbox() {
        open(vec!["a".to_string()]);
        let _ = drain();
        assert_eq!(drain(), None);
    }

    #[test]
    fn last_write_wins() {
        let _ = drain();
        open(vec!["a".to_string()]);
        close();
        assert_eq!(drain(), Some(ModalRequest::Close));
    }

    #[test]
    fn drain_on_empty_mailbox_returns_none() {
        let _ = drain();
        assert_eq!(drain(), None);
    }
}
