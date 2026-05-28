//! R51.114 §5.38 / §5.41 — ARIA keyboard activation helper.
//!
//! WAI-ARIA Authoring Practices defines a small set of activation
//! key patterns each interactive role accepts:
//!
//! - **Button / Switch / Toggle button**: `Space` or `Enter` →
//!   activate. The two keys are interchangeable on these roles
//!   because the user-visible affordance is "press to act".
//! - **Checkbox**: `Space` only → toggle (Enter is reserved for
//!   form submit, not the checkbox itself).
//! - **Radio**: arrow-key navigation within the group; `Space`
//!   toggles the focused radio when not in `aria-activedescendant`
//!   mode.
//! - **Slider**: arrow keys / `Home` / `End` / `PageUp` /
//!   `PageDown` — value mutation, not an activate edge.
//!
//! This module collects the **Activate** family helper because
//! Button, Switch / Toggle, and any future "press to act" role
//! land on the same `(Space | Enter) → invoke("send",
//! "KeyboardActivate")` dispatch. Checkbox / Radio / Slider helpers
//! land in their respective modules once a second TUI binding for
//! each surfaces the same DRY trigger
//! ([[substrate-incompleteness-signal]]).
//!
//! ## Why a free function instead of a trait method
//!
//! The pre-R51.114 cut had four bindings (`hello-button`,
//! `hello-button-tui`, `hello-toggle`, `hello-toggle-tui`) carrying
//! identical 10-line `apply_key` impls — 40 LOC of boilerplate
//! across the catalogue. A trait-default-method approach (lifting
//! `apply_key` into a shared `WidgetDispatch` supertrait) would
//! reduce LOC but force every binding's `impl` block to split into
//! a `WidgetDispatch` half + a backend-specific half, surrendering
//! the single-impl readability gain. The free-function approach
//! keeps each binding's `impl` block self-contained while collapsing
//! the activation body to a single line, which is the textbook
//! WAI-ARIA refactor pattern (`role-specific helper`, see WAI-ARIA
//! APG keyboard implementation guides).

use crate::external::IntrospectValue;
use crate::scene::Scene;

