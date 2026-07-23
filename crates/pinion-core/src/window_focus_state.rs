//! R1419 §5.39 §5.16 — the READ direction of the OS-window-focus channel: the
//! id of the OS window that currently holds the OS keyboard focus, published to
//! bindings so a paint-path read can derive display state from it.
//!
//! This is the OS-focus **peer of [`focus_state`](crate::focus_state)**, which
//! publishes the focused paint *tag*. The two answer different questions on the
//! two focus axes a binding cares about:
//!
//! * [`focus_state::focused`](crate::focus_state::focused) — *which widget/pane
//!   inside this binding* has focus (Tab / click / `focus/set` move it). Changes
//!   as the user moves focus WITHIN the application.
//! * [`os_focused_window`] — *which of this binding's OS windows* the window
//!   manager has activated, or `None` when the whole application is blurred (the
//!   user alt-tabbed to another OS app). Changes when OS focus enters or leaves
//!   the application, independent of the within-app focused tag.
//!
//! A binding reads this from any paint-path code (`WidgetCore::view`,
//! `WidgetCore::reconcile_frame`, an [`Effect`](crate::reactive::Effect)) to dim
//! on blur, re-arm a caret on refocus, or (a terminal multiplexer) gate a child
//! `FocusOut`/`FocusIn` report on whole-window blur — the OS-focus counterpart of
//! [`theme::system_color_scheme`](crate::theme::system_color_scheme) making the OS
//! colour scheme reactive.
//!
//! ## Why the read direction has to exist
//!
//! Before R1419 the OS-focused window reached a binding in two shapes, neither of
//! which is a paint-path read:
//!
//! * as an **internal key-dispatch gate** — `pinion-shell`'s `ShellCore` tracks
//!   the OS-focused window in a private `os_focused_window` field and consults it
//!   in `is_key_dispatch_window` to route keys in a multi-window binding. That is
//!   shell-internal plumbing; a view fn cannot read it.
//! * as an **external RPC observation** — the R1074 `scene/input_state` method
//!   renders `{os_focused_window, key_press_owners}` so an AI *client* can observe
//!   the gate. That is the wire for an outside observer, not an in-binding read on
//!   the paint path.
//!
//! [`External::on_focus_change`](crate::External::on_focus_change) (R694) does
//! NOT fill the gap: it is *widget* focus ("am I, this External, focused"), and
//! the OS-window-blur path in the shell (`window_blurred`) does not route through
//! the `notify_focus_change` observer that drives it — a blurred window keeps its
//! focused widget (only snapshotting it for restore), so `on_focus_change` never
//! fires on OS blur. So a binding-wide "which OS window is focused" read on the
//! paint path — what a whole-window dim, or a native pane focus reporter, needs —
//! did not exist. This module is it.
//!
//! ## Model — one writer, no drift
//!
//! `pinion-shell`'s `ShellCore` is the SSOT and the only writer: it owns the
//! single `os_focused_window` field (maintained from winit `WindowEvent::Focused`
//! and the window-destruction reconcile) and publishes every mutation through one
//! private funnel (`set_os_focused_window`) that writes this mirror — on every
//! write, so the published value is self-healing. There is no per-call-site
//! "remember to publish" discipline to forget.
//!
//! `pinion-tui` never writes it: a single full-screen terminal surface has no
//! OS-window-focus gate. On the TUI the mirror keeps its `None` default —
//! "unknown", a safe, visible non-answer (the same shape a headless probe or a
//! single-window binding before its first focus event reads), not a drift.
//!
//! ## Where the mirror lives — per binding, carrying the window IDENTITY
//!
//! The mirror is
//! [`Owner::os_focused_window_signal`](crate::reactive::Owner::os_focused_window_signal),
//! an owner-scoped [`Signal`](crate::reactive::Signal) resolved through the active
//! [`Owner`](crate::reactive::Owner) scope — the exact shape of the R1335
//! `focused_tag` mirror. It is **binding-wide across the owner tree**:
//! [`Owner::new_child`](crate::reactive::Owner::new_child) threads the root's
//! handle down to every secondary-window child scope, so an `os_focused_window()`
//! read resolves the same value under a secondary window's scope as under the
//! root, and the shell's single publish is seen tree-wide.
//!
//! The value is the **window id** (`Option<String>`), not a bool — the peer shape
//! of `focused_tag` (a widget tag) and of the shell's own `os_focused_window`
//! field and the R1074 RPC leg (both `Option<String>`). A binding with more than
//! one OS window (a tear-off floating window) compares the id against the window
//! it is painting to decide "is *my* window focused"; a bool would discard that.
//! The overwhelmingly common single-OS-window binding derives the bool it wants
//! with `os_focused_window() == Some(my_window_id)` or `.is_some()`.
//!
//! ## Reactive by construction
//!
//! The mirror is a [`Signal`](crate::reactive::Signal), so [`os_focused_window`]
//! auto-subscribes whatever reactive scope reads it (a view fn, an `Effect`) — an
//! OS-focus change re-runs it and repaints, the same way
//! [`theme::system_color_scheme`](crate::theme::system_color_scheme) makes the OS
//! colour scheme reactive. Read from a non-reactive site it is a plain read.
//! Writes equality-skip, so a redundant re-publish of the same window costs
//! nothing and cannot loop a repaint.

