//! R1327 §5.39 — the READ direction of the focus channel: the currently
//! focused paint tag, published to bindings. (R1335 re-homed the mirror from a
//! thread-local to a per-binding [`Owner`](crate::reactive::Owner) scope; the
//! seam and its rationale are unchanged.)
//!
//! [`focus_request`](crate::focus_request) is the WRITE direction — a widget
//! body asks the focus manager to *move* focus (`request` → the shell's
//! `drain`). This module is its peer: the focus manager *publishes* where
//! focus actually IS, so a binding can read it from any paint-path code
//! (`WidgetCore::view`, `WidgetCore::reconcile_frame`, an
//! [`Effect`](crate::reactive::Effect)) and derive display state from it.
//!
//! ## Why the read direction has to exist
//!
//! Before R1327, focus reached a binding in two shapes, neither of which is the
//! binding-wide focused **tag** on the paint path:
//!
//! * as a **tag**, but only inside two hooks that run on someone else's schedule
//!   — `WidgetCore::apply_key(_, focused, …)` (only while a key is being pressed)
//!   and `WidgetView::access_node_for_window(_, focused)` (the a11y tree
//!   builder). Caching the `apply_key` argument goes stale the moment focus moves
//!   without a key — a click, Tab, a [`focus_request`](crate::focus_request), a
//!   modal opening — and deriving display state inside the a11y builder is a
//!   layer violation: that hook exists to describe the widget tree to assistive
//!   technology, and its cadence is the AT tree's, not the paint's.
//! * as a **per-External boolean**, via
//!   [`External::on_focus_change`](crate::External::on_focus_change) (R694) — the
//!   shell tells each External when *it* gains or loses focus, and a binding can
//!   thread that back into its state (`hello-toolbar` does). That is the right
//!   channel for "am *I* focused" widget posture, but it answers a different
//!   question from "*which* pane is active": it never carries the focused tag, it
//!   only reaches focus stops that are Externals (a plain `Scene::Container` pane
//!   cannot use it), and it needs per-widget wiring at every stop.
//!
//! So a binding-wide "who has focus" read — what a window title naming the active
//! pane, a status bar, or a tab-strip highlight needs — did not exist. This module
//! is it.
//!
//! ## Model — one writer, no drift
//!
//! `pinion_runtime::FocusManager` remains the SSOT. It is the *only* writer of
//! this mirror: every one of its mutators (Tab traversal, click-to-focus,
//! `focus/set` over RPC, a drained [`focus_request`](crate::focus_request), a
//! modal trap opening or closing, the stale-tag drop when a focused widget
//! leaves the paint scene, window-blur restore) commits through a single
//! private funnel that writes the mirror — on every commit *attempt*, so the
//! published value is self-healing. There is no per-focus-*mutation*-site
//! "remember to publish" discipline to forget: every mutator already routes
//! through that one funnel.
//!
//! There IS one setup step per backend, not per mutation: the shell hands the
//! manager its binding's root [`Owner`](crate::reactive::Owner) once at boot
//! (`FocusManager::attach_owner`, in both `pinion-shell` and `pinion-tui`). A
//! backend that never attaches simply publishes nothing (the manager's `owner`
//! stays `None`) — the mirror then reads its `None` default, which is a safe,
//! visible failure, not a drift. Both shipped backends attach symmetrically.
//!
//! That single publish site is also why it does not sit in the shell's own
//! observer wiring: `AppShell`'s focus mutations are spread across click, key,
//! RPC, request-drain and window-event paths, and that wiring had in fact already
//! missed one of them (`window_focused` → `FocusManager::restore` fired no
//! `notify_focus_change` until R1327 fixed it). Publishing from the state's owner
//! is the only placement a new *mutation* call site cannot bypass.
//!
//! ## Where the mirror lives — per binding, not per thread (R1335)
//!
//! The mirror is [`Owner::focused_tag_signal`](crate::reactive::Owner::focused_tag_signal),
//! an owner-scoped [`Signal`](crate::reactive::Signal), resolved through the active
//! [`Owner`](crate::reactive::Owner) scope. R1327 stored it in a thread-local,
//! which conflates the *binding's* focus with the *thread's*: two bindings on one
//! thread (test harnesses today; a Phase-D editor embedding a second pinion
//! binding tomorrow) would share — and clobber — one another's focus. Focus is a
//! per-binding fact, so it belongs on the binding's owner scope.
//!
//! It is **binding-wide across the owner *tree***, not just the root: a binding is
//! an owner tree, one focused tag spans all its windows, and
//! [`Owner::new_child`](crate::reactive::Owner::new_child) threads the root's
//! mirror handle down to every secondary-window child scope. So a `focused()` read
//! resolves the same value under a secondary window's scope as under the root, and
//! this holds structurally — not merely because every view happens to run under
//! the root owner today. (This is where focus DIFFERS from
//! [`use_viewport_size`](crate::reactive::use_viewport_size): the viewport size is
//! a per-owner *value* a secondary window can legitimately differ on, so it is a
//! per-owner cache slot; focus is one binding-wide fact, so it is a tree-shared
//! field.)
//!
//! `FocusManager` (which owns the binding's root [`Owner`](crate::reactive::Owner)
//! handle) writes the mirror **inside [`Owner::run`](crate::reactive::Owner::run)**
//! — the R1006 "blocker B" discipline: a
//! [`Signal::set`](crate::reactive::Signal::set) synchronously re-runs subscribed
//! [`Effect`](crate::reactive::Effect)s, and a woken Effect that reads an
//! owner-scoped hook resolves [`Owner::current`](crate::reactive::Owner::current),
//! so the write must happen with that scope on the stack. (The reference consumer
//! reads a *captured* handle, which needs no such scope — see [`focused`] on why an
//! Effect should; the wrap is the correct discipline for any subscriber that does,
//! and matches how the viewport signal is published.) Wrapping the write in the
//! manager's own `run` also means no focus-dispatch call site has to be
//! scope-aware.
//!
//! ## Reactive by construction
//!
//! The mirror is a [`Signal`](crate::reactive::Signal), so [`focused`] auto-subscribes whatever reactive
//! scope reads it (a view fn, an `Effect`) — a focus change re-runs it and
//! repaints, the same way [`theme::system_color_scheme`](crate::theme::system_color_scheme)
//! makes the OS color scheme reactive. Read from a non-reactive site (a
//! reducer body, `apply_key`) it is a plain read. Writes equality-skip, so a
//! focus mutation that lands on the already-focused tag costs nothing and
//! cannot loop a repaint.
//!
//! ## Single-valued, binding-wide
//!
//! The focused tag is binding-wide, not per-window: pinion's `FocusManager`
//! holds one focused tag over the R25.1 union enumeration across every window
//! (a secondary window's pane is a legitimate focus target, but only one tag is
//! focused at a time). A binding that cares *which window* the focused tag was
//! painted in resolves that from its own topology — the tag is the identity.

