//! R1327 §5.39 — the READ direction of the focus channel: the currently
//! focused paint tag, published to bindings.
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
//! private funnel that calls [`publish`] — on every commit *attempt*, so the
//! published value is self-healing. There is no per-call-site "remember to
//! publish" discipline to forget.
//!
//! That is also why the publish does not sit in the shell: `AppShell`'s focus
//! mutations are spread across click, key, RPC, request-drain and window-event
//! paths, and its own observer wiring had in fact already missed one of them
//! (`window_focused` → `FocusManager::restore` fired no `notify_focus_change`
//! until R1327 fixed it). Publishing from the state's owner is the only placement
//! a new call site cannot bypass.
//!
//! ## Reactive by construction
//!
//! The mirror is a [`Signal`], so [`focused`] auto-subscribes whatever reactive
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

use crate::reactive::Signal;

thread_local! {
    /// R1327 §5.39 — the focused paint tag, mirrored from the
    /// `pinion_runtime::FocusManager` that owns it. One per UI thread; written
    /// only by [`publish`] (i.e. only by the focus manager), read by every
    /// binding that derives display state from focus.
    ///
    /// Same shape as [`theme`](crate::theme)'s OS color-scheme signal: a bare
    /// `Signal`, so a subscriber woken by [`publish`] (Effects re-run
    /// synchronously) can read [`focused`] again from inside its own notify — an
    /// Effect deriving a window title from focus does exactly that.
    static FOCUSED: Signal<Option<String>> = Signal::new(None);
}

/// R1327 §5.39 — the currently focused paint tag, or `None` when nothing is
/// focused.
///
/// **Auto-subscribes** the calling reactive scope — and in practice every binding
/// entry point is one: `view`, `reconcile_frame`, `update` (the reducer) and
/// `apply_key` all run inside the binding's root `Owner` scope, so a focus change
/// re-runs the reader and schedules a repaint. That is the reactive path for a tab
/// strip highlighting the active pane, a status bar describing the focused
/// control, a window title naming the focused pane. (There is no untracked read:
/// the subscription is harmless — a focus change already requests a redraw — but
/// it is not optional.)
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
    FOCUSED.with(Signal::get)
}

/// R1327 §5.39 — publish the focused tag. **Framework ingress: application
/// code must not call this.**
///
/// The sole caller is `pinion_runtime::FocusManager`, which publishes from its
/// single write funnel on every focus commit *attempt* — so a stray write from
/// application code does not move focus, and is overwritten by the next focus
/// operation even if that operation moves nothing (re-focusing the focused tag
/// re-asserts the truth). A binding that wants focus to move calls
/// [`focus_request::request`](crate::focus_request::request).
///
/// `pub` rather than `pub(crate)` only because the focus manager lives in a
/// sibling crate that the closed-core dependency direction (§6.3) keeps
/// downstream of `pinion-core` — the same reason
/// [`theme::set_system_color_scheme`](crate::theme::set_system_color_scheme) is
/// public.
///
/// Equality-skips: publishing the already-published tag notifies no subscriber.
pub fn publish(tag: Option<&str>) {
    let value = tag.map(str::to_owned);
    FOCUSED.with(|signal| signal.set(value));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::{Effect, Owner};
    use std::cell::RefCell as StdRefCell;
    use std::rc::Rc;

    #[test]
    fn r1327_default_is_unfocused() {
        assert_eq!(focused(), None);
    }

    #[test]
    fn r1327_publish_is_read_back() {
        publish(Some("editor"));
        assert_eq!(focused().as_deref(), Some("editor"));
        publish(None);
        assert_eq!(focused(), None);
    }

    #[test]
    fn r1327_reactive_scope_reruns_on_focus_change() {
        publish(None);
        let owner = Owner::new();
        let seen: Rc<StdRefCell<Vec<Option<String>>>> = Rc::new(StdRefCell::new(Vec::new()));
        let seen_e = Rc::clone(&seen);
        let _effect = owner.run(|| {
            Effect::new(&owner, move || {
                seen_e.borrow_mut().push(focused());
            })
        });
        assert_eq!(seen.borrow().as_slice(), &[None], "eager first run");

        publish(Some("outliner"));
        publish(Some("console"));
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
    fn r1327_republishing_the_same_tag_is_a_no_op() {
        publish(None);
        let owner = Owner::new();
        let runs = Rc::new(StdRefCell::new(0_u32));
        let runs_e = Rc::clone(&runs);
        let _effect = owner.run(|| {
            Effect::new(&owner, move || {
                let _ = focused();
                *runs_e.borrow_mut() += 1;
            })
        });
        assert_eq!(*runs.borrow(), 1);
        publish(Some("outliner"));
        assert_eq!(*runs.borrow(), 2);
        publish(Some("outliner"));
        assert_eq!(
            *runs.borrow(),
            2,
            "an equal publish notifies nobody (no repaint churn on a re-click)",
        );
    }

    #[test]
    fn r1327_subscriber_may_read_focus_re_entrantly() {
        // An Effect deriving display state from focus reads `focused()` inside
        // the notify triggered by `publish` — the storage must not hold a borrow
        // across the notify.
        publish(None);
        let owner = Owner::new();
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
        publish(Some("viewport"));
        assert_eq!(echo.borrow().as_deref(), Some("viewport"));
    }
}