/// R1419 §5.39 §5.16 — the id of the OS window that currently holds the OS
/// keyboard focus, or `None` when the application is blurred (or the OS-focus
/// state is unknown — a headless probe, a backend with no OS-window-focus gate,
/// or before the first focus event).
///
/// Resolves the active [`Owner`](crate::reactive::Owner) scope's OS-focus mirror
/// ([`Owner::os_focused_window_signal`](crate::reactive::Owner::os_focused_window_signal))
/// and **auto-subscribes** the calling reactive scope. This is the convenience
/// hook for a **view fn** — `view`, `reconcile_frame`, `update` (the reducer) and
/// `apply_key` all run inside the binding's root `Owner` scope, so an OS-focus
/// change re-runs the reader and schedules a repaint. That is the reactive path
/// for a whole-window dim on blur, a native pane focus reporter, or a caret that
/// stops blinking when the application loses OS focus.
///
/// **Inside an [`Effect`](crate::reactive::Effect), capture the signal instead.**
/// An Effect body runs on the subscriber stack, not the owner-handle stack, so
/// [`Owner::current`](crate::reactive::Owner::current) resolves nothing when the
/// Effect is woken by a signal *other* than OS focus — and this hook would then
/// report "blurred" spuriously. Capture the handle once
/// (`let sig = owner.os_focused_window_signal();`) and read `sig.get()` in the
/// body, the same pattern
/// [`focus_state::focused`](crate::focus_state::focused) documents for its own
/// mirror.
///
/// **Graceful, not strict.** Returns `None` — "no OS focus known" — when there is
/// no active `Owner` scope at all (a headless RPC probe, a bare unit test, any
/// off-binding call), matching [`focus_state::focused`](crate::focus_state::focused).
/// It cannot silently mis-resolve a *wrong* scope: the mirror is inherited
/// tree-wide ([`Owner::new_child`](crate::reactive::Owner::new_child)), so a read
/// under a secondary window's child scope returns the binding-wide value, not an
/// empty child mirror. `None` genuinely means "OS focus unknown", which — for the
/// question "does my window hold OS focus" — is honestly answered "not known to".
///
/// The value is the shell's own `os_focused_window`, so it agrees with the
/// `os_focused_window` leg the R1074 `scene/input_state` RPC method renders and
/// with the `is_key_dispatch_window` gate the shell routes keys on.
#[must_use]
pub fn os_focused_window() -> Option<String> {
    crate::reactive::Owner::current().and_then(|owner| owner.os_focused_window_signal().get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::{Effect, Owner};
    use std::cell::RefCell as StdRefCell;
    use std::rc::Rc;

    // The OS-focus mirror is owner-scoped (R1419): `os_focused_window()` resolves
    // the current Owner scope's signal, and `pinion-shell`'s `ShellCore` publishes
    // into it. These tests stand in for the shell by writing the owner's
    // `os_focused_window_signal()` directly — inside `owner.run(...)` where a
    // subscriber must re-run, matching the shell's own self-wrapped write.

    #[test]
    fn no_owner_scope_reads_none() {
        // Off any binding (a headless probe, a bare unit call) there is no OS
        // focus to know — graceful None, never a panic.
        assert_eq!(os_focused_window(), None);
    }

    #[test]
    fn default_is_blurred() {
        let owner = Owner::new();
        assert_eq!(
            owner.run(os_focused_window),
            None,
            "a fresh owner's OS-focus mirror is unknown/blurred",
        );
    }

    #[test]
    fn publish_is_read_back() {
        let owner = Owner::new();
        owner
            .os_focused_window_signal()
            .set(Some("main".to_owned()));
        assert_eq!(owner.run(os_focused_window).as_deref(), Some("main"));
        owner.os_focused_window_signal().set(None);
        assert_eq!(owner.run(os_focused_window), None, "blur reads back None");
    }

    #[test]
    fn each_binding_has_its_own_mirror() {
        // Two bindings (two detached owner trees) on one thread must not clobber
        // one another's OS focus — the same reason the focused-tag mirror is
        // per-binding (R1335), applied to the OS-focus axis.
        let a = Owner::new();
        let b = Owner::new();
        a.os_focused_window_signal().set(Some("app-a".to_owned()));
        b.os_focused_window_signal().set(Some("app-b".to_owned()));
        assert_eq!(a.run(os_focused_window).as_deref(), Some("app-a"));
        assert_eq!(
            b.run(os_focused_window).as_deref(),
            Some("app-b"),
            "no cross-talk",
        );
    }

    #[test]
    fn child_scope_shares_the_binding_wide_mirror() {
        // OS focus is binding-wide: a secondary window's child scope reads the
        // SAME mirror the root publishes to (Owner::new_child inherits the
        // handle), so a tear-off floating window resolves the shell's truth even
        // though its own child owner is on the stack — and can then compare the
        // id against its own window.
        let root = Owner::new();
        let child = Owner::new_child(&root);
        root.os_focused_window_signal()
            .set(Some("floating-1".to_owned()));
        assert_eq!(
            child.run(os_focused_window).as_deref(),
            Some("floating-1"),
            "child scope resolves the binding-wide OS-focus, not an empty child mirror",
        );
        // The child does not get its OWN isolated mirror: a write through the
        // child's handle is the same cell the root reads.
        child
            .os_focused_window_signal()
            .set(Some("main".to_owned()));
        assert_eq!(root.run(os_focused_window).as_deref(), Some("main"));
    }

    #[test]
    fn reactive_scope_reruns_on_os_focus_change() {
        let owner = Owner::new();
        let signal = owner.os_focused_window_signal();
        let seen: Rc<StdRefCell<Vec<Option<String>>>> = Rc::new(StdRefCell::new(Vec::new()));
        let seen_e = Rc::clone(&seen);
        let _effect = owner.run(|| {
            Effect::new(&owner, move || {
                seen_e.borrow_mut().push(os_focused_window());
            })
        });
        assert_eq!(seen.borrow().as_slice(), &[None], "eager first run");

        // Writes wrapped in the owner scope so the synchronous Effect re-run
        // resolves `Owner::current()` (the publish-side "blocker B" discipline
        // the shell's `set_os_focused_window` follows).
        owner.run(|| signal.set(Some("main".to_owned())));
        owner.run(|| signal.set(None)); // whole-window blur
        assert_eq!(
            seen.borrow().as_slice(),
            &[None, Some("main".to_string()), None],
            "every OS-focus change (focus AND blur) re-runs the subscriber",
        );
    }

    #[test]
    fn republishing_the_same_window_is_a_no_op() {
        let owner = Owner::new();
        let signal = owner.os_focused_window_signal();
        let runs = Rc::new(StdRefCell::new(0_u32));
        let runs_e = Rc::clone(&runs);
        let _effect = owner.run(|| {
            Effect::new(&owner, move || {
                let _ = os_focused_window();
                *runs_e.borrow_mut() += 1;
            })
        });
        assert_eq!(*runs.borrow(), 1);
        owner.run(|| signal.set(Some("main".to_owned())));
        assert_eq!(*runs.borrow(), 2);
        owner.run(|| signal.set(Some("main".to_owned())));
        assert_eq!(
            *runs.borrow(),
            2,
            "an equal re-publish notifies nobody (no repaint churn on a redundant focus event)",
        );
    }
}