/// R1327 §5.39 — the currently focused paint tag, or `None` when nothing is
/// focused.
///
/// Resolves the active [`Owner`](crate::reactive::Owner) scope's focus mirror
/// ([`Owner::focused_tag_signal`](crate::reactive::Owner::focused_tag_signal))
/// and **auto-subscribes** the calling reactive scope. This is the convenience
/// hook for a **view fn** — `view`, `reconcile_frame`, `update` (the reducer) and
/// `apply_key` all run inside the binding's root `Owner` scope, so a focus change
/// re-runs the reader and schedules a repaint. That is the reactive path for a tab
/// strip highlighting the active pane, a status bar describing the focused
/// control, a window title naming the focused pane.
///
/// **Inside an [`Effect`](crate::reactive::Effect), capture the signal instead.**
/// An Effect body runs on the subscriber stack, not the owner-handle stack, so
/// [`Owner::current`](crate::reactive::Owner::current) resolves nothing when the
/// Effect is woken by a signal *other* than focus — and this hook would then
/// report "unfocused" spuriously. Capture the handle once
/// (`let sig = owner.focused_tag_signal();`) and read `sig.get()` in the body, the
/// same pattern [`use_viewport_size`](crate::reactive::use_viewport_size)'s reflow
/// Effect uses (`hello-dock-panels-editor`'s title-sync Effect is the reference).
///
/// **Graceful, not strict — a deliberate divergence from the sibling seams.**
/// Returns `None` — "no focus known" — when there is no active `Owner` scope at
/// all (a headless RPC probe, a bare unit test, any off-binding call). It cannot
/// silently mis-resolve a *wrong* scope: the mirror is inherited tree-wide
/// ([`Owner::new_child`](crate::reactive::Owner::new_child)), so a read under a
/// secondary window's child scope returns the binding-wide value, not an empty
/// child mirror — the one owner-scoped failure mode that `None` could otherwise
/// hide is closed by construction. So "no scope" genuinely means "no binding",
/// for which `None` is the honest answer.
///
/// [`use_viewport_size`](crate::reactive::use_viewport_size) and
/// [`use_pane_viewport_size`](crate::reactive::use_pane_viewport_size) instead
/// *panic* off-scope. The choice differs here because (a) the one misuse this seam
/// actually exhibits — a bare `focused()` inside a multi-subscription `Effect` — is
/// steered away from by the Effect note above rather than by a panic, and (b) a
/// read that can be reached from inside an `Effect`'s synchronous re-run should not
/// add a new panic site to that path. The trade-off is that such a misuse reads a
/// quiet `None` rather than aborting; the doc + the reference consumer's captured
/// handle are the guard.
///
/// The value is the focus manager's own state, so it agrees with `focus/get` over
/// RPC and — **on the GUI shell** — with the `focused` argument
/// `WidgetCore::apply_key` receives. The TUI backend does not route keys through
/// the focus manager at all: it hands `apply_key` its primary surface's tag
/// unconditionally, a §2 #6 asymmetry that predates this seam and is tracked on
/// the TUI substrate. `focused()` reports the focus manager's tag on both.
///
/// To *move* focus, a binding calls [`focus_request::request`](crate::focus_request::request);
/// this is strictly the read direction.
#[must_use]
pub fn focused() -> Option<String> {
    crate::reactive::Owner::current().and_then(|owner| owner.focused_tag_signal().get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::{Effect, Owner};
    use std::cell::RefCell as StdRefCell;
    use std::rc::Rc;

    // The focus mirror is owner-scoped (R1335): `focused()` resolves the current
    // Owner scope's signal, and `pinion_runtime::FocusManager` publishes into it.
    // These tests stand in for the manager by writing the owner's
    // `focused_tag_signal()` directly — inside `owner.run(...)` where a
    // subscriber must re-run, matching the manager's own self-wrapped write.

    #[test]
    fn r1335_no_owner_scope_reads_none() {
        // Off any binding (a headless probe, a bare unit call) there is no focus
        // to know — graceful None, never a panic.
        assert_eq!(focused(), None);
    }

    #[test]
    fn r1335_default_is_unfocused() {
        let owner = Owner::new();
        assert_eq!(owner.run(focused), None, "a fresh owner's mirror is empty");
    }

    #[test]
    fn r1335_publish_is_read_back() {
        let owner = Owner::new();
        owner.focused_tag_signal().set(Some("editor".to_owned()));
        assert_eq!(owner.run(focused).as_deref(), Some("editor"));
        owner.focused_tag_signal().set(None);
        assert_eq!(owner.run(focused), None);
    }

    #[test]
    fn r1335_each_binding_has_its_own_mirror() {
        // The R1335 reason the mirror left the thread-local: two bindings (two
        // detached owner trees) on one thread must not clobber one another's focus.
        let a = Owner::new();
        let b = Owner::new();
        a.focused_tag_signal().set(Some("a-pane".to_owned()));
        b.focused_tag_signal().set(Some("b-pane".to_owned()));
        assert_eq!(a.run(focused).as_deref(), Some("a-pane"));
        assert_eq!(b.run(focused).as_deref(), Some("b-pane"), "no cross-talk");
    }

    #[test]
    fn r1335_child_scope_shares_the_binding_wide_mirror() {
        // Focus is binding-wide: a secondary window's child scope reads the SAME
        // mirror the root publishes to (Owner::new_child inherits the handle), so
        // `focused()` under the child resolves the value the FocusManager wrote to
        // the root — even though the child owner is what's on the stack. This is
        // what keeps `focused()` correct if a view ever runs under a per-window
        // child scope (the deferred R680 atomic 1 wrap-flip).
        let root = Owner::new();
        let child = Owner::new_child(&root);
        // The manager writes the ROOT mirror; the child must see it.
        root.focused_tag_signal().set(Some("outliner".to_owned()));
        assert_eq!(
            child.run(focused).as_deref(),
            Some("outliner"),
            "child scope resolves the binding-wide focus, not an empty child mirror",
        );
        // And the child does not get its OWN isolated mirror: a write through the
        // child's handle is the same cell the root reads.
        child.focused_tag_signal().set(Some("console".to_owned()));
        assert_eq!(root.run(focused).as_deref(), Some("console"));
    }

    #[test]
    fn r1335_reactive_scope_reruns_on_focus_change() {
        let owner = Owner::new();
        let signal = owner.focused_tag_signal();
        let seen: Rc<StdRefCell<Vec<Option<String>>>> = Rc::new(StdRefCell::new(Vec::new()));
        let seen_e = Rc::clone(&seen);
        let _effect = owner.run(|| {
            Effect::new(&owner, move || {
                seen_e.borrow_mut().push(focused());
            })
        });
        assert_eq!(seen.borrow().as_slice(), &[None], "eager first run");

        // Writes are wrapped in the owner scope so the synchronous Effect re-run
        // resolves `Owner::current()` (the publish-side "blocker B" discipline).
        owner.run(|| signal.set(Some("outliner".to_owned())));
        owner.run(|| signal.set(Some("console".to_owned())));
        assert_eq!(
            seen.borrow().as_slice(),
            &[
                None,
                Some("outliner".to_string()),
                Some("console".to_string())
            ],
            "every focus change re-runs the subscriber",
        );
    }

    #[test]
    fn r1335_republishing_the_same_tag_is_a_no_op() {
        let owner = Owner::new();
        let signal = owner.focused_tag_signal();
        let runs = Rc::new(StdRefCell::new(0_u32));
        let runs_e = Rc::clone(&runs);
        let _effect = owner.run(|| {
            Effect::new(&owner, move || {
                let _ = focused();
                *runs_e.borrow_mut() += 1;
            })
        });
        assert_eq!(*runs.borrow(), 1);
        owner.run(|| signal.set(Some("outliner".to_owned())));
        assert_eq!(*runs.borrow(), 2);
        owner.run(|| signal.set(Some("outliner".to_owned())));
        assert_eq!(
            *runs.borrow(),
            2,
            "an equal publish notifies nobody (no repaint churn on a re-click)",
        );
    }

    #[test]
    fn r1335_subscriber_may_read_focus_re_entrantly() {
        // An Effect deriving display state from focus reads `focused()` inside
        // the notify triggered by the write — the storage must not hold a borrow
        // across the notify.
        let owner = Owner::new();
        let signal = owner.focused_tag_signal();
        let echo: Rc<StdRefCell<Option<String>>> = Rc::new(StdRefCell::new(None));
        let echo_e = Rc::clone(&echo);
        let _effect = owner.run(|| {
            Effect::new(&owner, move || {
                let tag = focused();
                // A second read inside the same notify — the re-entrant path.
                let again = focused();
                assert_eq!(tag, again);
                *echo_e.borrow_mut() = again;
            })
        });
        owner.run(|| signal.set(Some("viewport".to_owned())));
        assert_eq!(echo.borrow().as_deref(), Some("viewport"));
    }
}
