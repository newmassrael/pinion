//! R664 §5.39 — programmatic focus-change request channel.
//!
//! Provides the substrate plumbing for a widget body
//! ([`External::invoke`](crate::external::External) handler, an
//! [`Effect`](crate::reactive::Effect) callback, a reducer body —
//! anywhere reactive code runs under an active
//! [`Owner`](crate::reactive::Owner) scope) to request that the focus
//! manager move focus to a specific paint tag *without* needing a
//! borrow on `pinion_runtime::FocusManager` (which lives in a
//! downstream crate and is unreachable from the widget body).
//!
//! ## Model
//!
//! [`request`] writes a target tag into a thread-local mailbox. The
//! shell substrate drains that mailbox at well-defined post-dispatch
//! sync points via [`drain`] and applies the request through
//! `FocusManager::focus_set` + `notify_focus_change` — the same path
//! a `click_to_focus` cycle uses, so `External::on_focus_change`
//! observers (the [`TextField`](crate::widgets::text_field::TextField)
//! IME bridge, the [`CaretBlink`](crate::widgets::caret_blink) gate,
//! …) fire identically whether the focus came from a mouse press or
//! a programmatic request.
//!
//! Last-write-wins: requesting focus twice in the same dispatch frame
//! collapses to the final tag (the most-recent caller's intent wins
//! the race). The same widget body that races itself is a programmer
//! error the substrate need not arbitrate.
//!
//! ## Why a `thread_local` mailbox over an `Owner` field
//!
//! `Owner` is reactive infrastructure that the shell may construct
//! per-dispatch wrap (`root_owner.run(...)`). A field on `Owner`
//! would force every focus request to flow through whichever owner
//! the caller currently sits under, and the shell would need to
//! enumerate every owner at drain time to discover requests. The
//! `thread_local` mailbox is the React `useImperativeHandle` /
//! `Solid createRef` / Flutter `FocusNode.requestFocus` shape —
//! imperative side channel, single drain point, zero cost when
//! unused (`Cell<Option<String>>::take` is a single conditional
//! pointer write on the empty-mailbox path).
//!
//! ## Bounded scope
//!
//! The mailbox holds *one* request — a request for a different tag
//! overwrites the previous one, on the same single-frame "last write
//! wins" basis above. Multi-target focus dispatch (focus rings, focus
//! traps for modal dialogs) is a separate axis and stays out of this
//! substrate; the [`crate::reactive::Owner`] cache or a per-widget
//! `Signal<Option<String>>` carries the multi-target story when a
//! consumer needs it.

use std::cell::Cell;

thread_local! {
    /// Pending focus-change request. Written by [`request`], read +
    /// cleared by [`drain`]. `Cell<Option<String>>` — last-write-wins
    /// semantics, zero allocation on the empty-mailbox path. The
    /// `String` storage covers both `&'static str` callers (literal
    /// widget tags like `"todo_edit"`) and dynamic callers
    /// (`format!("todo_edit#{id}")` if a multi-instance edit story
    /// ever surfaces — pinion sticks to single-slot today per
    /// `[[abstraction-needs-second-consumer]]`).
    static PENDING_FOCUS_REQUEST: Cell<Option<String>> = const { Cell::new(None) };
}

/// Request that focus move to `tag` at the next shell drain point.
///
/// Overwrites any prior pending request — last-write-wins for the
/// same dispatch frame. Safe to call from any thread (each thread
/// has its own mailbox); the shell's UI thread is the canonical
/// drainer.
///
/// Typical call sites:
/// - An `External::invoke` handler that flips an "edit-in-place"
///   signal and wants the new editor to receive keyboard input
///   immediately (R664 todomvc).
/// - A reducer body responding to a domain event that should refocus
///   a specific widget (modal close → restore caller's focus).
pub fn request(tag: impl Into<String>) {
    let value = tag.into();
    PENDING_FOCUS_REQUEST.with(|cell| cell.set(Some(value)));
}

/// Pop the pending focus-change request from this thread's mailbox.
/// Returns `Some(tag)` once per request — subsequent calls see
/// `None` until another [`request`] write.
///
/// Drained by the shell substrate after every dispatch path that
/// might have populated a request (`mouse_pressed`, `apply_key`,
/// `dispatch_rpc`, …) so the focus mutation happens before the
/// next paint cycle picks up the new editing widget.
#[must_use]
pub fn drain() -> Option<String> {
    PENDING_FOCUS_REQUEST.with(Cell::take)
}

