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
//! ## An ordered queue, not a single slot (R1456)
//!
//! The mailbox keeps **every** request written during a dispatch frame,
//! in call order, and the shell applies them in that order.
//!
//! This is where it parts company with its sibling. [`crate::focus_request`]
//! carries a **level** — "focus should end up at tag X" — so collapsing
//! successive writes to the last one is lossless: the final write *is*
//! the intended end state. A [`ModalRequest`] carries a **stack edit**
//! (push / pop), and edits do not collapse: `close` then `open` composes
//! to "replace the top scope", which is not the same as `open`. Until
//! R1456 this module held one `Cell` slot and inherited `focus_request`'s
//! last-write-wins policy across that type boundary, so the `close` was
//! silently dropped and its scope stayed on the stack forever.
//!
//! The axis that exposed it is **handoff**, not nesting:
//!
//! - *Nesting* stacks B on top of a still-open A. Each `open` comes from
//!   its own user action, hence its own dispatch frame — one request per
//!   frame, which a single slot happens to survive.
//! - *Handoff* closes A **and** opens B: a command palette or menu whose
//!   destructive row hands off to a confirm dialog (Qt's menu action →
//!   [`QMessageBox::question`]; every toolkit ships it). That is one user
//!   action by definition, so it is one dispatch frame with two edits,
//!   and "express it in separate frames" has nothing to apply to.
//!
//! Queueing also makes the case the old doc called degenerate
//! (`open` then `close` in one frame) well-defined — the trap opens and
//! immediately lifts — rather than silently keeping only the `close`.
//!
//! [`QMessageBox::question`]: https://doc.qt.io/qt-6/qmessagebox.html

use std::cell::RefCell;

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
    /// Modal-scope requests pending for the next drain, in call order.
    /// Appended by [`open`] / [`close`], taken + cleared by [`drain`].
    /// Zero allocation on the empty-mailbox steady state: `RefCell::take`
    /// swaps in `Vec::new`, which does not allocate, and dropping an
    /// empty `Vec` frees nothing.
    static PENDING_MODAL_REQUESTS: RefCell<Vec<ModalRequest>> = const {
        RefCell::new(Vec::new())
    };
}

/// Queue a request that a modal focus trap open over `members` at the
/// next shell drain point, after any request already pending this frame.
///
/// Typical call site: a reducer responding to a "open dialog" intent
/// that flips the application's `dialog_open` signal — it passes the
/// dialog's focusable control tags (in Tab order) so the shell
/// auto-focuses the first control and traps Tab inside the dialog.
pub fn open(members: Vec<String>) {
    PENDING_MODAL_REQUESTS.with_borrow_mut(|q| q.push(ModalRequest::Open { members }));
}

/// Queue a request that the topmost modal focus trap close at the next
/// shell drain point, after any request already pending this frame.
///
/// Typical call site: a reducer responding to a dialog dismissal (OK /
/// Cancel intent, Escape) that flips `dialog_open` back to `false` — the
/// shell pops the scope and restores the invoker's focus. A reducer
/// handing one modal off to another calls this and then [`open`] in the
/// same body; both edits survive (see the module-level handoff note).
pub fn close() {
    PENDING_MODAL_REQUESTS.with_borrow_mut(|q| q.push(ModalRequest::Close));
}

/// Take every request pending on this thread, in the order they were
/// written. Empty when nothing is pending; the mailbox is cleared, so a
/// second call sees nothing until another [`open`] / [`close`].
///
/// Drained by the shell substrate after every dispatch path that might
/// have populated a request, alongside [`crate::focus_request::drain`].
/// Returning the whole batch is the contract, not a convenience: the
/// requests are stack edits, so a caller that applied only the first
/// would leave the rest of the frame's intent on the floor — which is
/// exactly the R1456 bug this shape retires.
#[must_use]
pub fn drain() -> Vec<ModalRequest> {
    PENDING_MODAL_REQUESTS.with(RefCell::take)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn open_then_drain_returns_members() {
        let _ = drain();
        open(tags(&["ok", "cancel"]));
        assert_eq!(
            drain(),
            vec![ModalRequest::Open {
                members: tags(&["ok", "cancel"]),
            }]
        );
    }

    #[test]
    fn close_then_drain_returns_close() {
        let _ = drain();
        close();
        assert_eq!(drain(), vec![ModalRequest::Close]);
    }

    #[test]
    fn drain_clears_the_mailbox() {
        open(tags(&["a"]));
        let _ = drain();
        assert_eq!(drain(), Vec::new());
    }

    /// R1456 — the handoff a single slot could not express: one dispatch
    /// frame closes modal A and opens modal B. Both edits survive, in
    /// call order, so the drain pops A before pushing B.
    #[test]
    fn close_then_open_in_one_frame_keeps_both_in_order() {
        let _ = drain();
        close();
        open(tags(&["b"]));
        assert_eq!(
            drain(),
            vec![
                ModalRequest::Close,
                ModalRequest::Open {
                    members: tags(&["b"]),
                },
            ]
        );
    }

    /// The reverse order the old doc called degenerate is now simply
    /// well-defined: open, then immediately close, both applied.
    #[test]
    fn open_then_close_in_one_frame_keeps_both_in_order() {
        let _ = drain();
        open(tags(&["a"]));
        close();
        assert_eq!(
            drain(),
            vec![
                ModalRequest::Open {
                    members: tags(&["a"]),
                },
                ModalRequest::Close,
            ]
        );
    }

    /// Nesting is unchanged: one `open` per dispatch frame, each drained
    /// on its own, is still one request per drain.
    #[test]
    fn nesting_across_frames_drains_one_request_each() {
        let _ = drain();
        open(tags(&["a"]));
        assert_eq!(
            drain(),
            vec![ModalRequest::Open {
                members: tags(&["a"]),
            }]
        );
        open(tags(&["b"]));
        assert_eq!(
            drain(),
            vec![ModalRequest::Open {
                members: tags(&["b"]),
            }]
        );
    }

    #[test]
    fn drain_on_empty_mailbox_returns_nothing() {
        let _ = drain();
        assert_eq!(drain(), Vec::new());
    }
}
