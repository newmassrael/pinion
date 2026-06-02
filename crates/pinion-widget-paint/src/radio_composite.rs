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
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::radio::RadioState;
use pinion_core::{Color, WidgetStateName};

/// The shared M3 state-layer overlay for a composite radio cell: tint
/// `base` toward `OnSurface` by the hover (0.08) / pressed (0.12) opacity,
/// or toward `Surface` by the disabled (0.38) opacity; `Idle` is `base`
/// untinted (`Color::lerp` in linear space, [[color-lerp-linear-space]]).
///
/// Lifted at the Rule of Three (R750): `hello-radio-group`
/// (`radio_border_color`), `hello-breadcrumb` (`crumb`), and `hello-stepper`
/// (`step`) carried this exact 4-arm match byte-identically. The segmented
/// button (`segment_fill`) intentionally folds `Disabled` into `Idle` — its
/// base is the transparent track, so a disabled pill has nothing to tint —
/// and so keeps its own divergent 3-arm overlay, correctly NOT shared.
#[must_use]
pub fn state_layer(base: Color, state: RadioState, theme: &Theme) -> Color {
    match state {
        RadioState::Idle => base,
        RadioState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        RadioState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
        RadioState::Disabled => base.lerp(theme.resolve(ColorRole::Surface), 0.38),
    }
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
}
