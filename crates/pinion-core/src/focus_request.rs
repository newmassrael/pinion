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
/// targets are the same `'static` widget-tag literals the bindings mark
/// `.with_focusable(true)` for the R1020 §5.39 scene-derived enumeration.
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

/// R760 §5.39 — keyboard handler for a group of **independently-focusable,
/// individually-activatable tab stops** (a list of buttons: accordion
/// headers, clickable cards, FABs). `Space` / `Enter` activates the
/// `focused` target by sending it a plain `"KeyboardActivate"` event
/// through the §5.15 introspect channel (the button-activation edge); the
/// Arrow / `Home` / `End` keys rove shell focus via [`rove`]. Returns
/// whether `key` was consumed.
///
/// Lifted at the 3rd byte-identical consumer (`hello-accordion` R697,
/// `hello-card` R757, `hello-fab` R760) per the R727 / R732 self-grep
/// mandate. The R757 [`rove`] doc deferred activation as "per-binding
/// because the activation semantics diverge"; the re-audit at this 3rd
/// consumer found the divergence lives in each *`External`'s response* to
/// `KeyboardActivate` (a disclosure toggles, a card / FAB fires `click`),
/// **not** in the `apply_key` *wiring*, which is identical — so the wiring
/// lifts here while the per-widget statechart response stays in the
/// widget. `hello-accordion-single` is *not* a consumer: its sub-targets
/// are composite, so it sends `"{idx}:KeyboardActivate"` (a divergent wire
/// payload that genuinely stays per-binding, the pagination-child_invoke
/// precedent).
#[must_use]
pub fn activate_or_rove(
    scene: &mut crate::scene::Scene,
    tags: &[&'static str],
    focused: Option<&str>,
    key: &str,
) -> bool {
    let Some(focused_tag) = focused else {
        return false;
    };
    if matches!(key, "Space" | "Enter") {
        let Some(node) = scene.find_external_with_tag_mut(focused_tag) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        return intro
            .invoke(
                "send",
                crate::external::IntrospectValue::Text("KeyboardActivate".to_string()),
            )
            .is_ok();
    }
    rove(tags, focused_tag, key)
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

    // ── activate_or_rove (R760 button-cluster keyboard SSOT) ──────────

    use crate::scene::{ContainerNode, ExternalNode, Scene};
    use crate::widgets::button::ButtonExternal;

    const BTN_TAGS: [&str; 2] = ["b0", "b1"];

    /// A two-button cluster scene mirroring the shell's boot composition
    /// (`Scene::Container([primary, ...extras])`).
    fn button_cluster() -> Scene {
        let children = BTN_TAGS
            .iter()
            .map(|t| Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())).with_tag(*t)))
            .collect();
        Scene::Container(ContainerNode::new(children))
    }

    /// The button's `state` introspect slot as a raw name string.
    fn button_state_name(scene: &Scene, tag: &str) -> String {
        let intro = scene
            .find_external_with_tag(tag)
            .and_then(|n| n.handle.introspect())
            .expect("button external present");
        match intro.query("state") {
            Some(crate::external::IntrospectValue::Text(s)) => s,
            _ => panic!("button exposes a Text state"),
        }
    }

    #[test]
    fn activate_or_rove_space_activates_the_focused_button() {
        let mut scene = button_cluster();
        // Enter the focused button first so the activation edge has a
        // posture to act on (Idle -> Hover -> ... the click edge needs an
        // entered button); drive PointerEnter via the send wire.
        if let Some(n) = scene.find_external_with_tag_mut("b0") {
            let _ = n.handle.introspect_mut().unwrap().invoke(
                "send",
                crate::external::IntrospectValue::Text("PointerEnter".to_string()),
            );
        }
        assert!(
            activate_or_rove(&mut scene, &BTN_TAGS, Some("b0"), "Space"),
            "Space activates the focused button",
        );
        // KeyboardActivate is accepted by the button statechart (the
        // witness that the send wire routed); the cluster sibling is
        // untouched.
        assert_eq!(button_state_name(&scene, "b1"), "Idle", "sibling untouched");
    }

    #[test]
    fn activate_or_rove_enter_is_also_an_activation_key() {
        let mut scene = button_cluster();
        assert!(activate_or_rove(&mut scene, &BTN_TAGS, Some("b0"), "Enter"));
    }

    #[test]
    fn activate_or_rove_arrow_delegates_to_rove() {
        let _ = drain();
        let mut scene = button_cluster();
        assert!(
            activate_or_rove(&mut scene, &BTN_TAGS, Some("b0"), "ArrowRight"),
            "an arrow key roves",
        );
        assert_eq!(drain().as_deref(), Some("b1"), "rove emitted the next-sibling focus request");
    }

    #[test]
    fn activate_or_rove_returns_false_without_focus() {
        let mut scene = button_cluster();
        assert!(!activate_or_rove(&mut scene, &BTN_TAGS, None, "Space"));
        assert!(!activate_or_rove(&mut scene, &BTN_TAGS, None, "ArrowRight"));
    }

    #[test]
    fn activate_or_rove_unknown_key_is_not_consumed() {
        let _ = drain();
        let mut scene = button_cluster();
        assert!(!activate_or_rove(&mut scene, &BTN_TAGS, Some("b0"), "x"));
        assert_eq!(drain(), None, "a non-activation non-nav key emits nothing");
    }
}
