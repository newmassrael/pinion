//! R788 §5.39 §5.16 — the modal **open-lifecycle** reactive holder: the
//! lifted SSOT for "is this modal up, and is its focus trap installed".
//!
//! ## Why this exists (the 3rd-consumer lift)
//!
//! [`hello-dialog`](../../../../examples/hello-dialog) (R693) and
//! [`hello-drawer`](../../../../examples/hello-drawer) (R702) each grew the
//! *same* two-line coupling by hand: a `Signal<bool>` "open" flag plus the
//! [`crate::modal_scope_request`] focus-trap lifecycle, flipped together —
//!
//! ```text
//! open : open_flag.set(true)  ; modal_scope_request::open(members)
//! close: open_flag.set(false) ; modal_scope_request::close()
//! ```
//!
//! That pairing is **correctness-critical, not incidental**: flip the flag
//! without moving the scope (or vice versa) and the painted modal desyncs
//! from the focus trap — the background becomes Tab-reachable behind a
//! visible scrim, or a dismissed dialog leaves Tab trapped on nothing
//! (`divergence-is-a-bug`). When R788's modal file-open dialog arrived as
//! the **third** consumer, that mechanical 3-copy crossed the
//! `[[three-site-internal-duplication-substrate-lift]]` threshold, so the
//! coupling lives here once. The per-binding *outcome* state (the dialog's
//! accepted/cancelled result, the drawer's active destination, the file
//! picker's chosen path) stays in each binding — that is policy, not the
//! mechanism, and it is where the three genuinely diverge.
//!
//! ## What is *not* here
//!
//! - The focus trap's auto-focus / Tab-confinement / restore-on-close: that
//!   is `pinion_runtime::FocusManager`, driven by the
//!   [`crate::modal_scope_request`] the shell drains. [`ModalState`] only
//!   *requests* it in lockstep with the open flag.
//! - The modal's *members* (its focusable controls in Tab order): passed at
//!   [`ModalState::open`] time, since they are the binding's tags.
//! - The scrim / panel chrome: `pinion_widget_paint::dialog` /
//!   `::drawer`.

use std::rc::Rc;

use crate::reactive::{Owner, Signal};

/// R788 — the open-lifecycle of one modal surface (dialog / drawer / file
/// picker). Holds the reactive `open` flag; [`open`](Self::open) /
/// [`close`](Self::close) move it **and** the [`crate::modal_scope_request`]
/// focus-trap lifecycle together, so the two can never drift apart.
///
/// Resolve the shared instance with [`use_modal`] (one `Rc` across the
/// view-fn that reads [`is_open`](Self::is_open), the reducer that calls
/// [`open`](Self::open) / [`close`](Self::close), and `access_node`).
#[derive(Debug)]
pub struct ModalState {
    open: Signal<bool>,
}

impl ModalState {
    /// A fresh, closed modal.
    #[must_use]
    pub fn new() -> Self {
        Self { open: Signal::new(false) }
    }

    /// Whether the modal is currently up. Subscribes when read inside a
    /// view-fn / `access_node` (so opening or closing repaints).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Open the modal: raise the flag **and** install a focus trap over
    /// `members` (the modal's focusable controls in Tab order — the shell
    /// auto-focuses the first and confines Tab to them). Call from a
    /// reducer / `External::invoke` body, where the request drains in the
    /// same `handle_tail` so the trap is up before the next paint.
    pub fn open(&self, members: Vec<String>) {
        self.open.set(true);
        crate::modal_scope_request::open(members);
    }

    /// Close the modal: lower the flag **and** pop the focus trap
    /// (restoring focus to the invoker). The mirror of [`open`](Self::open);
    /// the per-binding outcome (accepted / cancelled / chosen) is the
    /// caller's to record alongside.
    pub fn close(&self) {
        self.open.set(false);
        crate::modal_scope_request::close();
    }
}

impl Default for ModalState {
    fn default() -> Self {
        Self::new()
    }
}

/// R788 — resolve the shared [`ModalState`] for `key`, building it once
/// (closed). Mirrors [`use_directory_state`](crate::widgets::file_browser::use_directory_state)
/// / [`use_column_widths`](crate::widgets::column_widths::use_column_widths):
/// the view, the reducer, and `access_node` all call this with the same
/// `key` and receive the same `Rc`, so the open flag is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set — call from within a view /
/// reducer / `create_extra_externals` hook (all run inside a
/// `root_owner.run`).
#[must_use]
pub fn use_modal(key: &'static str) -> Rc<ModalState> {
    Owner::current()
        .expect("use_modal requires an active Owner scope")
        .cache(key, ModalState::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modal_scope_request::{self, ModalRequest};

    #[test]
    fn r788_starts_closed() {
        Owner::new().run(|| {
            assert!(!use_modal("m").is_open(), "a fresh modal is closed");
        });
    }

    #[test]
    fn r788_open_raises_flag_and_requests_trap() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            let m = use_modal("m");
            m.open(vec!["ok".to_string(), "cancel".to_string()]);
            assert!(m.is_open(), "open raises the flag");
            assert_eq!(
                modal_scope_request::drain(),
                Some(ModalRequest::Open {
                    members: vec!["ok".to_string(), "cancel".to_string()],
                }),
                "open installs the focus trap over the members in lockstep",
            );
        });
    }

    #[test]
    fn r788_close_lowers_flag_and_pops_trap() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            let m = use_modal("m");
            m.open(vec!["ok".to_string()]);
            let _ = modal_scope_request::drain();
            m.close();
            assert!(!m.is_open(), "close lowers the flag");
            assert_eq!(
                modal_scope_request::drain(),
                Some(ModalRequest::Close),
                "close pops the focus trap in lockstep",
            );
        });
    }

    #[test]
    fn r788_same_key_shares_one_flag() {
        Owner::new().run(|| {
            let a = use_modal("shared");
            let b = use_modal("shared");
            a.open(vec!["x".to_string()]);
            assert!(b.is_open(), "the second handle observes the first's open");
        });
    }
}