/// R757 §5.39 — emit a roving-tabindex focus [`request`] for one of the
/// standard WAI-ARIA navigation keys, given the ordered list of sibling
/// tab-stop `tags` and the currently-`focused` tag.
///
/// Lifted from the byte-identical `apply_key` navigation block three
/// per-item-tab-stop bindings carried — `hello-accordion` (R697),
/// `hello-accordion-single`, and `hello-card` (R757). Each mapped the
/// same key set to the same wrapping move:
///
/// - `ArrowDown` / `ArrowRight` → next sibling (wrapping past the last);
/// - `ArrowUp` / `ArrowLeft` → previous sibling (wrapping before the first);
/// - `Home` → first sibling;
/// - `End` → last sibling.
///
/// Returns `true` — and emits the [`request`] — when `key` is one of
/// those navigation keys and `focused` is present in `tags`. Returns
/// `false` otherwise, so the caller falls through to its own keymap.
/// Widget *activation* (`Space` / `Enter`) stays per-binding because the
/// activation semantics diverge (a disclosure toggles, a card
/// activates), but the roving *navigation* is WAI-ARIA standard topology
/// that must be identical across every consumer, so it lives here once
/// ([[abstraction-needs-second-consumer]] — the divergence-is-a-bug
/// class, lifted at the 3rd identical copy per the R727/R732 self-grep
/// mandate). `tags` carries `&'static str` because [`request`]'s focus
/// targets are the same `'static` widget-tag literals the bindings'
/// `focusable_tags` enumerate.
///
/// This is the **per-item-tab-stop** roving axis (each sibling is its
/// own document tab stop; the Arrow keys move *shell focus* between
/// them), orthogonal to `pinion_widget_paint::radio_composite::roving_key`,
/// which is the **single-tab-stop** axis (the whole widget is one tab
/// stop and the Arrow keys move an *internal* active-descendant cursor
/// within it). The two roving families have different mechanisms — a
/// focus-request mailbox write vs a composite `"<i>:<Event>"` activation
/// cycle — so each lives at its own home; neither subsumes the other.
#[must_use]
pub fn rove(tags: &[&'static str], focused: &str, key: &str) -> bool {
    let Some(idx) = tags.iter().position(|t| *t == focused) else {
        return false;
    };
    // `position` succeeded ⇒ `tags` is non-empty, so the modular
    // arithmetic below never divides by zero.
    let n = tags.len();
    let target = match key {
        "ArrowDown" | "ArrowRight" => tags[(idx + 1) % n],
        "ArrowUp" | "ArrowLeft" => tags[(idx + n - 1) % n],
        "Home" => tags[0],
        "End" => tags[n - 1],
        _ => return false,
    };
    request(target);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAGS: [&str; 3] = ["a", "b", "c"];

    #[test]
    fn request_then_drain_returns_the_tag() {
        request("alpha");
        assert_eq!(drain(), Some("alpha".to_string()));
    }

    #[test]
    fn drain_on_empty_mailbox_returns_none() {
        // Defensive — earlier tests in the same thread may have
        // populated the mailbox. Clear first.
        let _ = drain();
        assert_eq!(drain(), None);
    }

    #[test]
    fn drain_clears_the_mailbox() {
        request("beta");
        let _ = drain();
        assert_eq!(drain(), None);
    }

    #[test]
    fn second_request_overwrites_the_first() {
        request("first");
        request("second");
        assert_eq!(drain(), Some("second".to_string()));
    }

    #[test]
    fn accepts_owned_string_payload() {
        // `format!()` output is the dynamic call site shape — confirm
        // the `impl Into<String>` bound covers it without an extra
        // `.to_string()`.
        let tag = format!("todo_edit#{}", 42_u64);
        request(tag);
        assert_eq!(drain(), Some("todo_edit#42".to_string()));
    }

    // ── rove (R757 roving-tabindex navigation) ────────────────────────

    #[test]
    fn rove_arrow_next_wraps_past_the_last() {
        let _ = drain();
        assert!(rove(&TAGS, "c", "ArrowRight"));
        assert_eq!(drain(), Some("a".to_string()), "ArrowRight from last wraps to first");
        assert!(rove(&TAGS, "a", "ArrowDown"));
        assert_eq!(drain(), Some("b".to_string()), "ArrowDown is the same next move");
    }

    #[test]
    fn rove_arrow_prev_wraps_before_the_first() {
        let _ = drain();
        assert!(rove(&TAGS, "a", "ArrowLeft"));
        assert_eq!(drain(), Some("c".to_string()), "ArrowLeft from first wraps to last");
        assert!(rove(&TAGS, "c", "ArrowUp"));
        assert_eq!(drain(), Some("b".to_string()), "ArrowUp is the same previous move");
    }

    #[test]
    fn rove_home_and_end_jump_to_the_edges() {
        let _ = drain();
        assert!(rove(&TAGS, "b", "Home"));
        assert_eq!(drain(), Some("a".to_string()));
        assert!(rove(&TAGS, "b", "End"));
        assert_eq!(drain(), Some("c".to_string()));
    }

    #[test]
    fn rove_ignores_non_navigation_keys_and_emits_nothing() {
        let _ = drain();
        assert!(!rove(&TAGS, "b", "Space"), "activation keys are not roving keys");
        assert!(!rove(&TAGS, "b", "x"), "arbitrary keys are not roving keys");
        assert_eq!(drain(), None, "a non-navigation key emits no focus request");
    }

    #[test]
    fn rove_returns_false_when_focused_tag_is_absent() {
        let _ = drain();
        assert!(!rove(&TAGS, "not_here", "ArrowRight"));
        assert_eq!(drain(), None, "an unknown focused tag emits no focus request");
    }
}