/// R51.114 §5.38 / §5.41 — apply an ARIA-compliant
/// activate-on-`Space`-or-`Enter` keystroke against a focused
/// widget's SCXML statechart.
///
/// Mirrors the WAI-ARIA Authoring Practices Button (3.6) and
/// Switch (3.27) keyboard patterns: when the widget tagged
/// `my_tag` holds keyboard focus and the user presses `Space` or
/// `Enter`, fire the SCXML `KeyboardActivate` event through the
/// §5.15 `invoke("send", Text("KeyboardActivate"))` channel. The
/// widget's `Button::detect` / `Toggle::detect` substrate emits
/// the `"click"` / `"toggle"` intent on the resulting transition,
/// reaching the application via the shared `walk_scene_and_drain`
/// path the shell drives after every dispatched event.
///
/// Returns `true` when the activation actually fired:
///
/// - `false` if `focused` is not `Some(my_tag)` — focus gates so
///   simultaneous focusable widgets do not double-fire on a
///   single keystroke.
/// - `false` if `key` is neither `"Space"` nor `"Enter"`.
/// - `false` if no [`Scene::External`] tagged `my_tag` is reachable in
///   `scene`, or its handle has no
///   [`crate::external::ExternalIntrospect`] surface (the "no
///   introspect = no activation" rule from §5.15 — an `External`
///   without an introspect surface is a paint-only or pure-side-effect
///   node).
/// - `false` if the introspect's `invoke("send", ...)` rejects the
///   call (e.g. the widget is `Disabled` and the SCXML template
///   has no activation transition from that state).
/// - `true` when the SCXML statechart accepted `KeyboardActivate`.
///
/// The same helper covers both the Vello (`pinion-shell`) and TUI
/// (`pinion-tui`) bindings because the activation contract is
/// backend-agnostic — only the input source differs (winit
/// `KeyEvent::Pressed` vs crossterm `Event::Key`), and that
/// translation happens shell-side before this helper sees the
/// already-abstract W3C key string.
///
/// R693.A §5.38 — resolves the target via
/// [`Scene::find_external_with_tag_mut`], so it works for both the
/// single-widget shape (`scene` IS the `Scene::External(my_tag)`) AND
/// the multi-External composed shape (`scene` is a
/// `Container([primary, ...extras])` and `my_tag` names one of the
/// children, e.g. a dialog's action button). Before R693.A the body
/// assumed `scene` was directly the `Scene::External`, so a
/// multi-External binding's Container root silently failed every
/// activation; `hello-dialog` was the first such consumer
/// ([[substrate-incompleteness-signal]]).
pub fn apply_aria_activate(
    scene: &mut Scene,
    focused: Option<&str>,
    key: &str,
    my_tag: &str,
) -> bool {
    if focused != Some(my_tag) {
        return false;
    }
    if !matches!(key, "Space" | "Enter") {
        return false;
    }
    let Some(node) = scene.find_external_with_tag_mut(my_tag) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    intro
        .invoke("send", IntrospectValue::Text("KeyboardActivate".to_string()))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::ExternalNode;
    use crate::widgets::button::ButtonExternal;

    fn make_scene(tag: &'static str) -> Scene {
        Scene::External(
            ExternalNode::new(Box::new(ButtonExternal::new())).with_tag(tag),
        )
    }

    #[test]
    fn space_on_focused_button_fires_activation() {
        // R51.114 — happy path: Button focused, Space pressed →
        // KeyboardActivate reaches the SCXML, which `Button::detect`
        // converts into a click intent on the next drain.
        let mut scene = make_scene("btn");
        let fired = apply_aria_activate(&mut scene, Some("btn"), "Space", "btn");
        assert!(fired);
    }

    #[test]
    fn enter_on_focused_button_fires_activation() {
        // ARIA Authoring Practices: Enter and Space are
        // interchangeable on Button — both must fire.
        let mut scene = make_scene("btn");
        let fired = apply_aria_activate(&mut scene, Some("btn"), "Enter", "btn");
        assert!(fired);
    }

    #[test]
    fn unfocused_widget_does_not_fire() {
        // Focus gate: a Space press while focus is elsewhere must
        // not trigger this widget's activation (would alias when
        // multiple buttons live on the same screen).
        let mut scene = make_scene("btn");
        let fired = apply_aria_activate(&mut scene, Some("other"), "Space", "btn");
        assert!(!fired);
    }

    #[test]
    fn no_focus_does_not_fire() {
        // `None` focus = no widget is the activation target.
        let mut scene = make_scene("btn");
        let fired = apply_aria_activate(&mut scene, None, "Space", "btn");
        assert!(!fired);
    }

    #[test]
    fn other_keys_do_not_fire() {
        // Only Space + Enter are activation keys for Button per
        // ARIA. Other keys fall through to the framework's next
        // dispatch arm (or are absorbed silently).
        let mut scene = make_scene("btn");
        let fired = apply_aria_activate(&mut scene, Some("btn"), "ArrowLeft", "btn");
        assert!(!fired);
        let fired = apply_aria_activate(&mut scene, Some("btn"), "a", "btn");
        assert!(!fired);
        let fired = apply_aria_activate(&mut scene, Some("btn"), "Tab", "btn");
        assert!(!fired);
    }

    #[test]
    fn empty_container_scene_does_not_fire() {
        // A Container with no External tagged `my_tag` has no
        // activation target — find_external_with_tag_mut returns None.
        use crate::scene::ContainerNode;
        let mut scene = Scene::Container(ContainerNode::default());
        let fired = apply_aria_activate(&mut scene, Some("btn"), "Space", "btn");
        assert!(!fired);
    }

    #[test]
    fn r693a_descends_container_to_focused_external() {
        // R693.A — multi-External composed shape: the focused button is
        // a child of a Container (the hello-dialog action-button case).
        // The helper must descend find_external_with_tag_mut, not assume
        // `scene` is the External directly.
        use crate::scene::ContainerNode;
        let mut scene = Scene::Container(ContainerNode::new(vec![
            make_scene("dialog_ok"),
            make_scene("dialog_cancel"),
        ]));
        // Enter on the focused cancel button fires only the cancel
        // child; the chained-or call shape a binding uses resolves to
        // the matching tag.
        assert!(!apply_aria_activate(&mut scene, Some("dialog_cancel"), "Enter", "dialog_ok"));
        assert!(apply_aria_activate(&mut scene, Some("dialog_cancel"), "Enter", "dialog_cancel"));
    }
}
