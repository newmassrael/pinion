//! R732 §5.40 — shared binding mechanics for the **1-D composite
//! radio-group widget shape**: a single
//! [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal)
//! painted as `N` tagged cells (`"<tag>#<i>"`) with a single-tab-stop
//! roving active descendant. The current location is a 1-of-N exclusive
//! selection.
//!
//! ## Why this lives here (the Rule-of-Three lift)
//!
//! Three bindings — `hello-radio-group` (R51.44), `hello-segmented-button`
//! (R728), and `hello-breadcrumb` (R731) — carried **byte-identical**
//! roving / activation / read-back / child-invoke / composite-focus
//! mechanics (only their paint, a11y roles, key map, `N`, and labels
//! differ). At the third consumer the Rule of Three fired, so the
//! *mechanical* core is lifted to one source of truth. This mirrors the
//! `pinion_core::widgets::aria::apply_aria_activate` lift (R51.114, "4
//! binding `apply_key` DRY 청산").
//!
//! What stays per-binding (the *opinionated* parts, correctly NOT shared):
//! the `view` paint, the AccessKit roles (`radiogroup`/`radio` vs
//! `navigation`/`link`+`aria-current`), the key→index map, `N`, and the
//! crumb / segment / radio labels. This module owns only the wiring that
//! was provably identical.

use pinion_a11y::{AccessAction, AccessFocus};
use pinion_core::external::{ExternalIntrospect, IntrospectValue};
use pinion_core::widgets::radio::RadioState;
use pinion_core::{Scene, WidgetStateName};

/// The shared roving-key `apply_key` shell for a composite radio widget:
/// when `focused` names this widget's `tag`, resolve `key` to a target cell
/// index through the per-binding `resolve` map (the *opinionated* part —
/// which arrows step, `Home`/`End`, etc.) and drive the activation cycle on
/// that cell. Returns whether the key was consumed.
///
/// Lifted at the Rule of Three (R751): breadcrumb (R731), segmented button
/// (R728), radio-group (R51.44), stepper (R750), and nav-rail (R751) all
/// carried this exact mechanical shell — only `resolve` (the key→index map)
/// and the widget's `tag` differed. (R750's self-grep lifted the
/// [`state_layer`] overlay but missed this shell; R751 clears it — the
/// incomplete-self-grep failure mode the 3rd-consumer mandate warns of.)
#[must_use]
pub fn roving_key(
    scene: &mut Scene,
    focused: Option<&str>,
    tag: &str,
    key: &str,
    resolve: impl Fn(Option<&dyn ExternalIntrospect>, &str) -> Option<usize>,
) -> bool {
    if focused != Some(tag) {
        return false;
    }
    let Scene::External(node) = scene else {
        return false;
    };
    let Some(idx) = resolve(node.handle.introspect(), key) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    drive_activate(intro, idx);
    true
}

/// Drive the full WAI-ARIA activation cycle (`PointerEnter` → `Down` →
/// `Up` → `Leave`) against cell `idx` through the composite `"<i>:<Event>"`
/// wire format. `PointerUp` is the activate edge; the trailing `Leave`
/// returns the cell's interaction state to `Idle` so no phantom `Hover`
/// lingers. `RadioGroup::send` enforces 1-of-N exclusion and fires the
/// §5.20 `"selected"` intent on the real selection change.
pub fn drive_activate(intro: &mut dyn ExternalIntrospect, idx: usize) {
    for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
        let _ = intro.invoke("send", IntrospectValue::Text(format!("{idx}:{ev}")));
    }
}

/// The currently selected (current) cell index, or `None`.
#[must_use]
pub fn selected_index(intro: &dyn ExternalIntrospect) -> Option<usize> {
    match intro.query("selected_index") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// The AT-side roving active-descendant index, or `None`.
#[must_use]
pub fn focused_index(intro: &dyn ExternalIntrospect) -> Option<usize> {
    match intro.query("focused_index") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// Cyclic arrow step from the current selection: `direction > 0` moves
/// forward, `<= 0` back, wrapping at the ends (the ARIA cyclic-ring
/// convention). With no selection a forward step lands on `0` and a back
/// step on `n - 1`. `n == 0` yields `0`.
#[must_use]
pub fn step(intro: Option<&dyn ExternalIntrospect>, direction: i32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let current = intro.and_then(selected_index);
    match (current, direction > 0) {
        (Some(c), true) => (c + 1) % n,
        (Some(c), false) => (c + n - 1) % n,
        (None, true) => 0,
        (None, false) => n - 1,
    }
}

/// Read each cell's `(RadioState, selected)` projection from the
/// `state.<i>` / `selected.<i>` introspect slots into `rows` (indexed by
/// cell). The single source of truth for the view + a11y read-back.
pub fn read_rows(intro: &dyn ExternalIntrospect, rows: &mut [(RadioState, bool)]) {
    for (i, slot) in rows.iter_mut().enumerate() {
        let state = match intro.query(&format!("state.{i}")) {
            Some(IntrospectValue::Text(name)) => RadioState::from_name_or_default(&name),
            _ => RadioState::Idle,
        };
        let selected = matches!(
            intro.query(&format!("selected.{i}")),
            Some(IntrospectValue::Bool(true)),
        );
        *slot = (state, selected);
    }
}

/// The active-descendant cell: the AT-pinned `focused` index if set, else
/// the selected (current) cell, else `0`.
#[must_use]
pub fn active_index(rows: &[(RadioState, bool)], focused: Option<usize>) -> usize {
    if let Some(idx) = focused {
        return idx;
    }
    rows.iter().position(|(_, sel)| *sel).unwrap_or(0)
}

/// The composite AccessKit focus target: the parent (`tag`) owns the tab
/// stop and the active cell (`"<tag>#<active_idx>"`) is the
/// `aria-activedescendant` (WAI-ARIA roving-tabindex).
#[must_use]
pub fn composite_focus(tag: &str, active_idx: usize) -> AccessFocus {
    AccessFocus::composite(tag, format!("{tag}#{active_idx}"))
}

/// AT child-action dispatch for a composite cell sub-tag (`"<i>"`):
/// `Click` / `Default` activate cell `idx` (1-of-N exclusion + the
/// `"selected"` intent), `Focus` pins the active descendant via the
/// `focused_index` intervene without mutating the selection, and other
/// actions decline (so the shell keeps its fallback chain). Returns
/// whether the action was handled. Out-of-range / non-numeric sub-tags
/// return `false`.
pub fn child_invoke(
    intro: &mut dyn ExternalIntrospect,
    sub_tag: &str,
    action: AccessAction,
    n: usize,
) -> bool {
    let Ok(idx) = sub_tag.parse::<usize>() else {
        return false;
    };
    if idx >= n {
        return false;
    }
    match action {
        AccessAction::Click | AccessAction::Default => {
            drive_activate(intro, idx);
            true
        }
        AccessAction::Focus => {
            if let Ok(i) = i64::try_from(idx) {
                let _ = intro.intervene("focused_index", IntrospectValue::Int(i));
            }
            true
        }
        AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
    }
}

/// The complete [`pinion_a11y::WidgetA11y::access_focus_target`] body for a
/// composite radio-group binding: when the group (`group_tag`) holds shell
/// focus, report the parent as the focus target with
/// `"<group_tag>#<active_idx>"` as the `aria-activedescendant` (the
/// roving-tabindex model); otherwise pass any sibling focus through
/// atomically.
///
/// R758 §5.40 — lifted at the 7th byte-identical consumer (radio-group /
/// segmented / nav-rail / breadcrumb / stepper / pagination / rating all
/// carried this exact if/else over [`composite_focus`]). The caller
/// computes `active_idx` from its own state projection (via
/// [`active_index`]); the focus-model policy lives here once.
#[must_use]
pub fn composite_focus_target(
    group_tag: &str,
    focused: Option<&str>,
    active_idx: usize,
) -> Option<AccessFocus> {
    if focused == Some(group_tag) {
        Some(composite_focus(group_tag, active_idx))
    } else {
        focused.map(AccessFocus::atomic)
    }
}

/// The complete [`pinion_a11y::WidgetA11y::access_child_invoke`] body for a
/// composite radio-group binding whose root paint node is the single
/// `Scene::External`: descend to the composite handle and dispatch the AT
/// `action` to cell `sub_tag` via [`child_invoke`]. Returns `false` when
/// the scene root is not an external / exposes no introspection — the same
/// decline path each binding spelled out by hand.
///
/// R758 §5.40 — lifted at the 6th byte-identical consumer. Bindings whose
/// `access_child_invoke` also dispatches *non-cell* sub-targets (e.g.
/// pagination's `prev` / `next` chevrons) keep their bespoke `match` and
/// may still delegate the numeric-cell arm to [`child_invoke`].
pub fn composite_child_invoke(
    scene: &mut Scene,
    sub_tag: &str,
    action: AccessAction,
    n: usize,
) -> bool {
    let Scene::External(node) = scene else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    child_invoke(intro, sub_tag, action, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::External;
    use pinion_core::widgets::radio_group::RadioGroupExternal;

    fn group() -> RadioGroupExternal {
        RadioGroupExternal::new(3)
    }

    #[test]
    fn drive_activate_selects_and_settles_idle() {
        let mut g = group();
        drive_activate(&mut g, 1);
        assert_eq!(g.selected_index(), Some(1));
        assert_eq!(g.state(1), RadioState::Idle, "trailing Leave returns to Idle");
    }

    #[test]
    fn step_wraps_both_directions() {
        let mut g = group();
        // No selection: forward -> 0, back -> n-1.
        assert_eq!(step(g.introspect(), 1, 3), 0);
        assert_eq!(step(g.introspect(), -1, 3), 2);
        drive_activate(&mut g, 2);
        assert_eq!(step(g.introspect(), 1, 3), 0, "2 + 1 wraps to 0");
        assert_eq!(step(g.introspect(), -1, 3), 1, "2 - 1 = 1");
    }

    #[test]
    fn step_zero_n_is_zero() {
        assert_eq!(step(None, 1, 0), 0);
    }

    #[test]
    fn read_rows_projects_state_and_selection() {
        let mut g = group();
        drive_activate(&mut g, 0);
        let mut rows = [(RadioState::Idle, false); 3];
        read_rows(&g, &mut rows);
        assert_eq!(rows[0], (RadioState::Idle, true));
        assert_eq!(rows[1], (RadioState::Idle, false));
    }

    #[test]
    fn active_index_prefers_focused_then_selected_then_zero() {
        let rows = [(RadioState::Idle, false), (RadioState::Idle, true), (RadioState::Idle, false)];
        assert_eq!(active_index(&rows, Some(2)), 2, "focused wins");
        assert_eq!(active_index(&rows, None), 1, "else selected");
        let none = [(RadioState::Idle, false); 3];
        assert_eq!(active_index(&none, None), 0, "else 0");
    }

    #[test]
    fn composite_focus_targets_parent_with_active_descendant() {
        let f = composite_focus("nav", 2);
        assert_eq!(f.focus_tag, "nav");
        assert_eq!(f.active_descendant.as_deref(), Some("nav#2"));
    }

    #[test]
    fn child_invoke_click_activates_and_focus_pins_without_select() {
        let mut g = group();
        assert!(child_invoke(&mut g, "1", AccessAction::Click, 3));
        assert_eq!(g.selected_index(), Some(1));
        // Focus on another cell does not move the selection.
        assert!(child_invoke(&mut g, "0", AccessAction::Focus, 3));
        assert_eq!(g.selected_index(), Some(1), "Focus is non-mutating");
        assert_eq!(g.focused_index(), Some(0));
    }

    #[test]
    fn child_invoke_out_of_range_and_non_numeric_decline() {
        let mut g = group();
        assert!(!child_invoke(&mut g, "9", AccessAction::Click, 3));
        assert!(!child_invoke(&mut g, "foo", AccessAction::Click, 3));
        assert!(!child_invoke(&mut g, "0", AccessAction::Increment, 3));
    }

    // ── R758 composite a11y wrapper lifts ──────────────────────────────

    #[test]
    fn composite_focus_target_parent_when_group_focused() {
        let f = composite_focus_target("nav", Some("nav"), 2).expect("group focused -> Some");
        assert_eq!(f.focus_tag, "nav");
        assert_eq!(f.active_descendant.as_deref(), Some("nav#2"));
    }

    #[test]
    fn composite_focus_target_atomic_for_sibling_and_none_for_unfocused() {
        let sibling = composite_focus_target("nav", Some("save_btn"), 0)
            .expect("a focused sibling -> Some(atomic)");
        assert_eq!(sibling.focus_tag, "save_btn");
        assert!(sibling.active_descendant.is_none(), "atomic carries no descendant");
        assert!(composite_focus_target("nav", None, 0).is_none(), "no focus -> None");
    }

    #[test]
    fn composite_child_invoke_descends_external_and_dispatches() {
        use pinion_core::scene::ExternalNode;
        let mut scene =
            Scene::External(ExternalNode::new(Box::new(group())).with_tag("nav"));
        assert!(composite_child_invoke(&mut scene, "2", AccessAction::Click, 3));
        let Scene::External(node) = &scene else { unreachable!() };
        assert_eq!(
            node.handle.introspect().and_then(selected_index),
            Some(2),
            "AT click on cell 2 selects it through the lifted wrapper",
        );
    }

    #[test]
    fn composite_child_invoke_declines_non_external_root() {
        use pinion_core::scene::ContainerNode;
        let mut not_external = Scene::Container(ContainerNode::new(vec![]));
        assert!(!composite_child_invoke(&mut not_external, "0", AccessAction::Click, 3));
    }
}
